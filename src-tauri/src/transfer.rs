//! Export, import and backup commands of the Settings page ("Backup & transfer") and the
//! profile list. The dialogs are opened here, in Rust: the windows get no file permissions and
//! never see a path, only what the user picked.

use crate::{Shared, overlay};
use detent_core::backup::{self, BackupInfo};
use detent_core::transfer::{
    self, EXPORT_EXTENSION, ImportSummary, MergeReport, parse_import, prepare_replace,
};
use detent_core::{Category, Record, Setup};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;

type Res<T> = Result<T, String>;

/// Import files are small; anything larger is not one.
const MAX_IMPORT_BYTES: u64 = 32 * 1024 * 1024;

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// A file name part that Windows accepts.
fn file_safe(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' || c == ' ' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let cleaned = cleaned.trim().to_owned();
    if cleaned.is_empty() {
        "profile".into()
    } else {
        cleaned
    }
}

/// Saves the configuration (or the profiles with the given ids) to a file the user chooses.
/// Resolves with the path, or `None` when the dialog was cancelled. Credentials are left out.
#[tauri::command]
pub async fn export_setup(
    app: AppHandle,
    shared: State<'_, Arc<Shared>>,
    profiles: Option<Vec<String>>,
) -> Res<Option<String>> {
    let (text, name) = {
        let engine = shared.engine();
        let now = now_secs();
        let text = transfer::export_json(
            engine.setup(),
            profiles.as_deref(),
            now,
            &shared.meta.version,
        )
        .map_err(|e| e.to_string())?;
        let name = match profiles.as_deref() {
            Some([only]) => engine
                .setup()
                .profiles
                .iter()
                .find(|p| &p.id == only)
                .map_or("profile".into(), |p| file_safe(&p.name)),
            Some(_) => "profiles".into(),
            None => format!("detent-{}", &backup::stamp(now)[..8]),
        };
        (text, format!("{name}.{EXPORT_EXTENSION}"))
    };
    let picked = tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .set_title("Export Detent configuration")
            .set_file_name(&name)
            .add_filter("Detent export", &["json"])
            .blocking_save_file()
    })
    .await
    .map_err(|e| e.to_string())?;
    let Some(picked) = picked else {
        return Ok(None);
    };
    let mut path: PathBuf = picked.into_path().map_err(|e| e.to_string())?;
    if path
        .extension()
        .is_none_or(|e| !e.eq_ignore_ascii_case("json"))
    {
        let mut name = path.file_name().unwrap_or_default().to_os_string();
        name.push(".json");
        path.set_file_name(name);
    }
    std::fs::write(&path, &text).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
    log::info!("exported to {}", path.display());
    shared.diagnostics.record(
        Record::new(Category::Log, "Configuration exported").detail(path.display().to_string()),
    );
    Ok(Some(path.display().to_string()))
}

/// Opens a file dialog, reads and checks the file, and keeps it for [`apply_import`]. Resolves
/// with what the file holds, or `None` when the dialog was cancelled. A file that cannot be
/// imported is an error with the reason.
#[tauri::command]
pub async fn pick_import_file(
    app: AppHandle,
    shared: State<'_, Arc<Shared>>,
) -> Res<Option<ImportSummary>> {
    let picked = tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .set_title("Import Detent configuration")
            .add_filter("Detent export or setup", &["json"])
            .blocking_pick_file()
    })
    .await
    .map_err(|e| e.to_string())?;
    let Some(picked) = picked else {
        return Ok(None);
    };
    let path = picked.into_path().map_err(|e| e.to_string())?;
    let size = std::fs::metadata(&path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?
        .len();
    if size > MAX_IMPORT_BYTES {
        return Err("this file is too large to be a Detent export".into());
    }
    let text = std::fs::read_to_string(&path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    match parse_import(&text) {
        Ok(file) => {
            let summary = file.summary();
            *shared
                .pending_import
                .lock()
                .unwrap_or_else(|e| e.into_inner()) = Some(file);
            Ok(Some(summary))
        }
        Err(e) => {
            log::warn!("import of {} refused: {e}", path.display());
            Err(e.to_string())
        }
    }
}

/// Forgets the picked file.
#[tauri::command]
pub fn cancel_import(shared: State<'_, Arc<Shared>>) {
    *shared
        .pending_import
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = None;
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum ImportMode {
    ReplaceAll,
    AddProfiles { profiles: Vec<String> },
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportResult {
    pub message: String,
    /// What was added, for "add profiles".
    pub report: Option<MergeReport>,
}

/// Tells the connectors, the overlay window and the hotkey about a replaced setup.
fn apply_replaced(app: &AppHandle, shared: &Shared, old_hotkey: &str) {
    let (connections, overlay_config) = {
        let engine = shared.engine();
        (engine.connections().clone(), engine.overlay())
    };
    shared.soundpad.apply(connections.soundpad);
    shared.discord.apply(connections.discord);
    if old_hotkey != overlay_config.hotkey {
        overlay::unregister_hotkey(app, old_hotkey);
        if let Err(e) = overlay::register_hotkey(app, &overlay_config.hotkey) {
            log::warn!("{e}; keeping the previous overlay hotkey {old_hotkey}");
            let _ = overlay::register_hotkey(app, old_hotkey);
            let mut engine = shared.engine();
            let mut overlay = engine.overlay();
            overlay.hotkey = old_hotkey.to_owned();
            engine.set_overlay(overlay);
            shared.commit(&mut engine);
        }
    }
    overlay::set_visible(app, overlay_config.visible);
    overlay::cover_monitor(app, overlay_config.monitor.as_ref());
}

/// Applies the picked file. A backup of the current configuration is written first; if that
/// fails, nothing is imported.
#[tauri::command]
pub fn apply_import(
    app: AppHandle,
    shared: State<'_, Arc<Shared>>,
    mode: ImportMode,
) -> Res<ImportResult> {
    let file = shared
        .pending_import
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
        .ok_or("no import file is open; choose a file first")?;
    let mut engine = shared.engine();
    let before = engine.setup().to_json();
    let old_hotkey = engine.setup().overlay.hotkey.clone();
    shared
        .backups
        .create(&before, "before import")
        .map_err(|e| {
            format!("nothing was imported, the backup of the current configuration failed: {e}")
        })?;
    let result = match mode {
        ImportMode::ReplaceAll => {
            let setup = prepare_replace(engine.setup(), file.setup.clone());
            engine.replace_setup(setup);
            ImportResult {
                message: format!(
                    "Imported the whole configuration ({} profile{}).",
                    file.setup.profiles.len(),
                    if file.setup.profiles.len() == 1 {
                        ""
                    } else {
                        "s"
                    }
                ),
                report: None,
            }
        }
        ImportMode::AddProfiles { profiles } => {
            let report = engine
                .import_profiles(&file.setup, &profiles)
                .map_err(|e| e.to_string())?;
            let renamed = report
                .added
                .iter()
                .filter(|p| p.renamed_from.is_some())
                .count();
            let mut message = format!(
                "Added {} profile{}.",
                report.added.len(),
                if report.added.len() == 1 { "" } else { "s" }
            );
            if renamed > 0 {
                message.push_str(&format!(" {renamed} renamed because the name was taken."));
            }
            if !report.dropped_executables.is_empty() {
                message.push_str(&format!(
                    " Programs already used by another profile were not taken over: {}.",
                    report.dropped_executables.join(", ")
                ));
            }
            ImportResult {
                message,
                report: Some(report),
            }
        }
    };
    shared.commit(&mut engine);
    drop(engine);
    *shared
        .pending_import
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = None;
    apply_replaced(&app, &shared, &old_hotkey);
    Ok(result)
}

#[tauri::command]
pub fn list_backups(shared: State<'_, Arc<Shared>>) -> Vec<BackupInfo> {
    shared.backups.list()
}

/// "Back up now".
#[tauri::command]
pub fn backup_now(shared: State<'_, Arc<Shared>>) -> Res<BackupInfo> {
    let text = shared.engine().setup().to_json();
    shared.backups.create(&text, "manual")
}

/// Restores a backup. The current configuration is backed up first; a backup that cannot be
/// read leaves everything as it is.
#[tauri::command]
pub fn restore_backup(app: AppHandle, shared: State<'_, Arc<Shared>>, name: String) -> Res<String> {
    let text = shared.backups.read(&name)?;
    let restored =
        Setup::from_json(&text).map_err(|e| format!("{name} cannot be restored: {e}"))?;
    restored
        .connections
        .validate()
        .map_err(|e| format!("{name} cannot be restored: {e}"))?;
    let mut engine = shared.engine();
    let old_hotkey = engine.setup().overlay.hotkey.clone();
    shared
        .backups
        .create(&engine.setup().to_json(), "before restore")
        .map_err(|e| {
            format!("nothing was restored, the backup of the current configuration failed: {e}")
        })?;
    let setup = prepare_replace(engine.setup(), restored);
    engine.replace_setup(setup);
    shared.commit(&mut engine);
    drop(engine);
    apply_replaced(&app, &shared, &old_hotkey);
    shared.diagnostics.record(Record::new(
        Category::Log,
        format!("Backup restored: {name}"),
    ));
    Ok(format!("Restored {name}."))
}

#[tauri::command]
pub fn open_backup_folder(app: AppHandle, shared: State<'_, Arc<Shared>>) -> Res<()> {
    use tauri_plugin_opener::OpenerExt;
    let dir = shared.backups.dir();
    std::fs::create_dir_all(dir).map_err(|e| format!("cannot create the backup folder: {e}"))?;
    app.opener()
        .open_path(dir.display().to_string(), None::<&str>)
        .map_err(|e| format!("cannot open the backup folder: {e}"))
}

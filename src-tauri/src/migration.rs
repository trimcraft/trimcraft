//! Takes over a Detent installation (Trimcraft's former name) on the first start: copies the
//! config and log folders, moves the Credential Manager entries and replaces the autostart
//! entries. What to do is decided in `trimcraft_core::migration`; this module does it and logs
//! every step to the app log and the event log.
//!
//! The folders are copied before Tauri starts (the log plugin creates the new log folder, and
//! the setup must be in place before it loads), so their steps are collected in a [`Report`]
//! and logged once the logger runs. The secret references inside the setup are renamed when it
//! loads (setup schema 5).
//!
//! A Detent that still runs uses another identifier, so it neither blocks Trimcraft's
//! single-instance check nor receives its second launches; both would react to the same
//! inputs, though, so Trimcraft warns when it finds one.

use crate::autostart;
use crate::secrets;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use trimcraft_core::migration::{
    self, CredentialStep, FolderKind, FolderPlan, FolderState, NOTE_FILE, OLD_IDENTIFIER,
    OLD_SECRET_PREFIX, SECRET_PREFIX, SETUP_FILE,
};
use trimcraft_core::{Category, Diagnostics, Level, Record};

/// Steps taken before the logger was running.
#[derive(Default)]
pub struct Report(Vec<(Level, String)>);

impl Report {
    fn info(&mut self, text: String) {
        self.0.push((Level::Info, text));
    }

    fn warn(&mut self, text: String) {
        self.0.push((Level::Warn, text));
    }

    /// Writes the collected steps to the app log and the event log.
    pub fn log(self, diagnostics: &Diagnostics) {
        for (level, text) in self.0 {
            note(diagnostics, level, text);
        }
    }
}

/// Info goes to both logs here; warnings reach the event log through the log bridge.
fn note(diagnostics: &Diagnostics, level: Level, text: String) {
    if level == Level::Info {
        log::info!("{text}");
        diagnostics.record(Record::new(Category::Log, text));
    } else {
        log::warn!("{text}");
    }
}

// --- Folders ---------------------------------------------------------------------------------

/// Copies Detent's config and log folders, if there are any to take over.
pub fn folders(identifier: &str) -> Report {
    let mut report = Report::default();
    if old_instance_running() {
        report.warn(
            "migration: Detent is still running; quit it, or both react to the same inputs".into(),
        );
    }
    for (kind, var) in [
        (FolderKind::Config, "APPDATA"),
        (FolderKind::Local, "LOCALAPPDATA"),
    ] {
        let Some(base) = std::env::var_os(var).map(PathBuf::from) else {
            report.warn(format!(
                "migration: %{var}% is not set; {kind:?} folder not checked"
            ));
            continue;
        };
        folder(
            kind,
            &base.join(OLD_IDENTIFIER),
            &base.join(identifier),
            &mut report,
        );
    }
    report
}

fn folder(kind: FolderKind, old: &Path, new: &Path, report: &mut Report) {
    let state = FolderState {
        old_exists: old.is_dir(),
        old_files: if old.is_dir() {
            list_files(old).unwrap_or_else(|e| {
                report.warn(format!("migration: cannot list {}: {e}", old.display()));
                Vec::new()
            })
        } else {
            Vec::new()
        },
        new_has_setup: new.join(SETUP_FILE).is_file(),
    };
    let (files, skipped) = match migration::plan_folder(kind, &state) {
        FolderPlan::Skip(why) => {
            if state.old_exists {
                report.info(format!(
                    "migration: {} not taken over: {why}",
                    old.display()
                ));
            }
            return;
        }
        FolderPlan::Copy { files, skipped } => (files, skipped),
    };
    let mut failed = Vec::new();
    for file in &files {
        if let Err(e) = copy(old, new, file) {
            report.warn(format!(
                "migration: cannot copy {file} from {}: {e}",
                old.display()
            ));
            failed.push(file.as_str());
        }
    }
    report.info(format!(
        "migration: copied {} of {} files from {} to {}{}",
        files.len() - failed.len(),
        files.len(),
        old.display(),
        new.display(),
        if skipped.is_empty() {
            String::new()
        } else {
            format!(" (left out: {})", summary(&skipped))
        }
    ));
    // Without its setup the config folder is taken over again at the next start.
    if kind == FolderKind::Config && failed.contains(&SETUP_FILE) {
        return;
    }
    let note = migration::note_text(&new.display().to_string());
    match fs::write(old.join(NOTE_FILE), note) {
        Ok(()) => report.info(format!("migration: left {NOTE_FILE} in {}", old.display())),
        Err(e) => report.warn(format!(
            "migration: cannot write {NOTE_FILE} into {}: {e}",
            old.display()
        )),
    }
}

fn summary(files: &[String]) -> String {
    // The WebView2 profile has thousands of files; name the folders.
    let mut tops: Vec<&str> = files
        .iter()
        .map(|f| f.split('/').next().unwrap_or(f))
        .collect();
    tops.dedup();
    tops.join(", ")
}

fn copy(old: &Path, new: &Path, file: &str) -> io::Result<()> {
    let to = new.join(file);
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::copy(old.join(file), to).map(|_| ())
}

/// Files below `dir`, relative, with `/` as separator.
fn list_files(dir: &Path) -> io::Result<Vec<String>> {
    let mut out = Vec::new();
    let mut pending = vec![PathBuf::new()];
    while let Some(rel) = pending.pop() {
        for entry in fs::read_dir(dir.join(&rel))? {
            let entry = entry?;
            let path = rel.join(entry.file_name());
            if entry.file_type()?.is_dir() {
                pending.push(path);
            } else {
                let parts: Vec<String> = path
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy().into_owned())
                    .collect();
                out.push(parts.join("/"));
            }
        }
    }
    Ok(out)
}

/// Detent's single-instance mutex exists (access denied: it runs elevated).
fn old_instance_running() -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, ERROR_ACCESS_DENIED, GetLastError};
    use windows_sys::Win32::Storage::FileSystem::SYNCHRONIZE;
    use windows_sys::Win32::System::Threading::OpenMutexW;
    let name = crate::elevation::wide(format!("{OLD_IDENTIFIER}-sim"));
    // SAFETY: NUL-terminated name; the handle is closed right away.
    let mutex = unsafe { OpenMutexW(SYNCHRONIZE, 0, name.as_ptr()) };
    if !mutex.is_null() {
        // SAFETY: opened above.
        unsafe { CloseHandle(mutex) };
        return true;
    }
    // SAFETY: no preconditions.
    unsafe { GetLastError() == ERROR_ACCESS_DENIED }
}

// --- Credentials -----------------------------------------------------------------------------

/// Moves Detent's Credential Manager entries (`detent/...`) to `trimcraft/...`. Runs before the
/// connectors start, so they find their secrets under the new names.
pub fn credentials(diagnostics: &Diagnostics) {
    let listed = secrets::list(OLD_SECRET_PREFIX).and_then(|old| {
        secrets::list(SECRET_PREFIX).map(|new| migration::plan_credentials(&old, &new))
    });
    let steps = match listed {
        Ok(steps) => steps,
        Err(e) => return note(diagnostics, Level::Warn, format!("migration: {e}")),
    };
    for step in steps {
        let (level, text) = match credential(&step) {
            Ok(done) => (
                Level::Info,
                format!("migration: credential {}: {done}", step.old()),
            ),
            Err(e) => (
                Level::Warn,
                format!("migration: credential {} kept: {e}", step.old()),
            ),
        };
        note(diagnostics, level, text);
    }
}

fn credential(step: &CredentialStep) -> Result<String, String> {
    let (old, new) = match step {
        CredentialStep::Copy { old, new } | CredentialStep::Reconcile { old, new } => (old, new),
    };
    let value = secrets::get_raw(old)?.ok_or("it disappeared")?;
    if let CredentialStep::Reconcile { .. } = step {
        if secrets::get_raw(new)?.as_deref() != Some(value.as_slice()) {
            return Err(format!("{new} exists with another value; both are kept"));
        }
        secrets::delete(old)?;
        return Ok(format!("{new} exists with the same value; removed"));
    }
    secrets::set_raw(new, &value)?;
    if secrets::get_raw(new)?.as_deref() != Some(value.as_slice()) {
        return Err(format!("{new} did not read back the same"));
    }
    secrets::delete(old)?;
    Ok(format!("moved to {new}"))
}

// --- Autostart -------------------------------------------------------------------------------

/// Removes Detent's autostart; [`autostart::sync`] registers Trimcraft's own from the settings.
/// Calls schtasks, so not on the main thread.
pub fn autostart(elevated: bool, diagnostics: &Diagnostics) {
    let (run_key, task) = autostart::detent_present();
    let plan = migration::plan_autostart(run_key, task, elevated);
    for step in plan.steps {
        let (level, text) = match autostart::remove_detent(step) {
            Ok(()) => (
                Level::Info,
                format!("migration: Detent autostart removed ({step:?})"),
            ),
            Err(e) => (
                Level::Warn,
                format!("migration: cannot remove Detent autostart ({step:?}): {e}"),
            ),
        };
        note(diagnostics, level, text);
    }
    if let Some(text) = plan.note {
        note(diagnostics, Level::Warn, format!("migration: {text}"));
    }
}

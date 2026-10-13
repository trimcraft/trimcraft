//! Updates (Settings, Updates): checks the signed release feed of the latest GitHub release
//! (`plugins.updater` in tauri.conf.json) and installs a new version with the per-user NSIS
//! installer in passive mode, which quits Trimcraft and starts the new version.
//!
//! The state goes into the engine, so the Settings page and the header badge show it (they read
//! it from the snapshot, whenever they are shown). No release yet (the feed is missing, 404)
//! counts as "up to date". Every outcome goes to the event log; nothing here may take the app
//! down.

use crate::Shared;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use tauri::AppHandle;
use tauri_plugin_updater::{Error, Updater, UpdaterBuilder, UpdaterExt};
use trimcraft_core::diagnostics::{Category, Level, Record};
use trimcraft_core::updates::{
    CheckOrigin, UpdateBusy, UpdateState, announcement, automatic_check_due, check_log,
    notes_excerpt, percent,
};

/// The startup check waits this long, so it never competes with loading.
const STARTUP_DELAY: std::time::Duration = std::time::Duration::from_secs(10);
/// The automatic check repeats this often while nothing was found.
const RECHECK_INTERVAL: std::time::Duration = std::time::Duration::from_secs(6 * 60 * 60);
/// A check that takes longer fails instead of staying at "Checking…" (the download that
/// installs an update has no limit).
const CHECK_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

fn set_state(shared: &Shared, state: UpdateState) {
    let mut engine = shared.engine();
    if engine.set_update_state(state) {
        shared.changed.store(true, Ordering::Relaxed);
    }
}

fn log(shared: &Shared, level: Level, text: String) {
    match level {
        Level::Info => log::info!("updates: {text}"),
        _ => log::warn!("updates: {text}"),
    }
    shared
        .diagnostics
        .record(Record::new(Category::Log, text).level(level));
}

/// What a running update would interrupt.
pub fn busy(shared: &Shared) -> UpdateBusy {
    let engine = shared.engine();
    UpdateBusy {
        macros: shared.player.running(),
        vjoy: engine.connections().vjoy.enabled && !engine.vjoy_state().devices.is_empty(),
    }
}

/// Developer override of the release feed: the URL of a `latest.json` to check instead, e.g.
/// a local file server, to test the check without publishing a release.
pub const ENDPOINT_VAR: &str = "TRIMCRAFT_UPDATE_ENDPOINT";

/// The release feed from [`ENDPOINT_VAR`]. Only in debug builds and builds with the
/// `update-endpoint-override` feature; published releases always use tauri.conf.json.
#[cfg(any(debug_assertions, feature = "update-endpoint-override"))]
fn endpoint_override() -> Option<tauri::Url> {
    let value = std::env::var(ENDPOINT_VAR).ok()?;
    match value.parse() {
        Ok(url) => Some(url),
        Err(e) => {
            log::warn!("updates: ignoring {ENDPOINT_VAR}={value:?}: {e}");
            None
        }
    }
}

/// With [`ENDPOINT_VAR`] set, the automatic check repeats after this many seconds instead of
/// [`RECHECK_INTERVAL`] (same builds only).
pub const RECHECK_VAR: &str = "TRIMCRAFT_UPDATE_RECHECK_SECS";

#[cfg(any(debug_assertions, feature = "update-endpoint-override"))]
fn recheck_interval() -> std::time::Duration {
    std::env::var(RECHECK_VAR)
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|_| endpoint_override().is_some())
        .map_or(RECHECK_INTERVAL, std::time::Duration::from_secs)
}

#[cfg(not(any(debug_assertions, feature = "update-endpoint-override")))]
fn endpoint_override() -> Option<tauri::Url> {
    None
}

#[cfg(not(any(debug_assertions, feature = "update-endpoint-override")))]
fn recheck_interval() -> std::time::Duration {
    RECHECK_INTERVAL
}

/// The updater for the configured feed (or the override).
fn updater(builder: UpdaterBuilder) -> Result<Updater, Error> {
    let builder = match endpoint_override() {
        Some(url) => {
            log::warn!("updates: checking {url} ({ENDPOINT_VAR})");
            builder.endpoints(vec![url])?
        }
        None => builder,
    };
    builder.timeout(CHECK_TIMEOUT).build()
}

/// Checks for a new release. Returns the new state (also stored in the engine) and logs the
/// outcome.
pub async fn check(app: &AppHandle, shared: &Arc<Shared>, origin: CheckOrigin) -> UpdateState {
    set_state(shared, UpdateState::Checking);
    let result = match updater(app.updater_builder()) {
        Ok(updater) => updater.check().await,
        Err(e) => Err(e),
    };
    let state = match result {
        Ok(Some(update)) => UpdateState::Available {
            version: update.version.clone(),
            notes: notes_excerpt(update.body.as_deref().unwrap_or("")),
            date: update.date.map(|d| d.to_string()),
        },
        Ok(None) => UpdateState::UpToDate { no_releases: false },
        // The feed is not there (no release published yet): nothing to update to.
        Err(Error::ReleaseNotFound) => UpdateState::UpToDate { no_releases: true },
        Err(e) => UpdateState::Failed {
            message: format!("Could not check for updates: {e}"),
        },
    };
    if let Some((level, text)) = check_log(origin, &app.package_info().version.to_string(), &state)
    {
        log(shared, level, text);
    }
    set_state(shared, state.clone());
    state
}

/// The automatic check, while the setting is on: shortly after the start, then every
/// [`RECHECK_INTERVAL`] until an update is found (Trimcraft often runs in the tray for days, and
/// a release published after the start is found too). On a thread of its own.
pub fn check_automatically(app: &AppHandle, shared: &Arc<Shared>) {
    if !shared.engine().setup().settings.check_updates_on_startup {
        log::info!("updates: the automatic check is off");
    }
    let (app, shared) = (app.clone(), Arc::clone(shared));
    let spawned = std::thread::Builder::new()
        .name("trimcraft-updates".into())
        .spawn(move || {
            std::thread::sleep(STARTUP_DELAY);
            let mut origin = CheckOrigin::Startup;
            let mut announced: Option<String> = None;
            loop {
                let due = {
                    let engine = shared.engine();
                    let enabled = engine.setup().settings.check_updates_on_startup;
                    automatic_check_due(enabled, engine.update_state())
                };
                if due {
                    let state = tauri::async_runtime::block_on(check(&app, &shared, origin));
                    if let Some(version) = announcement(origin, &state, announced.as_deref()) {
                        announce(&app, version);
                        announced = Some(version.to_owned());
                    }
                }
                origin = CheckOrigin::Scheduled;
                std::thread::sleep(recheck_interval());
            }
        });
    if let Err(e) = spawned {
        log::error!("could not start the update check: {e}");
    }
}

/// A notification about a found update, while the main window is hidden (in the tray); with the
/// window open the header badge shows it.
fn announce(app: &AppHandle, version: &str) {
    use tauri::Manager;
    use tauri_plugin_notification::NotificationExt;
    let visible = app
        .get_webview_window("main")
        .and_then(|w| w.is_visible().ok())
        .unwrap_or(false);
    if visible {
        return;
    }
    let shown = app
        .notification()
        .builder()
        .title(format!("Trimcraft {version} is available"))
        .body("Install it in Settings, Updates.")
        .show();
    if let Err(e) = shown {
        log::warn!("updates: could not show the notification: {e}");
    }
}

/// Downloads and installs the available update. On Windows the installer quits Trimcraft (its
/// macros stop and vJoy is released first) and starts the new version.
pub async fn install(app: &AppHandle, shared: &Arc<Shared>) -> Result<(), String> {
    let before_exit = Arc::clone(shared);
    let updater = updater(app.updater_builder().on_before_exit(move || {
        log::info!("updates: quitting for the installer");
        before_exit.player.shutdown();
        before_exit.vjoy.shutdown();
    }))
    .map_err(|e| e.to_string())?;
    let update = match updater.check().await {
        Ok(Some(update)) => update,
        Ok(None) | Err(Error::ReleaseNotFound) => {
            set_state(shared, UpdateState::UpToDate { no_releases: false });
            return Err("there is no newer version any more".into());
        }
        Err(e) => {
            let message = format!("Could not check for updates: {e}");
            log(shared, Level::Warn, message.clone());
            set_state(
                shared,
                UpdateState::Failed {
                    message: message.clone(),
                },
            );
            return Err(message);
        }
    };
    let version = update.version.clone();
    log(
        shared,
        Level::Info,
        format!("Downloading Trimcraft {version}"),
    );
    set_state(
        shared,
        UpdateState::Downloading {
            version: version.clone(),
            percent: None,
        },
    );
    let mut done = 0u64;
    let progress = Arc::clone(shared);
    let progress_version = version.clone();
    let installing = Arc::clone(shared);
    let installing_version = version.clone();
    let result = update
        .download_and_install(
            move |chunk, total| {
                done += chunk as u64;
                set_state(
                    &progress,
                    UpdateState::Downloading {
                        version: progress_version.clone(),
                        percent: percent(done, total),
                    },
                );
            },
            move || {
                set_state(
                    &installing,
                    UpdateState::Installing {
                        version: installing_version,
                    },
                );
            },
        )
        .await;
    match result {
        Ok(()) => {
            log(
                shared,
                Level::Info,
                format!("Installing Trimcraft {version}"),
            );
            // Windows quits for the installer before getting here; elsewhere restart.
            app.restart();
        }
        Err(e) => {
            let message = format!("Updating to {version} failed: {e}");
            log(shared, Level::Error, message.clone());
            set_state(
                shared,
                UpdateState::Failed {
                    message: message.clone(),
                },
            );
            Err(message)
        }
    }
}

#[cfg(test)]
mod tests {
    /// The version in Cargo.toml's `[workspace.package]` section.
    fn cargo_version(toml: &str) -> Option<String> {
        let section = toml
            .split("\n[")
            .find(|s| s.starts_with("workspace.package]"))?;
        section.lines().find_map(|l| {
            let v = l.strip_prefix("version")?.trim_start().strip_prefix('=')?;
            Some(v.trim().trim_matches('"').to_owned())
        })
    }

    /// The one version all files agree on, or the differences (scripts/check-versions.mjs
    /// does the same for the release workflow, also against the tag).
    fn check_versions(
        cargo: &str,
        package: &str,
        lock: &str,
        tauri: &str,
    ) -> Result<String, String> {
        let json = |text: &str| -> Option<String> {
            serde_json::from_str::<serde_json::Value>(text).ok()?["version"]
                .as_str()
                .map(str::to_owned)
        };
        let all = [
            ("Cargo.toml", cargo_version(cargo)),
            ("package.json", json(package)),
            ("package-lock.json", json(lock)),
            ("src-tauri/tauri.conf.json", json(tauri)),
        ];
        let first = all[0].1.clone();
        if first.is_some() && all.iter().all(|(_, v)| *v == first) {
            return Ok(first.unwrap_or_default());
        }
        Err(all
            .iter()
            .map(|(f, v)| format!("{f} {}", v.as_deref().unwrap_or("(none)")))
            .collect::<Vec<_>>()
            .join(", "))
    }

    /// CI fails when a version is bumped in one place only.
    #[test]
    fn versions_are_in_sync() {
        let version = check_versions(
            include_str!("../../Cargo.toml"),
            include_str!("../../package.json"),
            include_str!("../../package-lock.json"),
            include_str!("../tauri.conf.json"),
        )
        .unwrap_or_else(|e| panic!("versions differ: {e}"));
        assert_eq!(version, env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn version_check_finds_differences() {
        let cargo = "[workspace]\nmembers = []\n\n[workspace.package]\nversion = \"0.2.0\"\n";
        let v = |v: &str| format!(r#"{{"name":"x","version":"{v}"}}"#);
        assert_eq!(
            check_versions(cargo, &v("0.2.0"), &v("0.2.0"), &v("0.2.0")),
            Ok("0.2.0".into())
        );
        let err = check_versions(cargo, &v("0.2.0"), &v("0.1.0"), &v("0.2.0")).unwrap_err();
        assert!(err.contains("package-lock.json 0.1.0"), "{err}");
        let err = check_versions(cargo, &v("0.2.0"), &v("0.2.0"), "{}").unwrap_err();
        assert!(err.contains("tauri.conf.json (none)"), "{err}");
        assert!(check_versions("[workspace]\n", &v("0.2.0"), &v("0.2.0"), &v("0.2.0")).is_err());
    }

    /// The feed override is for local tests; published builds never honour it.
    #[test]
    fn released_builds_use_the_configured_feed() {
        for workflow in [
            include_str!("../../.github/workflows/release.yml"),
            include_str!("../../.github/workflows/ci.yml"),
        ] {
            assert!(!workflow.contains("update-endpoint-override"));
            assert!(!workflow.contains(super::ENDPOINT_VAR));
            assert!(!workflow.contains("dangerousInsecureTransportProtocol"));
        }
        let conf = include_str!("../tauri.conf.json");
        assert!(!conf.contains("dangerousInsecureTransportProtocol"));
    }

    /// The updater feed and key ship with the app (tauri.conf.json, plugins.updater).
    #[test]
    fn updater_is_configured() {
        let conf: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        let updater = &conf["plugins"]["updater"];
        assert_eq!(
            updater["endpoints"][0],
            "https://github.com/trimcraft/trimcraft/releases/latest/download/latest.json"
        );
        let pubkey = updater["pubkey"].as_str().unwrap();
        // A minisign public key, base64 of "untrusted comment: minisign public key: …".
        assert!(pubkey.starts_with("dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk"));
        assert_eq!(updater["windows"]["installMode"], "passive");
        // Per-user install, like the first install.
        assert_eq!(
            conf["bundle"]["windows"]["nsis"]["installMode"],
            "currentUser"
        );
        // Signed updater artifacts only in the release build (the CI build has no key).
        assert!(conf["bundle"].get("createUpdaterArtifacts").is_none());
        let release: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.release.conf.json")).unwrap();
        assert_eq!(release["bundle"]["createUpdaterArtifacts"], true);
    }
}

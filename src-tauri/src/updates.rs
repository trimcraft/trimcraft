//! Updates (Settings, Updates): checks the signed release feed of the latest GitHub release
//! (`plugins.updater` in tauri.conf.json) and installs a new version with the per-user NSIS
//! installer in passive mode, which quits Trimcraft and starts the new version.
//!
//! The state goes into the engine, so the Settings page and the header badge show it. No
//! release yet (the feed is missing, 404) counts as "up to date". Every failure goes to the
//! event log; nothing here may take the app down.

use crate::Shared;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use tauri::AppHandle;
use tauri_plugin_updater::{Error, UpdaterExt};
use trimcraft_core::diagnostics::{Category, Level, Record};
use trimcraft_core::updates::{UpdateBusy, UpdateState, notes_excerpt, percent};

/// The startup check waits this long, so it never competes with loading.
const STARTUP_DELAY: std::time::Duration = std::time::Duration::from_secs(10);

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

/// Checks for a new release. Returns the new state (also stored in the engine).
pub async fn check(app: &AppHandle, shared: &Arc<Shared>) -> UpdateState {
    set_state(shared, UpdateState::Checking);
    let result = match app.updater() {
        Ok(updater) => updater.check().await,
        Err(e) => Err(e),
    };
    let state = match result {
        Ok(Some(update)) => {
            log(
                shared,
                Level::Info,
                format!("Trimcraft {} is available", update.version),
            );
            UpdateState::Available {
                version: update.version.clone(),
                notes: notes_excerpt(update.body.as_deref().unwrap_or("")),
                date: update.date.map(|d| d.to_string()),
            }
        }
        Ok(None) => UpdateState::UpToDate { no_releases: false },
        // The feed is not there (no release published yet): nothing to update to.
        Err(Error::ReleaseNotFound) => UpdateState::UpToDate { no_releases: true },
        Err(e) => {
            let message = format!("Could not check for updates: {e}");
            log(shared, Level::Warn, message.clone());
            UpdateState::Failed { message }
        }
    };
    set_state(shared, state.clone());
    state
}

/// The startup check, if the setting is on, in the background.
pub fn check_on_startup(app: &AppHandle, shared: &Arc<Shared>) {
    if !shared.engine().setup().settings.check_updates_on_startup {
        return;
    }
    let (app, shared) = (app.clone(), Arc::clone(shared));
    tauri::async_runtime::spawn(async move {
        tokio_sleep(STARTUP_DELAY).await;
        check(&app, &shared).await;
    });
}

async fn tokio_sleep(d: std::time::Duration) {
    // The async runtime is Tokio; a blocking sleep on its blocking pool keeps this free of a
    // direct Tokio dependency.
    let _ = tauri::async_runtime::spawn_blocking(move || std::thread::sleep(d)).await;
}

/// Downloads and installs the available update. On Windows the installer quits Trimcraft (its
/// macros stop and vJoy is released first) and starts the new version.
pub async fn install(app: &AppHandle, shared: &Arc<Shared>) -> Result<(), String> {
    let before_exit = Arc::clone(shared);
    let updater = app
        .updater_builder()
        .on_before_exit(move || {
            log::info!("updates: quitting for the installer");
            before_exit.player.shutdown();
            before_exit.vjoy.shutdown();
        })
        .build()
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

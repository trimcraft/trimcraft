//! Detent desktop app: wires the engine to the input backend, the windows and the disk.

mod commands;
mod discord;
mod executor;
mod overlay;
mod recorder;
mod secrets;
mod soundpad;
mod storage;

use detent_core::{DeviceRegistry, Engine, Setup};
use discord::Discord;
use executor::Executor;
use serde::Serialize;
use soundpad::Soundpad;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};
use storage::Storage;
use tauri::{AppHandle, Emitter, Manager};

const DEVICES_FILE: &str = "devices.json";
const SETUP_FILE: &str = "setup.json";
const VALUES_FILE: &str = "values.json";
/// Active state per virtual control.
const CONTROL_STATES_FILE: &str = "control-states.json";

/// UI refresh: at most this often. Input itself is processed as it arrives.
const FRAME: Duration = Duration::from_millis(16);
/// Function values (counters) and active states are saved at most this often while they change.
const SAVE_VALUES_EVERY: Duration = Duration::from_secs(1);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Meta {
    version: String,
    backend: String,
    data_dir: String,
    log_dir: String,
    platform: String,
}

pub struct Shared {
    engine: Mutex<Engine>,
    changed: AtomicBool,
    storage: Storage,
    executor: Executor,
    soundpad: Arc<Soundpad>,
    discord: Arc<Discord>,
    recorder: recorder::Recorder,
    meta: Meta,
}

impl Shared {
    pub fn engine(&self) -> MutexGuard<'_, Engine> {
        // A panic while holding the lock leaves the engine usable; keep going.
        self.engine.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Runs queued actions, saves whatever the engine marked as changed and asks the UI to
    /// redraw.
    pub fn commit(&self, engine: &mut Engine) {
        self.dispatch(engine);
        self.save(engine);
        self.changed.store(true, Ordering::Relaxed);
    }

    /// Hands the actions the engine emitted to the executor thread, in order. Actions whose
    /// connection is disabled stay stored but are skipped with a warning.
    fn dispatch(&self, engine: &mut Engine) {
        let actions: Vec<_> = engine
            .take_actions()
            .into_iter()
            .filter(|request| {
                let enabled = engine.action_enabled(&request.action);
                if !enabled {
                    log::warn!(
                        "action skipped, its connection is disabled: {} / {}: {}",
                        request.control,
                        request.state,
                        request.action.describe()
                    );
                }
                enabled
            })
            .collect();
        if !actions.is_empty() {
            self.executor.submit(actions);
        }
    }

    fn save(&self, engine: &mut Engine) {
        let dirty = engine.take_dirty();
        if dirty.registry {
            self.storage.save_json(DEVICES_FILE, engine.registry());
        }
        if dirty.setup {
            self.storage
                .save_text(SETUP_FILE, &engine.setup().to_json());
        }
        if dirty.values {
            self.storage.save_json(VALUES_FILE, &engine.values());
        }
        if dirty.controls {
            self.storage
                .save_json(CONTROL_STATES_FILE, &engine.active_states());
        }
    }
}

pub fn run() {
    tauri::Builder::default()
        .plugin(
            tauri_plugin_log::Builder::new()
                .level(log::LevelFilter::Info)
                .targets([
                    tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::Stdout),
                    tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::LogDir {
                        file_name: Some("detent".into()),
                    }),
                ])
                .build(),
        )
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _shortcut, event| {
                    if event.state == tauri_plugin_global_shortcut::ShortcutState::Pressed {
                        overlay::toggle(app);
                    }
                })
                .build(),
        )
        .setup(|app| {
            let handle = app.handle().clone();
            let backend = detent_input::default_backend();
            let shared = Arc::new(load(&handle, backend.name())?);
            app.manage(shared.clone());
            report_connectors(&shared);
            overlay::create(&handle, &shared)?;
            start_input(handle, shared, backend);
            Ok(())
        })
        .on_window_event(|window, event| {
            // The overlay window would keep the app alive; closing the main window quits.
            if window.label() == "main"
                && matches!(event, tauri::WindowEvent::CloseRequested { .. })
            {
                window.app_handle().exit(0);
            }
        })
        .invoke_handler(commands::handler())
        .run(tauri::generate_context!())
        .expect("error while running Detent");
}

fn load(app: &AppHandle, backend: &str) -> Result<Shared, Box<dyn std::error::Error>> {
    let storage = Storage::new(app.path().app_config_dir()?)?;
    let registry: DeviceRegistry = storage.load_json(DEVICES_FILE).unwrap_or_default();
    let setup = storage
        .load(SETUP_FILE, |t| {
            Setup::from_json(t).map_err(|e| e.to_string())
        })
        .unwrap_or_default();
    let values: BTreeMap<String, f64> = storage.load_json(VALUES_FILE).unwrap_or_default();
    let control_states: BTreeMap<String, detent_core::RadioMemory> =
        storage.load_json(CONTROL_STATES_FILE).unwrap_or_default();
    let meta = Meta {
        version: app.package_info().version.to_string(),
        backend: backend.into(),
        data_dir: storage.dir.display().to_string(),
        log_dir: app
            .path()
            .app_log_dir()
            .map(|p| p.display().to_string())
            .unwrap_or_default(),
        platform: std::env::consts::OS.into(),
    };
    log::info!(
        "Detent {} on {}, input backend {}",
        meta.version,
        meta.platform,
        meta.backend
    );
    let mut engine = Engine::new(registry, setup, &values);
    engine.restore_active_states(&control_states);
    let soundpad = Arc::new(Soundpad::spawn(engine.connections().soundpad.clone()));
    let discord = Arc::new(Discord::spawn(engine.connections().discord.clone()));
    Ok(Shared {
        engine: Mutex::new(engine),
        changed: AtomicBool::new(true),
        storage,
        executor: Executor::spawn(app.clone(), soundpad.clone(), discord.clone()),
        soundpad,
        discord,
        recorder: recorder::Recorder::default(),
        meta,
    })
}

/// Input thread: applies every backend event to the engine as it arrives, emits snapshots to
/// the windows at most once per frame, and saves changes.
fn start_input(app: AppHandle, shared: Arc<Shared>, backend: Box<dyn detent_input::InputBackend>) {
    std::thread::Builder::new()
        .name("detent-engine".into())
        .spawn(move || {
            let backend = detent_input::spawn(backend);
            let mut backend_alive = true;
            let mut last_emit = Instant::now();
            let mut last_values_save = Instant::now();
            loop {
                if !backend_alive {
                    std::thread::sleep(FRAME);
                } else {
                    match backend.events.recv_timeout(FRAME) {
                        Ok(event) => {
                            let mut engine = shared.engine();
                            if engine.apply(event) {
                                shared.changed.store(true, Ordering::Relaxed);
                            }
                            shared.dispatch(&mut engine);
                            // Drain whatever else is queued before taking the next frame decision.
                            while let Ok(event) = backend.events.try_recv() {
                                if engine.apply(event) {
                                    shared.changed.store(true, Ordering::Relaxed);
                                }
                                shared.dispatch(&mut engine);
                            }
                        }
                        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                            // Keep serving the UI; the error is in the snapshot.
                            log::error!("input backend stopped");
                            backend_alive = false;
                        }
                    }
                }

                if last_values_save.elapsed() >= SAVE_VALUES_EVERY {
                    last_values_save = Instant::now();
                    let mut engine = shared.engine();
                    shared.save(&mut engine);
                }

                if last_emit.elapsed() >= FRAME && shared.changed.swap(false, Ordering::Relaxed) {
                    last_emit = Instant::now();
                    let snapshot = shared.engine().snapshot();
                    if let Err(e) = app.emit("detent://snapshot", &snapshot) {
                        log::warn!("could not send snapshot: {e}");
                    }
                }
                overlay::keep_on_top(&app);
            }
        })
        .expect("spawn engine thread");
}

/// Connector status changes go into the engine, so both windows see them.
fn report_connectors(shared: &Arc<Shared>) {
    use detent_core::ConnectorId;
    let reporter = |id: ConnectorId| {
        let weak = Arc::downgrade(shared);
        move |state| {
            let Some(shared) = weak.upgrade() else { return };
            let mut engine = shared.engine();
            if engine.set_connector_state(id, state) {
                shared.changed.store(true, Ordering::Relaxed);
            }
        }
    };
    shared.soundpad.on_status(reporter(ConnectorId::Soundpad));
    shared.discord.on_status(reporter(ConnectorId::Discord));
}

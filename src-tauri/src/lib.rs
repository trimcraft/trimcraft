//! Detent desktop app: wires the engine to the input backend, the windows and the disk.

mod autostart;
mod commands;
mod discord;
mod elevation;
mod executor;
mod foreground;
mod overlay;
mod recorder;
mod secrets;
mod soundpad;
mod storage;
mod tray;

use detent_core::settings::{self, AUTOSTART_ARG};
use detent_core::{DeviceRegistry, Engine, Setup, ShellStatus};
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
    /// Detent runs as administrator.
    elevated: bool,
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
    let context = tauri::generate_context!();
    let identifier = context.config().identifier.clone();
    let autostarted = std::env::args().any(|a| a == AUTOSTART_ARG);
    let show = std::env::args().any(|a| a == elevation::SHOW_ARG);
    let relaunched_from = elevation::relaunched_from();
    if let Some(pid) = relaunched_from {
        // The non-elevated instance that started this one must be gone first, or the
        // single-instance check below would end this one.
        elevation::wait_for_exit(pid, Duration::from_secs(10));
    }
    if elevation::forward_to_elevated_instance(&identifier) {
        return;
    }
    tauri::Builder::default()
        // First, so a second launch ends before anything else starts.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            log::info!("second launch; showing the window");
            tray::show_main(app);
        }))
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
        // Used from Rust only (open_discord_portal); the windows get no opener permission.
        .plugin(tauri_plugin_opener::init())
        // Used from Rust only (tray notice); the windows get no notification permission.
        .plugin(tauri_plugin_notification::init())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _shortcut, event| {
                    if event.state == tauri_plugin_global_shortcut::ShortcutState::Pressed {
                        overlay::toggle(app);
                    }
                })
                .build(),
        )
        .setup(move |app| {
            let handle = app.handle().clone();
            let storage = Storage::new(handle.path().app_config_dir()?)?;
            let (setup, first_run) = load_setup(&storage);
            let elevated = elevation::is_elevated();
            if settings::relaunch_elevated(&setup.settings, elevated, relaunched_from.is_some()) {
                match elevation::relaunch(false) {
                    // The elevated instance waits for this one to exit.
                    Ok(()) => std::process::exit(0),
                    Err(e) => log::warn!("{e}; continuing without administrator rights"),
                }
            }
            if elevated {
                elevation::allow_forwarding(&identifier);
            }
            let hidden = !show && settings::start_hidden(&setup.settings, autostarted, first_run);
            let backend = detent_input::default_backend();
            let shared = Arc::new(load(&handle, backend.name(), storage, setup, elevated)?);
            app.manage(shared.clone());
            report_connectors(&shared);
            sync_autostart(&shared);
            tray::create(&handle, shared.engine().setup().overlay.visible)?;
            overlay::create(&handle, &shared)?;
            if !elevated {
                foreground::watch(shared.clone());
            }
            if let Some(main) = handle.get_webview_window("main") {
                if hidden {
                    log::info!("started in the tray");
                } else {
                    main.show()?;
                }
            }
            start_input(handle, shared, backend);
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() == "main"
                && let tauri::WindowEvent::CloseRequested { api, .. } = event
            {
                close_main(window, api);
            }
        })
        .invoke_handler(commands::handler())
        .run(context)
        .expect("error while running Detent");
}

/// The main window's close button: hides it to the tray (input, actions and overlay keep
/// running) or quits, as set. The overlay window would keep the app alive, so quitting is
/// explicit.
fn close_main(window: &tauri::Window, api: &tauri::CloseRequestApi) {
    let app = window.app_handle();
    let shared = app.state::<Arc<Shared>>();
    let mut engine = shared.engine();
    if !engine.setup().settings.close_to_tray {
        drop(engine);
        return app.exit(0);
    }
    api.prevent_close();
    if let Err(e) = window.hide() {
        log::warn!("could not hide the main window: {e}");
    }
    if !engine.setup().settings.tray_notice_shown {
        engine.mark_tray_notice_shown();
        shared.commit(&mut engine);
        drop(engine);
        use tauri_plugin_notification::NotificationExt;
        let shown = app
            .notification()
            .builder()
            .title("Detent keeps running in the tray")
            .body("Inputs, actions and the overlay stay active. Quit from the tray icon's menu.")
            .show();
        if let Err(e) = shown {
            log::warn!("could not show the tray notification: {e}");
        }
    }
}

/// Registers or removes autostart to match the settings, in the background (schtasks takes a
/// moment), and reports the result to the Settings page.
pub fn sync_autostart(shared: &Arc<Shared>) {
    let shared = Arc::clone(shared);
    let spawned = std::thread::Builder::new()
        .name("detent-autostart".into())
        .spawn(move || {
            let settings = shared.engine().setup().settings.clone();
            let (autostart, note) = autostart::sync(&settings, shared.meta.elevated);
            let mut engine = shared.engine();
            let status = ShellStatus {
                autostart,
                autostart_note: note,
                ..engine.shell_status().clone()
            };
            if engine.set_shell_status(status) {
                shared.changed.store(true, Ordering::Relaxed);
            }
        });
    if let Err(e) = spawned {
        log::error!("could not start the autostart update: {e}");
    }
}

/// The saved setup, and whether there was none yet (first run).
fn load_setup(storage: &Storage) -> (Setup, bool) {
    let first_run = !storage.path(SETUP_FILE).exists();
    let setup = storage
        .load(SETUP_FILE, |t| {
            Setup::from_json(t).map_err(|e| e.to_string())
        })
        .unwrap_or_default();
    (setup, first_run)
}

fn load(
    app: &AppHandle,
    backend: &str,
    storage: Storage,
    setup: Setup,
    elevated: bool,
) -> Result<Shared, Box<dyn std::error::Error>> {
    let registry: DeviceRegistry = storage.load_json(DEVICES_FILE).unwrap_or_default();
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
        elevated,
    };
    log::info!(
        "Detent {} on {}, input backend {}, administrator: {}",
        meta.version,
        meta.platform,
        meta.backend,
        if meta.elevated { "yes" } else { "no" }
    );
    let mut engine = Engine::new(registry, setup, &values);
    engine.restore_active_states(&control_states);
    engine.set_shell_status(ShellStatus {
        elevated,
        ..ShellStatus::default()
    });
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

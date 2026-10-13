//! Trimcraft desktop app: wires the engine to the input backend, the windows and the disk.

mod autostart;
mod backups;
mod commands;
mod discord;
mod elevation;
mod executor;
mod foreground;
mod input;
mod launch;
mod migration;
mod overlay;
mod player;
mod profile_watch;
mod recorder;
mod secrets;
mod sound;
mod soundpad;
mod speech;
mod storage;
mod toasts;
mod transfer;
mod tray;
mod updates;
mod vjoy;
mod voicemeeter;
mod volume;
mod webhook;

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
use trimcraft_core::diagnostics::action_record;
use trimcraft_core::settings::{self, AUTOSTART_ARG};
use trimcraft_core::{
    ActionEvent, Category, DeviceRegistry, Diagnostics, Engine, Level, Record, Setup, ShellStatus,
};

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
    /// Trimcraft runs as administrator.
    elevated: bool,
}

pub struct Shared {
    engine: Mutex<Engine>,
    changed: AtomicBool,
    storage: Storage,
    executor: Executor,
    diagnostics: Diagnostics,
    soundpad: Arc<Soundpad>,
    discord: Arc<Discord>,
    vjoy: vjoy::Vjoy,
    voicemeeter: Arc<voicemeeter::Voicemeeter>,
    speech: Arc<speech::Speech>,
    sound: Arc<sound::Sound>,
    recorder: recorder::Recorder,
    /// Plays input macros; stops them on signals from the engine and on panic.
    player: player::Player,
    meta: Meta,
    /// The profile last reported in the log.
    logged_profile: Mutex<String>,
    /// The vJoy output last handed to the vJoy thread.
    vjoy_sent: Mutex<trimcraft_core::vjoy::VjoyState>,
    /// The input backend's clock (event timestamps), for timed outputs started by commands.
    clock: fn() -> u64,
    backups: Arc<backups::Backups>,
    toasts: Arc<toasts::Toasts>,
    /// An import file the user picked and has not applied yet.
    pending_import: Mutex<Option<trimcraft_core::transfer::ImportFile>>,
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
        self.log_profile(engine);
        let signals = engine.take_macro_signals();
        if !signals.is_empty() {
            self.player.signal(signals);
        }
        let actions: Vec<_> = engine
            .take_actions()
            .into_iter()
            .filter(|request| {
                let enabled = engine.action_enabled(&request.action);
                if !enabled {
                    // Info in the file: the event log shows it as a warning of its own.
                    log::info!(
                        "action skipped, its connection is disabled: {} / {}: {}",
                        request.control,
                        request.state,
                        request.action.describe()
                    );
                    self.diagnostics.record(action_record(
                        &ActionEvent::Skipped {
                            reason: "its connection is disabled".into(),
                        },
                        &executor::origin(request),
                        &request.action.describe(),
                    ));
                }
                enabled
            })
            .collect();
        if !actions.is_empty() {
            self.executor.submit(actions);
        }
        let requests = engine.take_toasts();
        if !requests.is_empty() {
            let overlay = &engine.setup().overlay;
            // With the overlay hidden and "while hidden" off, a toast would have nowhere to show.
            if overlay.visible || overlay.toasts.while_hidden || engine.overlay_editing() {
                self.toasts.push(requests, overlay.toasts.duration_ms);
            }
        }
        self.send_vjoy(engine);
    }

    /// Hands the engine's vJoy output to the vJoy thread when it changed.
    fn send_vjoy(&self, engine: &Engine) {
        let state = engine.vjoy_state();
        let mut sent = self.vjoy_sent.lock().unwrap_or_else(|e| e.into_inner());
        if *sent != state {
            sent.clone_from(&state);
            self.vjoy.submit(state);
        }
    }

    /// Now on the input clock.
    pub fn now_ns(&self) -> u64 {
        (self.clock)()
    }

    /// Logs a change of the active profile, however it happened.
    fn log_profile(&self, engine: &Engine) {
        let setup = engine.setup();
        let mut logged = self
            .logged_profile
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if *logged != setup.active_profile {
            logged.clone_from(&setup.active_profile);
            let foreground = engine.foreground();
            log::info!(
                "profile: \"{}\" active (foreground: {})",
                setup.profile().name,
                foreground.unwrap_or("-")
            );
        }
    }

    fn save(&self, engine: &mut Engine) {
        let dirty = engine.take_dirty();
        if dirty.registry {
            self.storage.save_json(DEVICES_FILE, engine.registry());
        }
        if dirty.setup {
            let text = engine.setup().to_json();
            if self.storage.save_text(SETUP_FILE, &text) {
                self.backups.note_saved(&text);
            }
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
    let diagnostics = Diagnostics::default();
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
    // Before the log plugin creates the new folders and before the setup loads.
    let migrated = migration::folders(&identifier);
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
                        file_name: Some("trimcraft".into()),
                    }),
                    // Warnings and errors also go to the event log.
                    tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::Dispatch(
                        log_bridge(diagnostics.clone()),
                    )),
                ])
                .build(),
        )
        // Used from Rust only (open_discord_portal); the windows get no opener permission.
        .plugin(tauri_plugin_opener::init())
        // Used from Rust only (export and import dialogs); the windows get no dialog permission.
        .plugin(tauri_plugin_dialog::init())
        // Used from Rust only (tray notice); the windows get no notification permission.
        .plugin(tauri_plugin_notification::init())
        // Used from Rust only (updates.rs); the windows get no updater permission.
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, shortcut, event| {
                    if event.state != tauri_plugin_global_shortcut::ShortcutState::Pressed {
                        return;
                    }
                    let shared = app.state::<Arc<Shared>>();
                    let panic = shared.engine().setup().settings.panic_hotkey.clone();
                    if panic
                        .parse::<tauri_plugin_global_shortcut::Shortcut>()
                        .ok()
                        .as_ref()
                        == Some(shortcut)
                    {
                        log::warn!("panic hotkey: stopping all macros, releasing vJoy");
                        let shared = Arc::clone(&shared);
                        // Stopping waits for the macro threads; not on the event loop.
                        let _ = std::thread::Builder::new()
                            .name("trimcraft-panic".into())
                            .spawn(move || {
                                shared.player.panic();
                                // Then vJoy: buttons released, POVs centred (axes stay).
                                let mut engine = shared.engine();
                                engine.vjoy_release_all();
                                shared.commit(&mut engine);
                            });
                    } else {
                        overlay::toggle(app);
                    }
                })
                .build(),
        )
        .setup(move |app| {
            migrated.log(&diagnostics);
            // Before the connectors start and look for their secrets.
            migration::credentials(&diagnostics);
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
            let backend = trimcraft_input::default_backend();
            let shared = Arc::new(load(
                &handle,
                backend.name(),
                storage,
                setup,
                elevated,
                diagnostics,
                backend.clock(),
            )?);
            app.manage(shared.clone());
            report_connectors(&shared);
            migrate_autostart(&shared);
            sync_autostart(&shared);
            tray::create(&handle, shared.engine().setup().overlay.visible)?;
            overlay::create(&handle, &shared)?;
            let panic = shared.engine().setup().settings.panic_hotkey.clone();
            if let Err(e) = overlay::register_hotkey(&handle, &panic) {
                log::error!("panic hotkey {panic:?} could not be registered: {e}");
            }
            start_diagnostics(handle.clone(), shared.diagnostics.clone());
            watch_foreground(&shared);
            if let Some(main) = handle.get_webview_window("main") {
                if hidden {
                    log::info!("started in the tray");
                } else {
                    main.show()?;
                }
            }
            updates::check_on_startup(&handle, &shared);
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
        .build(context)
        .expect("error while building Trimcraft")
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event
                && let Some(shared) = app.try_state::<Arc<Shared>>()
            {
                // Nothing stays pressed after Trimcraft is gone: macros stop and release their
                // keys first, then vJoy releases its buttons and gives the devices back.
                shared.player.shutdown();
                shared.vjoy.shutdown();
            }
        });
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
            .title("Trimcraft keeps running in the tray")
            .body("Inputs, actions and the overlay stay active. Quit from the tray icon's menu.")
            .show();
        if let Err(e) = shown {
            log::warn!("could not show the tray notification: {e}");
        }
    }
}

/// Removes Detent's autostart (Trimcraft's former name), in the background (schtasks). Touches
/// none of Trimcraft's own entries, so it can run alongside [`sync_autostart`].
fn migrate_autostart(shared: &Arc<Shared>) {
    let (elevated, diagnostics) = (shared.meta.elevated, shared.diagnostics.clone());
    let spawned = std::thread::Builder::new()
        .name("trimcraft-migration".into())
        .spawn(move || migration::autostart(elevated, &diagnostics));
    if let Err(e) = spawned {
        log::error!("could not start the autostart migration: {e}");
    }
}

/// Registers or removes autostart to match the settings, in the background (schtasks takes a
/// moment), and reports the result to the Settings page.
pub fn sync_autostart(shared: &Arc<Shared>) {
    let shared = Arc::clone(shared);
    let spawned = std::thread::Builder::new()
        .name("trimcraft-autostart".into())
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
    diagnostics: Diagnostics,
    clock: fn() -> u64,
) -> Result<Shared, Box<dyn std::error::Error>> {
    let registry: DeviceRegistry = storage.load_json(DEVICES_FILE).unwrap_or_default();
    let values: BTreeMap<String, f64> = storage.load_json(VALUES_FILE).unwrap_or_default();
    let control_states: BTreeMap<String, trimcraft_core::RadioMemory> =
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
        "Trimcraft {} on {}, input backend {}, administrator: {}",
        meta.version,
        meta.platform,
        meta.backend,
        if meta.elevated { "yes" } else { "no" }
    );
    let mut engine = Engine::new(registry, setup, &values);
    engine.set_diagnostics(diagnostics.clone());
    engine.restore_active_states(&control_states);
    engine.set_shell_status(ShellStatus {
        elevated,
        ..ShellStatus::default()
    });
    let soundpad = Arc::new(Soundpad::spawn(engine.connections().soundpad.clone()));
    let discord = Arc::new(Discord::spawn(engine.connections().discord.clone()));
    let vjoy_enabled = engine.connections().vjoy.enabled;
    let diagnostics_for_vjoy = diagnostics.clone();
    let backups = backups::Backups::spawn(&storage.dir, diagnostics.clone());
    let voicemeeter = Arc::new(voicemeeter::Voicemeeter::spawn(
        engine.connections().voicemeeter.clone(),
    ));
    let speech = Arc::new(speech::Speech::spawn());
    let sound = Arc::new(sound::Sound::spawn());
    let player = player::Player::new(app.clone(), diagnostics.clone());
    let services = executor::Services {
        soundpad: soundpad.clone(),
        discord: discord.clone(),
        voicemeeter: voicemeeter.clone(),
        speech: speech.clone(),
        sound: sound.clone(),
        player: player.clone(),
    };
    Ok(Shared {
        engine: Mutex::new(engine),
        changed: AtomicBool::new(true),
        storage,
        executor: Executor::spawn(app.clone(), services, diagnostics.clone()),
        diagnostics,
        soundpad,
        discord,
        vjoy: vjoy::Vjoy::spawn(vjoy_enabled, diagnostics_for_vjoy),
        voicemeeter,
        speech,
        sound,
        recorder: recorder::Recorder::default(),
        player,
        meta,
        logged_profile: Mutex::new(String::new()),
        vjoy_sent: Mutex::new(trimcraft_core::vjoy::VjoyState::default()),
        clock,
        backups,
        toasts: toasts::Toasts::spawn(app.clone()),
        pending_import: Mutex::new(None),
    })
}

/// Input thread: applies every backend event to the engine as it arrives, emits snapshots to
/// the windows at most once per frame, and saves changes.
fn start_input(
    app: AppHandle,
    shared: Arc<Shared>,
    backend: Box<dyn trimcraft_input::InputBackend>,
) {
    std::thread::Builder::new()
        .name("trimcraft-engine".into())
        .spawn(move || {
            let backend = trimcraft_input::spawn(backend);
            let mut backend_alive = true;
            let mut last_emit = Instant::now();
            let mut last_values_save = Instant::now();
            loop {
                if !backend_alive {
                    std::thread::sleep(FRAME);
                } else {
                    // Wake for the next frame, or earlier when a long or double press is due.
                    let deadline = shared.engine().next_deadline();
                    let wait = deadline.map_or(FRAME, |at| {
                        Duration::from_nanos(at.saturating_sub(backend.now_ns())).min(FRAME)
                    });
                    match backend.events.recv_timeout(wait) {
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
                        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                            if deadline.is_some_and(|at| backend.now_ns() >= at) {
                                let mut engine = shared.engine();
                                if engine.advance(backend.now_ns()) {
                                    shared.changed.store(true, Ordering::Relaxed);
                                }
                                shared.dispatch(&mut engine);
                            }
                        }
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
                    if let Err(e) = app.emit("trimcraft://snapshot", &snapshot) {
                        log::warn!("could not send snapshot: {e}");
                    }
                }
                overlay::keep_on_top(&app);
            }
        })
        .expect("spawn engine thread");
}

/// One foreground watcher for both consumers: the engine switches profiles as set, and the
/// main window warns when the foreground program runs as administrator and Trimcraft does not.
fn watch_foreground(shared: &Arc<Shared>) {
    let weak = Arc::downgrade(shared);
    foreground::watch(move |fg| {
        let Some(shared) = weak.upgrade() else { return };
        profile_watch::on_foreground(&shared, fg);
        elevation::check_foreground(&shared, fg);
    });
}

/// Connector status changes go into the engine, so both windows see them.
fn report_connectors(shared: &Arc<Shared>) {
    use trimcraft_core::ConnectorId;
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
    shared.vjoy.on_status(reporter(ConnectorId::Vjoy));
    // Restored states (and the active profile) are on vJoy from the start.
    shared.send_vjoy(&shared.engine());
    shared
        .voicemeeter
        .on_status(reporter(ConnectorId::Voicemeeter));
}

/// Warnings and errors from the `log` crate, as event log entries.
fn log_bridge(diagnostics: Diagnostics) -> tauri_plugin_log::fern::Dispatch {
    tauri_plugin_log::fern::Dispatch::new()
        .level(log::LevelFilter::Warn)
        .chain(tauri_plugin_log::fern::Output::call(move |record| {
            let level = if record.level() == log::Level::Error {
                Level::Error
            } else {
                Level::Warn
            };
            diagnostics.record(
                Record::new(Category::Log, record.args().to_string())
                    .level(level)
                    .detail(format!("log target: {}", record.target())),
            );
        }))
}

/// How often new event log entries are sent to the windows. Batching keeps heavy input from
/// flooding the UI; the entries themselves are recorded as they happen.
const DIAGNOSTICS_BATCH: Duration = Duration::from_millis(100);

/// Sends entries recorded since the last batch to the windows as one event.
fn start_diagnostics(app: AppHandle, diagnostics: Diagnostics) {
    std::thread::Builder::new()
        .name("trimcraft-diagnostics".into())
        .spawn(move || {
            let mut cursor = diagnostics.all().last().map_or(0, |e| e.seq);
            loop {
                std::thread::sleep(DIAGNOSTICS_BATCH);
                let batch = diagnostics.since(cursor);
                let Some(last) = batch.last() else { continue };
                cursor = last.seq;
                if let Err(e) = app.emit("trimcraft://diagnostics", &batch) {
                    // Not logged as a warning: that would itself become an entry.
                    log::debug!("could not send diagnostics: {e}");
                }
            }
        })
        .expect("spawn diagnostics thread");
}

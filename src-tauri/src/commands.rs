//! Commands the UI invokes. Each one changes the engine, saves, and triggers a redraw.

use crate::{Meta, Shared, elevation, overlay, profile_watch, secrets};
use crate::{volume, webhook};
use detent_core::audio::VoiceRef;
use detent_core::connection::{DiscordSettings, SecretRef, valid_client_id};
use detent_core::discord;
use detent_core::recording::{RecordKind, RecorderConfig};
use detent_core::{
    Action, ActionRequest, Anchor, AppSettings, ButtonPair, Connections, ConnectorId, ControlRef,
    DetectorConfig, DeviceId, EngineSnapshot, Function, KeyInfo, ManualCommand, MonitorRef,
    Movement, OverlayItem, Phase, ProfileSwitching, Sound, VirtualControl, WidgetPosition,
};
use serde::Deserialize;
use std::sync::Arc;
use tauri::{AppHandle, State};

type Res<T> = Result<T, String>;

pub fn handler() -> impl Fn(tauri::ipc::Invoke) -> bool + Send + Sync + 'static {
    tauri::generate_handler![
        get_meta,
        get_snapshot,
        set_device_label,
        add_encoder,
        rename_encoder,
        swap_encoder,
        remove_encoder,
        dismiss_suggestion,
        save_function,
        remove_function,
        set_function_value,
        toggle_overlay_control,
        toggle_overlay_function,
        set_overlay_layout,
        set_overlay_visible,
        set_hotkey,
        set_detector_config,
        get_keys,
        save_virtual_control,
        remove_virtual_control,
        set_radio_active,
        command_radio,
        set_control_value,
        toggle_overlay_virtual_control,
        start_learn,
        stop_learn,
        list_monitors,
        set_overlay_monitor,
        set_overlay_widget,
        set_overlay_positions,
        set_overlay_editing,
        set_connections,
        test_connection,
        save_discord_credentials,
        discord_setup,
        open_discord_portal,
        forget_discord_authorization,
        remove_discord_credentials,
        soundpad_sounds,
        start_recording,
        stop_recording,
        cancel_recording,
        clear_recording,
        set_recorder_config,
        test_action,
        list_voices,
        list_sound_devices,
        list_audio_programs,
        store_http_secret,
        get_diagnostics,
        clear_diagnostics,
        open_log_folder,
        crate::transfer::export_setup,
        crate::transfer::pick_import_file,
        crate::transfer::cancel_import,
        crate::transfer::apply_import,
        crate::transfer::list_backups,
        crate::transfer::backup_now,
        crate::transfer::restore_backup,
        crate::transfer::open_backup_folder,
        set_overlay_appearance,
        set_toast_settings,
        set_toast_position,
        get_toasts,
        test_toast,
        set_settings,
        restart_elevated,
        select_profile,
        create_profile,
        duplicate_profile,
        rename_profile,
        delete_profile,
        make_default_profile,
        set_profile_executables,
        set_profile_switching,
        toggle_overlay_profile_name,
        list_process_windows,
    ]
}

/// Runs `f` on the engine, then saves and redraws.
fn with<T>(
    shared: &Shared,
    f: impl FnOnce(&mut detent_core::Engine) -> Result<T, detent_core::EngineError>,
) -> Res<T> {
    let mut engine = shared.engine();
    let result = f(&mut engine).map_err(|e| e.to_string());
    shared.commit(&mut engine);
    result
}

#[tauri::command]
fn get_meta(shared: State<'_, Arc<Shared>>) -> Meta {
    shared.meta.clone()
}

#[tauri::command]
fn get_snapshot(shared: State<'_, Arc<Shared>>) -> EngineSnapshot {
    shared.engine().snapshot()
}

#[tauri::command]
fn set_device_label(
    shared: State<'_, Arc<Shared>>,
    device: DeviceId,
    label: Option<String>,
) -> Res<()> {
    with(&shared, |e| e.set_device_label(&device, label))
}

#[tauri::command]
fn add_encoder(
    shared: State<'_, Arc<Shared>>,
    name: String,
    device: DeviceId,
    decrement: u16,
    increment: u16,
) -> Res<String> {
    with(&shared, |e| {
        e.add_encoder(name, device, decrement, increment)
    })
}

#[tauri::command]
fn rename_encoder(shared: State<'_, Arc<Shared>>, id: String, name: String) -> Res<()> {
    with(&shared, |e| e.rename_encoder(&id, name))
}

#[tauri::command]
fn swap_encoder(shared: State<'_, Arc<Shared>>, id: String) -> Res<()> {
    with(&shared, |e| e.swap_encoder(&id))
}

#[tauri::command]
fn remove_encoder(shared: State<'_, Arc<Shared>>, id: String) -> Res<()> {
    with(&shared, |e| e.remove_encoder(&id))
}

#[tauri::command]
fn dismiss_suggestion(shared: State<'_, Arc<Shared>>, pair: ButtonPair) -> Res<()> {
    with(&shared, |e| {
        e.dismiss_suggestion(pair);
        Ok(())
    })
}

#[tauri::command]
fn save_function(shared: State<'_, Arc<Shared>>, function: Function) -> Res<String> {
    with(&shared, |e| e.save_function(function))
}

#[tauri::command]
fn remove_function(shared: State<'_, Arc<Shared>>, id: String) -> Res<()> {
    with(&shared, |e| e.remove_function(&id))
}

#[tauri::command]
fn set_function_value(shared: State<'_, Arc<Shared>>, id: String, value: f64) -> Res<()> {
    with(&shared, |e| e.set_function_value(&id, value))
}

#[tauri::command]
fn toggle_overlay_control(
    shared: State<'_, Arc<Shared>>,
    device: DeviceId,
    control: ControlRef,
) -> Res<bool> {
    with(&shared, |e| Ok(e.toggle_overlay_control(&device, control)))
}

#[tauri::command]
fn toggle_overlay_function(shared: State<'_, Arc<Shared>>, id: String) -> Res<bool> {
    with(&shared, |e| e.toggle_overlay_function(&id))
}

#[derive(Debug, Deserialize)]
struct Layout {
    anchor: Anchor,
    margin: u32,
    scale: f32,
}

#[tauri::command]
fn set_overlay_layout(shared: State<'_, Arc<Shared>>, layout: Layout) -> Res<()> {
    if !(0.25..=4.0).contains(&layout.scale) {
        return Err("size must be between 25% and 400%".into());
    }
    with(&shared, |e| {
        let mut overlay = e.overlay();
        overlay.anchor = layout.anchor;
        overlay.margin = layout.margin.min(10_000);
        overlay.scale = layout.scale;
        e.set_overlay(overlay);
        Ok(())
    })
}

#[tauri::command]
fn set_overlay_visible(app: AppHandle, shared: State<'_, Arc<Shared>>, visible: bool) -> Res<()> {
    with(&shared, |e| {
        e.set_overlay_visible(visible);
        Ok(())
    })?;
    overlay::set_visible(&app, visible);
    Ok(())
}

#[tauri::command]
fn set_hotkey(app: AppHandle, shared: State<'_, Arc<Shared>>, hotkey: String) -> Res<()> {
    let old = shared.engine().setup().overlay.hotkey.clone();
    if old == hotkey {
        return Ok(());
    }
    let panic = shared.engine().setup().settings.panic_hotkey.clone();
    if overlay::same_hotkey(&hotkey, &panic) {
        return Err("the overlay hotkey must differ from the panic hotkey".into());
    }
    overlay::unregister_hotkey(&app, &old);
    if let Err(e) = overlay::register_hotkey(&app, &hotkey) {
        // Keep the previous hotkey working.
        let _ = overlay::register_hotkey(&app, &old);
        return Err(e);
    }
    with(&shared, |e| {
        let mut overlay = e.overlay();
        overlay.hotkey = hotkey;
        e.set_overlay(overlay);
        Ok(())
    })
}

#[tauri::command]
fn set_detector_config(shared: State<'_, Arc<Shared>>, config: DetectorConfig) -> Res<()> {
    if config.max_pulse_ms <= 0.0 || config.max_run_gap_ms <= 0.0 || config.min_run_length < 2 {
        return Err("thresholds must be positive and a run needs at least 2 pulses".into());
    }
    with(&shared, |e| {
        e.set_detector_config(config);
        Ok(())
    })
}

#[tauri::command]
fn get_keys() -> &'static [KeyInfo] {
    detent_core::action::KEYS
}

#[tauri::command]
fn save_virtual_control(shared: State<'_, Arc<Shared>>, control: VirtualControl) -> Res<String> {
    with_secrets(&shared, |e| e.save_virtual_control(control))
}

#[tauri::command]
fn remove_virtual_control(shared: State<'_, Arc<Shared>>, id: String) -> Res<()> {
    with_secrets(&shared, |e| e.remove_virtual_control(&id))
}

/// Like [`with`], for changes that can drop actions: secret HTTP header values no action
/// uses any more are deleted from the credential store.
fn with_secrets<T>(
    shared: &Shared,
    f: impl FnOnce(&mut detent_core::Engine) -> Result<T, detent_core::EngineError>,
) -> Res<T> {
    let before = shared.engine().setup().secret_refs();
    let result = with(shared, f)?;
    let after = shared.engine().setup().secret_refs();
    for secret in before.iter().filter(|s| !after.contains(s)) {
        if let Err(e) = webhook::delete_secret(secret) {
            log::warn!("{e}");
        }
    }
    Ok(result)
}

#[tauri::command]
fn set_radio_active(shared: State<'_, Arc<Shared>>, id: String, state: Option<String>) -> Res<()> {
    with(&shared, |e| e.set_radio_active(&id, state))
}

/// Runs a command on any virtual control as if a trigger had fired (next/previous, clear,
/// toggle off, toggle/on/off, reset; which ones apply depends on the kind).
#[tauri::command]
fn command_radio(shared: State<'_, Arc<Shared>>, id: String, command: ManualCommand) -> Res<()> {
    with(&shared, |e| e.command_control(&id, command))
}

/// Sets a counter control's value without running actions.
#[tauri::command]
fn set_control_value(shared: State<'_, Arc<Shared>>, id: String, value: f64) -> Res<()> {
    with(&shared, |e| e.set_control_value(&id, value))
}

#[tauri::command]
fn toggle_overlay_virtual_control(shared: State<'_, Arc<Shared>>, id: String) -> Res<bool> {
    with(&shared, |e| e.toggle_overlay_virtual_control(&id))
}

// --- Profiles -----------------------------------------------------------------------------

/// The profile selector: makes a profile active (no actions run).
#[tauri::command]
fn select_profile(shared: State<'_, Arc<Shared>>, id: String) -> Res<()> {
    with(&shared, |e| e.select_profile(&id))
}

/// Creates an empty profile and makes it active. Returns its id.
#[tauri::command]
fn create_profile(shared: State<'_, Arc<Shared>>, name: String) -> Res<String> {
    with(&shared, |e| Ok(e.create_profile(&name)))
}

/// Copies a profile (controls, widgets, radio states; not the executables) and makes the copy
/// active. Returns its id.
#[tauri::command]
fn duplicate_profile(shared: State<'_, Arc<Shared>>, id: String, name: String) -> Res<String> {
    with(&shared, |e| e.duplicate_profile(&id, &name))
}

#[tauri::command]
fn rename_profile(shared: State<'_, Arc<Shared>>, id: String, name: String) -> Res<()> {
    with(&shared, |e| e.rename_profile(&id, &name))
}

#[tauri::command]
fn delete_profile(shared: State<'_, Arc<Shared>>, id: String) -> Res<()> {
    with_secrets(&shared, |e| e.delete_profile(&id))
}

/// Moves a profile to the front: the default profile.
#[tauri::command]
fn make_default_profile(shared: State<'_, Arc<Shared>>, id: String) -> Res<()> {
    with(&shared, |e| e.make_default_profile(&id))
}

#[tauri::command]
fn set_profile_executables(
    shared: State<'_, Arc<Shared>>,
    id: String,
    executables: Vec<String>,
) -> Res<()> {
    with(&shared, |e| e.set_profile_executables(&id, executables))
}

/// Auto-switch on/off and what happens when no profile matches.
#[tauri::command]
fn set_profile_switching(shared: State<'_, Arc<Shared>>, switching: ProfileSwitching) -> Res<()> {
    with(&shared, |e| {
        e.set_profile_switching(switching);
        Ok(())
    })
}

#[tauri::command]
fn toggle_overlay_profile_name(shared: State<'_, Arc<Shared>>) -> Res<bool> {
    with(&shared, |e| Ok(e.toggle_overlay_profile_name()))
}

/// Programs with a window, for "Pick from running processes".
#[tauri::command]
fn list_process_windows() -> Vec<profile_watch::ProcessWindow> {
    profile_watch::windows()
}

#[tauri::command]
fn start_learn(shared: State<'_, Arc<Shared>>) -> Res<()> {
    with(&shared, |e| {
        e.start_learn();
        Ok(())
    })
}

#[tauri::command]
fn stop_learn(shared: State<'_, Arc<Shared>>) -> Res<()> {
    with(&shared, |e| {
        e.stop_learn();
        Ok(())
    })
}

#[tauri::command]
fn list_monitors(app: AppHandle) -> Res<Vec<overlay::MonitorInfo>> {
    overlay::monitors(&app)
}

#[tauri::command]
fn set_overlay_monitor(
    app: AppHandle,
    shared: State<'_, Arc<Shared>>,
    monitor: Option<MonitorRef>,
) -> Res<()> {
    with(&shared, |e| {
        e.set_overlay_monitor(monitor.clone());
        Ok(())
    })?;
    overlay::cover_monitor(&app, monitor.as_ref());
    Ok(())
}

#[tauri::command]
fn set_overlay_widget(shared: State<'_, Arc<Shared>>, index: usize, item: OverlayItem) -> Res<()> {
    with(&shared, |e| e.set_overlay_widget(index, item))
}

#[tauri::command]
fn set_overlay_positions(
    shared: State<'_, Arc<Shared>>,
    positions: Vec<Option<WidgetPosition>>,
) -> Res<()> {
    with(&shared, |e| e.set_overlay_positions(positions))
}

/// Layout edit mode: the overlay takes the mouse (and is shown even if hidden) so widgets
/// can be dragged; leaving it restores click-through and the saved visibility.
#[tauri::command]
fn set_overlay_editing(app: AppHandle, shared: State<'_, Arc<Shared>>, editing: bool) -> Res<()> {
    let visible = with(&shared, |e| {
        e.set_overlay_editing(editing);
        Ok(e.setup().overlay.visible)
    })?;
    overlay::set_visible(&app, visible);
    log::info!(
        "overlay layout edit mode {}",
        if editing { "on" } else { "off" }
    );
    Ok(())
}

/// Saves connector settings (Connections page) and applies them. Discord's client ID and
/// secret reference only change through [`save_discord_credentials`].
#[tauri::command]
fn set_connections(shared: State<'_, Arc<Shared>>, mut connections: Connections) -> Res<()> {
    let current = shared.engine().connections().discord.clone();
    connections.discord.client_id = current.client_id;
    connections.discord.client_secret = current.client_secret;
    let soundpad = connections.soundpad.clone();
    let discord = connections.discord.clone();
    let voicemeeter = connections.voicemeeter.clone();
    with(&shared, |e| e.set_connections(connections))?;
    shared.soundpad.apply(soundpad);
    shared.discord.apply(discord);
    shared.voicemeeter.apply(voicemeeter);
    Ok(())
}

/// "Test connection": checks the connector now. Async: the pipe round trip (and for Discord
/// the consent popup) must not block the UI thread.
#[tauri::command]
async fn test_connection(shared: State<'_, Arc<Shared>>, id: ConnectorId) -> Res<String> {
    let shared = Arc::clone(&shared);
    tauri::async_runtime::spawn_blocking(move || match id {
        ConnectorId::Soundpad => shared.soundpad.test(),
        ConnectorId::Discord => shared.discord.test(),
        ConnectorId::Voicemeeter => shared.voicemeeter.test(),
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Changes Discord settings and applies them.
fn update_discord(shared: &Shared, change: impl FnOnce(&mut DiscordSettings)) -> Res<()> {
    let mut connections = shared.engine().connections().clone();
    change(&mut connections.discord);
    let discord = connections.discord.clone();
    with(shared, |e| e.set_connections(connections))?;
    shared.discord.apply(discord);
    Ok(())
}

/// Saves the Discord application's client ID and, if given, its secret. The secret goes to
/// the credential store; the setup only gets a reference to it. A different client ID drops
/// the stored authorization, which belongs to the old application.
#[tauri::command]
fn save_discord_credentials(
    shared: State<'_, Arc<Shared>>,
    client_id: String,
    client_secret: Option<String>,
) -> Res<()> {
    let client_id = client_id.trim().to_owned();
    if !valid_client_id(&client_id) {
        return Err(
            "the Client ID is a long number; copy it from the Developer Portal (OAuth2 page)"
                .into(),
        );
    }
    let secret = client_secret
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty());
    let current = shared.engine().connections().discord.clone();
    let has_secret = current.client_secret.is_some() && secrets::exists(discord::SECRET_TARGET);
    if secret.is_none() && !has_secret {
        return Err("enter the Client Secret".into());
    }
    if let Some(secret) = &secret {
        secrets::set(discord::SECRET_TARGET, secret)?;
    }
    if client_id != current.client_id {
        secrets::delete(discord::TOKENS_TARGET)?;
    }
    log::info!(
        "discord: credentials saved (client id {client_id}{})",
        if secret.is_some() { ", new secret" } else { "" }
    );
    update_discord(&shared, |d| {
        d.client_id = client_id;
        d.client_secret = Some(DiscordSettings::secret_ref());
    })
}

/// Redirect URI, scopes and portal link for the in-app setup guide, from the constants the
/// connector uses.
#[tauri::command]
fn discord_setup() -> discord::SetupInfo {
    discord::setup_info()
}

/// Opens the Discord Developer Portal in the default browser. Only this fixed URL can be
/// opened, so the UI cannot open arbitrary links.
#[tauri::command]
fn open_discord_portal(app: AppHandle) -> Res<()> {
    use tauri_plugin_opener::OpenerExt;
    app.opener()
        .open_url(discord::PORTAL_URL, None::<&str>)
        .map_err(|e| format!("cannot open the browser: {e}"))
}

/// "Forget authorization": deletes the OAuth tokens and disconnects. The next Test connection
/// asks in Discord again.
#[tauri::command]
fn forget_discord_authorization(shared: State<'_, Arc<Shared>>) -> Res<()> {
    secrets::delete(discord::TOKENS_TARGET)?;
    log::info!("discord: authorization forgotten");
    shared.discord.forget();
    Ok(())
}

/// "Remove credentials": deletes client secret and tokens and clears the client ID.
#[tauri::command]
fn remove_discord_credentials(shared: State<'_, Arc<Shared>>) -> Res<()> {
    secrets::delete(discord::TOKENS_TARGET)?;
    secrets::delete(discord::SECRET_TARGET)?;
    log::info!("discord: credentials removed");
    update_discord(&shared, |d| {
        d.client_id.clear();
        d.client_secret = None;
    })?;
    shared.discord.forget();
    Ok(())
}

/// Soundpad's sound list ("All sounds"), for the sound picker.
#[tauri::command]
async fn soundpad_sounds(shared: State<'_, Arc<Shared>>) -> Res<Vec<Sound>> {
    let soundpad = shared.soundpad.clone();
    tauri::async_runtime::spawn_blocking(move || soundpad.sounds())
        .await
        .map_err(|e| e.to_string())?
}

/// Starts recording a macro after the countdown. Keyboard macros record keys, mouse macros
/// buttons, wheel and movement (absolute or relative).
#[tauri::command]
fn start_recording(
    app: AppHandle,
    shared: State<'_, Arc<Shared>>,
    kind: RecordKind,
    movement: Movement,
) -> Res<()> {
    shared.recorder.start(&app, &shared, kind, movement)
}

/// The Stop button: ends the recording and keeps it.
#[tauri::command]
fn stop_recording(shared: State<'_, Arc<Shared>>) {
    shared.recorder.stop();
}

#[tauri::command]
fn cancel_recording(shared: State<'_, Arc<Shared>>) {
    shared.recorder.cancel();
}

/// The editor took the recording (or dismissed its error).
#[tauri::command]
fn clear_recording(shared: State<'_, Arc<Shared>>) -> Res<()> {
    with(&shared, |e| {
        e.clear_recording();
        Ok(())
    })
}

#[tauri::command]
fn set_recorder_config(shared: State<'_, Arc<Shared>>, config: RecorderConfig) -> Res<()> {
    with(&shared, |e| e.set_recorder_config(config))
}

/// Plays an action being edited. Input macros wait for the recording countdown, so the user
/// can switch to the game first; everything else runs at once. Placeholders get "Test play",
/// "editor" and the active profile. A "while active" macro has no state to end here, so it
/// plays once; "n times" plays n times (stop it with the panic hotkey).
#[tauri::command]
fn test_action(shared: State<'_, Arc<Shared>>, mut action: Action) -> Res<()> {
    action.normalize().map_err(|e| e.to_string())?;
    if !shared.engine().action_enabled(&action) {
        return Err("this action's connection is disabled; enable it under Connections".into());
    }
    let countdown = matches!(action, Action::InputMacro(_));
    if let Action::InputMacro(m) = &mut action {
        // Not guarded against the saved macro: a test may run while it does.
        m.id.clear();
        if m.repeat == detent_core::macros::Repeat::WhileActive {
            m.repeat = detent_core::macros::Repeat::Once;
        }
    }
    let mut request = ActionRequest::new("Test play", "editor", Phase::Activate, &action);
    request.profile = shared.engine().setup().profile().name.clone();
    let shared = Arc::clone(&shared);
    std::thread::Builder::new()
        .name("detent-test-play".into())
        .spawn(move || {
            if countdown {
                std::thread::sleep(std::time::Duration::from_millis(
                    detent_core::recording::COUNTDOWN_MS,
                ));
            }
            shared.executor.submit(vec![request]);
        })
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Installed Windows voices, for the speech editor.
#[tauri::command]
async fn list_voices(shared: State<'_, Arc<Shared>>) -> Res<Vec<VoiceRef>> {
    let speech = shared.speech.clone();
    tauri::async_runtime::spawn_blocking(move || speech.voices())
        .await
        .map_err(|e| e.to_string())?
}

/// Sound output devices, for the sound file editor.
#[tauri::command]
async fn list_sound_devices(shared: State<'_, Arc<Shared>>) -> Res<Vec<String>> {
    let sound = shared.sound.clone();
    tauri::async_runtime::spawn_blocking(move || sound.devices())
        .await
        .map_err(|e| e.to_string())?
}

/// Programs that have an audio session now, for the volume editor.
#[tauri::command]
async fn list_audio_programs() -> Res<Vec<String>> {
    tauri::async_runtime::spawn_blocking(volume::session_programs)
        .await
        .map_err(|e| e.to_string())?
}

/// Stores a secret HTTP header value in the credential store; the editor keeps only the
/// returned reference. Unused ones are deleted when the action is saved without them.
#[tauri::command]
fn store_http_secret(value: String) -> Res<SecretRef> {
    webhook::store_secret(&value)
}

/// The event log held by the app, oldest first. New entries arrive as `detent://diagnostics`
/// events; entries are numbered, so the UI can drop ones it already has.
#[tauri::command]
fn get_diagnostics(shared: State<'_, Arc<Shared>>) -> Vec<detent_core::Entry> {
    shared.diagnostics.all()
}

#[tauri::command]
fn clear_diagnostics(shared: State<'_, Arc<Shared>>) {
    shared.diagnostics.clear();
}

/// Opens the log folder in the file manager. Only that folder can be opened.
#[tauri::command]
fn open_log_folder(app: AppHandle, shared: State<'_, Arc<Shared>>) -> Res<()> {
    use tauri_plugin_opener::OpenerExt;
    let dir = std::path::PathBuf::from(&shared.meta.log_dir);
    if shared.meta.log_dir.is_empty() {
        return Err("the log folder is unknown".into());
    }
    // The folder appears with the first log line; make sure it exists.
    std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create the log folder: {e}"))?;
    app.opener()
        .open_path(dir.display().to_string(), None::<&str>)
        .map_err(|e| format!("cannot open the log folder: {e}"))
}

/// Saves the Settings page ("General") and registers or removes autostart to match.
#[tauri::command]
fn set_settings(
    app: AppHandle,
    shared: State<'_, Arc<Shared>>,
    mut settings: AppSettings,
) -> Res<()> {
    settings.panic_hotkey = settings.panic_hotkey.trim().to_owned();
    let old = shared.engine().setup().settings.panic_hotkey.clone();
    if settings.panic_hotkey != old {
        let overlay_hotkey = shared.engine().setup().overlay.hotkey.clone();
        if overlay::same_hotkey(&settings.panic_hotkey, &overlay_hotkey) {
            return Err("the panic hotkey must differ from the overlay hotkey".into());
        }
        overlay::unregister_hotkey(&app, &old);
        if let Err(e) = overlay::register_hotkey(&app, &settings.panic_hotkey) {
            // Keep the previous one working.
            let _ = overlay::register_hotkey(&app, &old);
            return Err(e);
        }
    }
    with(&shared, |e| {
        e.set_settings(settings);
        Ok(())
    })?;
    crate::sync_autostart(&shared);
    Ok(())
}

/// "Restart as administrator": starts an elevated Detent (UAC prompt) with its window shown,
/// and quits this one. Async: the prompt must not block the UI thread.
#[tauri::command]
async fn restart_elevated(app: AppHandle) -> Res<()> {
    tauri::async_runtime::spawn_blocking(|| elevation::relaunch(true))
        .await
        .map_err(|e| e.to_string())??;
    app.exit(0);
    Ok(())
}

/// Colors, text size and corners of the overlay (Settings > Overlay appearance). Values are
/// kept within their bounds.
#[tauri::command]
fn set_overlay_appearance(
    shared: State<'_, Arc<Shared>>,
    appearance: detent_core::Appearance,
) -> Res<()> {
    with(&shared, |e| {
        e.set_overlay_appearance(appearance);
        Ok(())
    })
}

/// How long toasts stay, whether they show while the overlay is hidden, and the profile toast.
#[tauri::command]
fn set_toast_settings(
    shared: State<'_, Arc<Shared>>,
    settings: detent_core::overlay::ToastSettings,
) -> Res<()> {
    with(&shared, |e| {
        e.set_toast_settings(settings);
        Ok(())
    })
}

/// Places the toast area of the active profile; `None` puts it back to the default place.
#[tauri::command]
fn set_toast_position(shared: State<'_, Arc<Shared>>, position: Option<WidgetPosition>) -> Res<()> {
    with(&shared, |e| {
        e.set_toast_position(position);
        Ok(())
    })
}

/// The toasts on screen now; later changes arrive as `detent://toasts` events.
#[tauri::command]
fn get_toasts(shared: State<'_, Arc<Shared>>) -> Vec<detent_core::Toast> {
    shared.toasts.list()
}

/// Shows a sample toast, as a real one would appear.
#[tauri::command]
fn test_toast(shared: State<'_, Arc<Shared>>) {
    let (settings, visible, editing) = {
        let engine = shared.engine();
        let overlay = &engine.setup().overlay;
        (overlay.toasts, overlay.visible, engine.overlay_editing())
    };
    if visible || settings.while_hidden || editing {
        shared.toasts.push(
            vec![detent_core::ToastRequest {
                key: "test".into(),
                title: "Test toast".into(),
                text: "It works".into(),
            }],
            settings.duration_ms,
        );
    }
}

#[cfg(test)]
mod tests {
    //! The UI sends these payloads (see ui/src/lib/backend.ts); they must deserialise.
    use super::*;

    #[test]
    fn ui_payloads_deserialise() {
        let _: ControlRef = serde_json::from_str(r#"{"type":"axis","index":2}"#).unwrap();
        let _: ButtonPair =
            serde_json::from_str(r#"{"device":"f00d:0003#1","decrement":10,"increment":11}"#)
                .unwrap();
        let _: DetectorConfig =
            serde_json::from_str(r#"{"maxPulseMs":200,"maxRunGapMs":600,"minRunLength":3}"#)
                .unwrap();
        let _: Layout =
            serde_json::from_str(r#"{"anchor":"bottomLeft","margin":32,"scale":1.5}"#).unwrap();
        let f: Function = serde_json::from_str(
            r#"{"id":"","name":"Channel","kind":{"type":"counter",
                "spec":{"min":1,"max":20,"step":1,"initial":1,"end":"wrap","decimals":0},
                "input":{"type":"encoder","encoder":"enc-1"}}}"#,
        )
        .unwrap();
        assert_eq!(f.encoder(), Some("enc-1"));

        let c: VirtualControl = serde_json::from_str(
            r#"{"id":"","name":"Comms","kind":{"type":"radio",
                "states":[
                  {"id":"","name":"Guard",
                   "select":[{"type":"button","device":"f00d:0001#1","index":3}],
                   "onActivate":[{"type":"keyboardMacro","steps":[
                      {"type":"keyDown","key":"LeftCtrl"},
                      {"type":"keyTap","key":"F1","holdMs":null},
                      {"type":"keyTap","key":"F2","holdMs":80},
                      {"type":"delay","ms":100},
                      {"type":"keyUp","key":"LeftCtrl"}]}],
                   "onDeactivate":[{"type":"mouseMacro","steps":[
                      {"type":"buttonDown","button":"left"},
                      {"type":"buttonUp","button":"left"},
                      {"type":"click","button":"x1","holdMs":null},
                      {"type":"wheel","delta":-1},
                      {"type":"moveRelative","dx":5,"dy":-5},
                      {"type":"delay","ms":10}]}]}],
                "increment":[{"type":"encoderStep","encoder":"enc-1","direction":"increment"}],
                "decrement":[{"type":"encoderStep","encoder":"enc-1","direction":"decrement"}],
                "clear":[{"type":"hat","device":"f00d:0001#1","index":0,"direction":"upLeft"}],
                "toggleOff":[{"type":"button","device":"f00d:0001#1","index":7}],
                "end":"clamp","toggleOnReselect":true,"startup":"empty"}}"#,
        )
        .unwrap();
        assert_eq!(c.radio().unwrap().states[0].on_activate.len(), 1);
        assert_eq!(c.radio().unwrap().startup, detent_core::Startup::Empty);
        let item: OverlayItem = serde_json::from_str(
            r#"{"type":"radio","control":"vc-1","layout":"horizontal","position":{"x":0.25,"y":0.5}}"#,
        )
        .unwrap();
        assert_eq!(item.position, Some(WidgetPosition { x: 0.25, y: 0.5 }));
        let _: OverlayItem = serde_json::from_str(
            r#"{"type":"control","device":"d","control":{"type":"hat","index":0},"label":null,"position":null}"#,
        )
        .unwrap();
        let positions: Vec<Option<WidgetPosition>> =
            serde_json::from_str(r#"[{"x":0.1,"y":0.9},null]"#).unwrap();
        assert_eq!(positions.len(), 2);
        let _: Option<MonitorRef> = serde_json::from_str(
            r#"{"name":"\\\\.\\DISPLAY2","x":-2560,"y":0,"width":2560,"height":1440}"#,
        )
        .unwrap();
        let _: Option<MonitorRef> = serde_json::from_str("null").unwrap();
        assert_eq!(c.radio().unwrap().toggle_off.len(), 1);
        let sp: VirtualControl = serde_json::from_str(
            r#"{"id":"vc-1","name":"Sounds","kind":{"type":"radio","states":[
                {"id":"st-1","name":"Siren","select":[],
                 "onActivate":[{"type":"soundpad","command":{"type":"play",
                    "sound":{"title":"siren","path":"C:\\Sounds\\siren.mp3"},"output":"speakers"}}],
                 "onDeactivate":[{"type":"soundpad","command":{"type":"stop"}},
                    {"type":"soundpad","command":{"type":"togglePause"}},
                    {"type":"soundpad","command":{"type":"playRandom","output":"mic"}},
                    {"type":"soundpad","command":{"type":"playNext"}},
                    {"type":"soundpad","command":{"type":"playPrevious"}}]}],
                "increment":[],"decrement":[],"clear":[],"toggleOff":[],
                "end":"wrap","toggleOnReselect":false,"startup":"restoreLast"}}"#,
        )
        .unwrap();
        assert_eq!(sp.radio().unwrap().states[0].on_deactivate.len(), 5);

        // Connections.
        let connections: Connections = serde_json::from_str(
            r#"{"soundpad":{"enabled":true,"timeoutMs":800,"listMaxAgeMs":10000}}"#,
        )
        .unwrap();
        assert!(connections.validate().is_ok() && connections.soundpad.enabled);
        let id: ConnectorId = serde_json::from_str(r#""soundpad""#).unwrap();
        assert_eq!(id, ConnectorId::Soundpad);
        let id: ConnectorId = serde_json::from_str(r#""discord""#).unwrap();
        assert_eq!(id, ConnectorId::Discord);
        // As the UI sends them: the stored secret reference comes back unchanged.
        let connections: Connections = serde_json::from_str(
            r#"{"soundpad":{"enabled":false,"timeoutMs":500,"listMaxAgeMs":10000},
                "discord":{"enabled":true,"clientId":"1234567890123456789",
                  "clientSecret":{"store":"windowsCredentialManager","target":"detent/discord/client-secret"}}}"#,
        )
        .unwrap();
        assert!(connections.validate().is_ok() && connections.discord.enabled);
        let dc: VirtualControl = serde_json::from_str(
            r#"{"id":"vc-2","name":"Voice","kind":{"type":"radio","states":[
                {"id":"st-1","name":"Muted","select":[],
                 "onActivate":[{"type":"discord","command":"mute"},{"type":"discord","command":"deafen"}],
                 "onDeactivate":[{"type":"discord","command":"unmute"},{"type":"discord","command":"undeafen"},
                    {"type":"discord","command":"toggleMute"},{"type":"discord","command":"toggleDeafen"}]}],
                "increment":[],"decrement":[],"clear":[],"toggleOff":[],
                "end":"wrap","toggleOnReselect":false,"startup":"restoreLast"}}"#,
        )
        .unwrap();
        assert_eq!(dc.radio().unwrap().states[0].on_deactivate.len(), 4);
        // The setup guide reads these (ui/src/lib/types.ts: DiscordSetup).
        let setup = serde_json::to_value(discord_setup()).unwrap();
        assert_eq!(setup["redirectUri"], discord::REDIRECT_URI);
        assert_eq!(setup["portalUrl"], discord::PORTAL_URL);
        assert_eq!(setup["scopes"].as_array().unwrap().len(), 3);

        // Settings page: as the UI sends it (the tray notice flag is ignored by the engine).
        let settings: AppSettings = serde_json::from_str(
            r#"{"closeToTray":false,"startWithWindows":true,"startMinimized":true,
                "runAsAdmin":true,"trayNoticeShown":false}"#,
        )
        .unwrap();
        assert!(settings.start_with_windows && settings.run_as_admin && !settings.close_to_tray);
        // More action types, as ActionList.svelte creates them (newAction) and the editors
        // change them.
        let more: Vec<Action> = serde_json::from_str(
            r#"[
              {"type":"speak","text":"{state}","voice":{"id":"HKEY_LOCAL_MACHINE/x","name":"Zira"},"rate":0,"volume":100,"wait":false},
              {"type":"playSound","path":"C:/s.wav","volume":100,"device":null,"stopPrevious":false,"wait":true},
              {"type":"volume","target":{"type":"master"},"command":{"type":"toggleMute"}},
              {"type":"volume","target":{"type":"app","process":"Discord.exe"},"command":{"type":"set","percent":50}},
              {"type":"voicemeeter","target":{"kind":"strip","index":0},"command":{"type":"changeGain","deltaDb":3}},
              {"type":"mediaKey","key":"playPause"},
              {"type":"launch","target":{"type":"program","path":"C:/a.exe","arguments":"","workingDir":"","onlyIfNotRunning":true},"wait":false},
              {"type":"launch","target":{"type":"url","url":"https://example.com"},"wait":false},
              {"type":"http","method":"POST","url":"https://x.test","headers":[
                {"name":"Authorization","value":"","secret":{"store":"windowsCredentialManager","target":"detent/http/1"}}],
               "body":"","timeoutMs":5000,"wait":false}
            ]"#,
        )
        .unwrap();
        assert_eq!(more.len(), 9);
        for mut action in more {
            action.normalize().unwrap();
        }
        let connections: Connections = serde_json::from_str(
            r#"{"soundpad":{"enabled":false,"timeoutMs":500,"listMaxAgeMs":10000},
                "discord":{"enabled":false,"clientId":"","clientSecret":null},
                "voicemeeter":{"enabled":true}}"#,
        )
        .unwrap();
        assert!(connections.voicemeeter.enabled);
        let id: ConnectorId = serde_json::from_str(r#""voicemeeter""#).unwrap();
        assert_eq!(id, ConnectorId::Voicemeeter);
        // What the pickers and store_http_secret return (types.ts: VoiceRef, SecretRef).
        let voice = serde_json::to_value(VoiceRef {
            id: "a".into(),
            name: "b".into(),
        })
        .unwrap();
        assert_eq!(
            (voice["id"].as_str(), voice["name"].as_str()),
            (Some("a"), Some("b"))
        );
        let secret = serde_json::to_value(SecretRef::WindowsCredentialManager {
            target: "detent/http/1".into(),
        })
        .unwrap();
        assert_eq!(secret["store"], "windowsCredentialManager");
        let view = serde_json::to_value(detent_core::connection::ConnectorView::new(
            ConnectorId::Voicemeeter,
            false,
            Some(&detent_core::ConnectorState {
                missing: Some("Voicemeeter is not installed".into()),
                ..Default::default()
            }),
        ))
        .unwrap();
        assert_eq!(view["missing"], "Voicemeeter is not installed");

        // Recorder and test play.
        let both: RecordKind = serde_json::from_str(r#"{"keys":true,"mouse":true}"#).unwrap();
        assert_eq!(both, RecordKind::BOTH);
        // Input macros as MacroEditor.svelte builds them, and Stop macros.
        let mut mixed: Action = serde_json::from_str(
            r#"{"type":"inputMacro","id":"mac-1","movement":"absolute","monitor":null,
                "playback":{"speedPercent":100,"minDelayMs":0,"endpointOnly":false},
                "repeat":{"type":"times","count":3},"onEnd":"finishRun","pauseMs":250,"retrigger":"restart",
                "steps":[{"type":"keyDown","key":"LeftCtrl"},{"type":"click","button":"left","holdMs":null},
                  {"type":"delay","ms":20},{"type":"keyUp","key":"LeftCtrl"}]}"#,
        )
        .unwrap();
        mixed.normalize().unwrap();
        assert_eq!(mixed.input_macro().unwrap().pause_ms, 250);
        let _: Action = serde_json::from_str(
            r#"{"type":"inputMacro","steps":[],"repeat":{"type":"whileActive"}}"#,
        )
        .unwrap();
        for stop in [
            r#"{"type":"stopMacros","target":{"type":"all"}}"#,
            r#"{"type":"stopMacros","target":{"type":"macro","id":"mac-1"}}"#,
        ] {
            let mut a: Action = serde_json::from_str(stop).unwrap();
            a.normalize().unwrap();
        }
        let settings: AppSettings =
            serde_json::from_str(r#"{"panicHotkey":"Ctrl+Alt+P"}"#).unwrap();
        assert_eq!(settings.panic_hotkey, "Ctrl+Alt+P");
        assert!(
            detent_core::settings::DEFAULT_PANIC_HOTKEY
                .parse::<tauri_plugin_global_shortcut::Shortcut>()
                .is_ok()
        );
        // The default panic hotkey differs from the default overlay hotkey, also when spelled
        // differently.
        assert!(!overlay::same_hotkey(
            detent_core::settings::DEFAULT_PANIC_HOTKEY,
            &detent_core::OverlayConfig::default().hotkey
        ));
        assert!(overlay::same_hotkey("Ctrl+Shift+F12", "shift+ctrl+F12"));
        let _: Movement = serde_json::from_str(r#""absolute""#).unwrap();
        let _: Movement = serde_json::from_str(r#""relative""#).unwrap();
        let config: RecorderConfig = serde_json::from_str(
            r#"{"stopHotkey":"Ctrl+Shift+F9","tolerance":2.5,"sampleIntervalMs":8}"#,
        )
        .unwrap();
        assert!(config.validate().is_ok());
        let mut recorded: Action = serde_json::from_str(
            r#"{"type":"mouseMacro","movement":"absolute",
                "playback":{"speedPercent":200,"minDelayMs":10,"endpointOnly":true},
                "monitor":{"name":"\\\\.\\DISPLAY2","x":3440,"y":0,"width":1440,"height":3440},
                "steps":[{"type":"moveTo","x":0.5,"y":0.5},
                  {"type":"movePath","points":[{"x":0.1,"y":0.1,"t":0},{"x":0.2,"y":0.3,"t":16}]},
                  {"type":"moveRelativePath","points":[{"dx":3,"dy":-1,"t":0}]},
                  {"type":"click","button":"left","holdMs":40},{"type":"delay","ms":120}]}"#,
        )
        .unwrap();
        recorded.normalize().unwrap();
        let mut keys: Action = serde_json::from_str(
            r#"{"type":"keyboardMacro","playback":{"speedPercent":100,"minDelayMs":0,"endpointOnly":false},
                "steps":[{"type":"keyTap","key":"W","holdMs":890}]}"#,
        )
        .unwrap();
        keys.normalize().unwrap();
        // Profiles.
        let switching: ProfileSwitching =
            serde_json::from_str(r#"{"autoSwitch":false,"noMatch":"switchToDefault"}"#).unwrap();
        assert_eq!(switching.no_match, detent_core::NoMatch::SwitchToDefault);
        let _: ProfileSwitching =
            serde_json::from_str(r#"{"autoSwitch":true,"noMatch":"keepCurrent"}"#).unwrap();
        let _: Vec<String> = serde_json::from_str(r#"["DCS.exe","C:\\Games\\x.exe"]"#).unwrap();
        for target in [
            r#"{"type":"profile","profile":"profile-2"}"#,
            r#"{"type":"next"}"#,
            r#"{"type":"previous"}"#,
        ] {
            let mut action: Action =
                serde_json::from_str(&format!(r#"{{"type":"switchProfile","target":{target}}}"#))
                    .unwrap();
            action.normalize().unwrap();
        }
        let _: OverlayItem =
            serde_json::from_str(r#"{"type":"profileName","position":{"x":0.5,"y":0.02}}"#)
                .unwrap();
        // The picker reads these (ui/src/lib/types.ts: ProcessWindow).
        let window = serde_json::to_value(profile_watch::ProcessWindow {
            exe: "DCS.exe".into(),
            title: "DCS".into(),
        })
        .unwrap();
        assert_eq!(window["exe"], "DCS.exe");
        assert_eq!(window["title"], "DCS");

        for command in [
            r#""increment""#,
            r#""decrement""#,
            r#""clear""#,
            r#""toggleOff""#,
            r#""toggle""#,
            r#""on""#,
            r#""off""#,
            r#""reset""#,
        ] {
            let _: ManualCommand = serde_json::from_str(command).unwrap();
        }

        // Control kinds and triggers as the editors build them.
        let trigger = r#"{"type":"button","device":"d","index":3,"press":{"kind":"press"},"modifiers":[],"conditions":[]}"#;
        for kind in [
            format!(
                r#"{{"type":"toggle","toggle":[{trigger}],"on":[],"off":[],"onOn":[],"onOff":[],"startup":"restoreLast"}}"#
            ),
            format!(
                r#"{{"type":"momentary","triggers":[{trigger}],"onPress":[],"onRelease":[],"invert":false}}"#
            ),
            format!(
                r#"{{"type":"counter","spec":{{"min":1,"max":20,"step":1,"initial":1,"end":"wrap","decimals":0}},
                    "increment":[{trigger}],"decrement":[],"reset":[],"onChange":[],"startup":"restoreLast"}}"#
            ),
            format!(
                r#"{{"type":"mode","states":[{{"id":"","name":"Air","select":[],"shift":[{trigger}],
                    "onActivate":[],"onDeactivate":[]}}],"next":[],"previous":[],"end":"wrap","startup":"restoreLast"}}"#
            ),
        ] {
            let mut c: VirtualControl =
                serde_json::from_str(&format!(r#"{{"id":"","name":"X","kind":{kind}}}"#)).unwrap();
            c.normalize().unwrap();
        }
        let t: detent_core::Trigger = serde_json::from_str(
            r#"{"type":"axisZone","device":"d","index":2,"min":80,"max":100,
                "press":{"kind":"release"},
                "modifiers":[{"device":"d","index":7}],
                "conditions":[{"mode":"vc-2","state":"st-1"}]}"#,
        )
        .unwrap();
        assert_eq!(t.modifiers.len(), 1);
        for press in [
            r#"{"kind":"long","ms":500}"#,
            r#"{"kind":"double","ms":300}"#,
            r#"{"kind":"release"}"#,
        ] {
            let _: detent_core::PressKind = serde_json::from_str(press).unwrap();
        }
    }

    #[test]
    fn key_list_serialises_for_the_ui() {
        let json = serde_json::to_value(detent_core::action::KEYS).unwrap();
        assert_eq!(json[0]["name"], "A");
        assert_eq!(json[0]["scan"], 0x1E);
        assert_eq!(json[0]["extended"], false);
    }

    #[test]
    fn default_hotkey_parses() {
        let hotkey = detent_core::OverlayConfig::default().hotkey;
        assert!(
            hotkey
                .parse::<tauri_plugin_global_shortcut::Shortcut>()
                .is_ok()
        );
    }

    #[test]
    fn appearance_and_toast_payloads_deserialise() {
        let appearance: detent_core::Appearance = serde_json::from_str(
            r##"{"theme":"hudGreen","accent":"#39ff6a","backgroundOpacity":60,"textScale":1.25,"cornerRadius":4}"##,
        )
        .unwrap();
        assert_eq!(appearance.theme, detent_core::overlay::Theme::HudGreen);
        let settings: detent_core::overlay::ToastSettings =
            serde_json::from_str(r#"{"durationMs":3000,"whileHidden":false,"profileToast":true}"#)
                .unwrap();
        assert_eq!(settings.duration_ms, 3000);
        // A widget as the Overlay panel sends it back, with the new settings.
        let item: OverlayItem = serde_json::from_str(
            r#"{"type":"profileName","position":null,"visibility":{"mode":"afterChange","seconds":3},"scale":1.5,"opacity":0.5}"#,
        )
        .unwrap();
        assert_eq!(item.scale, Some(1.5));
        let position: Option<WidgetPosition> =
            serde_json::from_str(r#"{"x":0.5,"y":0.1}"#).unwrap();
        assert!(position.is_some());
    }
}

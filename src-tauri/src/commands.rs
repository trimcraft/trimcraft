//! Commands the UI invokes. Each one changes the engine, saves, and triggers a redraw.

use crate::{Meta, Shared, overlay};
use detent_core::{
    Anchor, ButtonPair, ControlRef, DetectorConfig, DeviceId, EngineSnapshot, Function, KeyInfo,
    MonitorRef, OverlayItem, RadioCommand, VirtualControl, WidgetPosition,
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
        toggle_overlay_virtual_control,
        start_learn,
        stop_learn,
        list_monitors,
        set_overlay_monitor,
        set_overlay_widget,
        set_overlay_positions,
        set_overlay_editing,
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
        let mut overlay = e.setup().overlay.clone();
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
    overlay::unregister_hotkey(&app, &old);
    if let Err(e) = overlay::register_hotkey(&app, &hotkey) {
        // Keep the previous hotkey working.
        let _ = overlay::register_hotkey(&app, &old);
        return Err(e);
    }
    with(&shared, |e| {
        let mut overlay = e.setup().overlay.clone();
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
    with(&shared, |e| e.save_virtual_control(control))
}

#[tauri::command]
fn remove_virtual_control(shared: State<'_, Arc<Shared>>, id: String) -> Res<()> {
    with(&shared, |e| e.remove_virtual_control(&id))
}

#[tauri::command]
fn set_radio_active(shared: State<'_, Arc<Shared>>, id: String, state: Option<String>) -> Res<()> {
    with(&shared, |e| e.set_radio_active(&id, state))
}

/// Radio commands the UI can run, as if a trigger had fired.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
enum UiRadioCommand {
    Increment,
    Decrement,
    Clear,
    ToggleOff,
}

#[tauri::command]
fn command_radio(shared: State<'_, Arc<Shared>>, id: String, command: UiRadioCommand) -> Res<()> {
    let command = match command {
        UiRadioCommand::Increment => RadioCommand::Increment,
        UiRadioCommand::Decrement => RadioCommand::Decrement,
        UiRadioCommand::Clear => RadioCommand::Clear,
        UiRadioCommand::ToggleOff => RadioCommand::ToggleOff,
    };
    with(&shared, |e| e.command_radio(&id, command))
}

#[tauri::command]
fn toggle_overlay_virtual_control(shared: State<'_, Arc<Shared>>, id: String) -> Res<bool> {
    with(&shared, |e| e.toggle_overlay_virtual_control(&id))
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
        assert_eq!(c.radio().states[0].on_activate.len(), 1);
        assert_eq!(c.radio().startup, detent_core::Startup::Empty);
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
        assert_eq!(c.radio().toggle_off.len(), 1);
        for command in [
            r#""increment""#,
            r#""decrement""#,
            r#""clear""#,
            r#""toggleOff""#,
        ] {
            let _: UiRadioCommand = serde_json::from_str(command).unwrap();
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
}

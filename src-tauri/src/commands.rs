//! Commands the UI invokes. Each one changes the engine, saves, and triggers a redraw.

use crate::{Meta, Shared, overlay};
use detent_core::{
    Anchor, ButtonPair, ControlRef, DetectorConfig, DeviceId, EngineSnapshot, Function,
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

//! The overlay window: transparent, borderless, always on top, ignores the mouse.
//!
//! It never touches the game process. It is visible over games in windowed or borderless
//! fullscreen mode, not over exclusive fullscreen.

use crate::Shared;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

const LABEL: &str = "overlay";

/// Some games re-assert their own z-order; reclaim "topmost" this often while visible.
const TOPMOST_INTERVAL: Duration = Duration::from_secs(2);

pub fn create(app: &AppHandle, shared: &Arc<Shared>) -> tauri::Result<()> {
    let window = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("overlay.html".into()))
        .title("Detent Overlay")
        .transparent(true)
        .decorations(false)
        .shadow(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .focused(false)
        .visible(false)
        .build()?;
    cover_primary_monitor(&window);
    let visible = shared.engine().setup().overlay.visible;
    set_visible(app, visible);

    let hotkey = shared.engine().setup().overlay.hotkey.clone();
    if let Err(e) = register_hotkey(app, &hotkey) {
        log::error!("overlay hotkey {hotkey:?} could not be registered: {e}");
    }
    Ok(())
}

fn cover_primary_monitor(window: &WebviewWindow) {
    match window.primary_monitor() {
        Ok(Some(monitor)) => {
            let _ = window.set_position(*monitor.position());
            let _ = window.set_size(*monitor.size());
            log::info!(
                "overlay covers primary monitor {:?} at {:?}, {:?}",
                monitor.name(),
                monitor.position(),
                monitor.size()
            );
        }
        Ok(None) => log::warn!("no primary monitor found for the overlay"),
        Err(e) => log::warn!("could not query monitors: {e}"),
    }
}

pub fn set_visible(app: &AppHandle, visible: bool) {
    let Some(window) = app.get_webview_window(LABEL) else {
        return;
    };
    let result = if visible {
        window
            .show()
            .and_then(|()| window.set_always_on_top(true))
            .and_then(|()| window.set_ignore_cursor_events(true))
    } else {
        window.hide()
    };
    if let Err(e) = result {
        log::warn!("could not change overlay visibility: {e}");
    }
}

/// Hotkey handler: flips visibility and remembers it.
pub fn toggle(app: &AppHandle) {
    let shared = app.state::<Arc<Shared>>();
    let mut engine = shared.engine();
    let visible = !engine.setup().overlay.visible;
    engine.set_overlay_visible(visible);
    shared.commit(&mut engine);
    drop(engine);
    set_visible(app, visible);
    log::info!(
        "overlay {} by hotkey",
        if visible { "shown" } else { "hidden" }
    );
}

pub fn register_hotkey(app: &AppHandle, hotkey: &str) -> Result<(), String> {
    use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut};
    let shortcut: Shortcut = hotkey
        .parse()
        .map_err(|e| format!("invalid hotkey {hotkey:?}: {e}"))?;
    app.global_shortcut()
        .register(shortcut)
        .map_err(|e| e.to_string())
}

pub fn unregister_hotkey(app: &AppHandle, hotkey: &str) {
    use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut};
    if let Ok(shortcut) = hotkey.parse::<Shortcut>() {
        let _ = app.global_shortcut().unregister(shortcut);
    }
}

pub fn keep_on_top(app: &AppHandle) {
    static LAST: Mutex<Option<Instant>> = Mutex::new(None);
    let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
    if last.is_some_and(|t| t.elapsed() < TOPMOST_INTERVAL) {
        return;
    }
    *last = Some(Instant::now());
    if let Some(window) = app.get_webview_window(LABEL)
        && window.is_visible().unwrap_or(false)
    {
        let _ = window.set_always_on_top(true);
    }
}

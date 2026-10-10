//! The overlay window: transparent, borderless, always on top, covering one monitor. It
//! ignores the mouse except in layout edit mode.
//!
//! It never touches the game process. It is visible over games in windowed or borderless
//! fullscreen mode, not over exclusive fullscreen.

use crate::Shared;
use detent_core::MonitorRef;
use serde::Serialize;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager, Monitor, WebviewUrl, WebviewWindowBuilder};

const LABEL: &str = "overlay";

/// Some games re-assert their own z-order; reclaim "topmost" this often while visible.
const TOPMOST_INTERVAL: Duration = Duration::from_secs(2);

pub fn create(app: &AppHandle, shared: &Arc<Shared>) -> tauri::Result<()> {
    WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("overlay.html".into()))
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
    let monitor = shared.engine().setup().overlay.monitor.clone();
    cover_monitor(app, monitor.as_ref());
    let visible = shared.engine().setup().overlay.visible;
    set_visible(app, visible);

    let hotkey = shared.engine().setup().overlay.hotkey.clone();
    if let Err(e) = register_hotkey(app, &hotkey) {
        log::error!("overlay hotkey {hotkey:?} could not be registered: {e}");
    }
    Ok(())
}

/// A monitor for the picker in the UI.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MonitorInfo {
    #[serde(flatten)]
    pub monitor: MonitorRef,
    pub primary: bool,
    pub scale: f64,
}

fn to_ref(m: &Monitor) -> MonitorRef {
    MonitorRef {
        name: m.name().cloned(),
        x: m.position().x,
        y: m.position().y,
        width: m.size().width,
        height: m.size().height,
    }
}

pub fn monitors(app: &AppHandle) -> Result<Vec<MonitorInfo>, String> {
    let primary = app
        .primary_monitor()
        .map_err(|e| e.to_string())?
        .map(|m| to_ref(&m));
    Ok(app
        .available_monitors()
        .map_err(|e| e.to_string())?
        .iter()
        .map(|m| {
            let monitor = to_ref(m);
            MonitorInfo {
                primary: primary.as_ref() == Some(&monitor),
                scale: m.scale_factor(),
                monitor,
            }
        })
        .collect())
}

/// The connected monitors, for absolute mouse positions in macros.
pub fn screens(app: &AppHandle) -> Option<detent_core::Screens> {
    match monitors(app) {
        Ok(list) => Some(detent_core::Screens {
            primary: list.iter().position(|m| m.primary),
            monitors: list.into_iter().map(|m| m.monitor).collect(),
        }),
        Err(e) => {
            log::warn!("could not query monitors: {e}");
            None
        }
    }
}

/// Moves the overlay window over the saved monitor, or the primary one if none is saved or
/// the saved one is not connected.
pub fn cover_monitor(app: &AppHandle, saved: Option<&MonitorRef>) {
    let Some(window) = app.get_webview_window(LABEL) else {
        return;
    };
    let chosen = saved.and_then(|saved| {
        let available = window.available_monitors().unwrap_or_default();
        let refs: Vec<MonitorRef> = available.iter().map(to_ref).collect();
        let found = detent_core::overlay::find_monitor(saved, &refs);
        if found.is_none() {
            log::warn!(
                "overlay monitor {:?} at {},{} is not connected; using the primary monitor",
                saved.name,
                saved.x,
                saved.y
            );
        }
        found.map(|i| available[i].clone())
    });
    let monitor = match chosen {
        Some(m) => m,
        None => match window.primary_monitor() {
            Ok(Some(m)) => m,
            Ok(None) => return log::warn!("no primary monitor found for the overlay"),
            Err(e) => return log::warn!("could not query monitors: {e}"),
        },
    };
    let _ = window.set_position(*monitor.position());
    let _ = window.set_size(*monitor.size());
    log::info!(
        "overlay covers monitor {:?} at {:?}, {:?}",
        monitor.name(),
        monitor.position(),
        monitor.size()
    );
}

/// Layout edit mode, and whether a macro recording is counting down or running.
fn modes(app: &AppHandle) -> (bool, bool) {
    let shared = app.state::<Arc<Shared>>();
    let engine = shared.engine();
    (engine.overlay_editing(), engine.recording_active())
}

/// Applies the saved visibility again, e.g. after a recording started or ended.
pub fn refresh_visibility(app: &AppHandle) {
    let visible = app.state::<Arc<Shared>>().engine().setup().overlay.visible;
    set_visible(app, visible);
}

/// Shows the overlay window when `visible`, in layout edit mode, or while a macro is being
/// recorded (it shows the recording indicator). It only takes the mouse in edit mode;
/// otherwise clicks go through to the game.
pub fn set_visible(app: &AppHandle, visible: bool) {
    let Some(window) = app.get_webview_window(LABEL) else {
        return;
    };
    let (editing, recording) = modes(app);
    let result = if visible || editing || recording {
        window
            .show()
            .and_then(|()| window.set_always_on_top(true))
            .and_then(|()| window.set_ignore_cursor_events(!editing))
    } else {
        window.hide()
    };
    if editing {
        let _ = window.set_focus();
    }
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

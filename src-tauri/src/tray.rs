//! The tray icon: always present while Detent runs. Left click shows the main window; the
//! menu has Show window, Overlay visible (kept in sync with the hotkey) and Quit.

use crate::overlay;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Wry};

const SHOW: &str = "show";
const OVERLAY: &str = "overlay";
const QUIT: &str = "quit";

/// The overlay checkbox, to keep it in sync.
struct Tray {
    overlay: CheckMenuItem<Wry>,
}

pub fn create(app: &AppHandle, overlay_visible: bool) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, SHOW, "Show window", true, None::<&str>)?;
    let overlay = CheckMenuItem::with_id(
        app,
        OVERLAY,
        "Overlay visible",
        true,
        overlay_visible,
        None::<&str>,
    )?;
    let separator = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, QUIT, "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &overlay, &separator, &quit])?;
    let mut builder = TrayIconBuilder::with_id("detent")
        .tooltip("Detent")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            SHOW => show_main(app),
            OVERLAY => {
                let visible = !overlay::saved_visible(app);
                overlay::set_shown(app, visible, "from the tray");
            }
            QUIT => {
                log::info!("quit from the tray");
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    app.manage(Tray { overlay });
    Ok(())
}

/// Ticks the overlay checkbox to match the saved visibility.
pub fn set_overlay_checked(app: &AppHandle, visible: bool) {
    if let Some(tray) = app.try_state::<Tray>()
        && let Err(e) = tray.overlay.set_checked(visible)
    {
        log::warn!("could not update the tray menu: {e}");
    }
}

/// Shows, restores and focuses the main window (tray click, second launch).
pub fn show_main(app: &AppHandle) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    let result = window
        .show()
        .and_then(|()| window.unminimize())
        .and_then(|()| window.set_focus());
    if let Err(e) = result {
        log::warn!("could not show the main window: {e}");
    }
}

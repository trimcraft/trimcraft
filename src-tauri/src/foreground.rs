//! Warns when the foreground program runs as administrator and Detent does not: Windows then
//! drops Detent's keys and mouse (UIPI). Event-driven through a WinEvent hook on foreground
//! changes, on a thread of its own; it only reads the foreground process's token.
//!
//! Only runs while Detent is not elevated; an elevated Detent reaches every program.

use crate::{Shared, elevation};
use detent_core::settings::{ProcessElevation, foreground_warning};
use std::sync::atomic::Ordering;
use std::sync::{Arc, OnceLock};
use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::System::Threading::GetCurrentProcessId;
use windows_sys::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, EVENT_SYSTEM_FOREGROUND, GetForegroundWindow, GetMessageW, GetWindowTextW,
    GetWindowThreadProcessId, MSG, TranslateMessage, WINEVENT_OUTOFCONTEXT,
};

/// The hook procedure is a plain function; it reaches the app through this.
static SHARED: OnceLock<Arc<Shared>> = OnceLock::new();

pub fn watch(shared: Arc<Shared>) {
    if SHARED.set(shared).is_err() {
        return;
    }
    let spawned = std::thread::Builder::new()
        .name("detent-foreground".into())
        .spawn(|| {
            // SAFETY: the hook procedure is a valid callback for the life of the process;
            // out-of-context hooks are delivered to this thread's message loop.
            let hook = unsafe {
                SetWinEventHook(
                    EVENT_SYSTEM_FOREGROUND,
                    EVENT_SYSTEM_FOREGROUND,
                    std::ptr::null_mut(),
                    Some(on_foreground),
                    0,
                    0,
                    WINEVENT_OUTOFCONTEXT,
                )
            };
            if hook.is_null() {
                return log::error!("could not watch the foreground window");
            }
            // SAFETY: no preconditions.
            check(unsafe { GetForegroundWindow() });
            let mut msg = MSG::default();
            // SAFETY: standard message loop on this thread.
            while unsafe { GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) } > 0 {
                // SAFETY: as above.
                unsafe {
                    TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
        });
    if let Err(e) = spawned {
        log::error!("could not start the foreground watcher: {e}");
    }
}

unsafe extern "system" fn on_foreground(
    _hook: HWINEVENTHOOK,
    _event: u32,
    hwnd: HWND,
    _object: i32,
    _child: i32,
    _thread: u32,
    _time: u32,
) {
    check(hwnd);
}

fn check(hwnd: HWND) {
    let Some(shared) = SHARED.get() else { return };
    if hwnd.is_null() {
        return;
    }
    let mut pid = 0;
    // SAFETY: any window handle; a stale one gives pid 0.
    unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
    if pid == 0 {
        return;
    }
    // SAFETY: no preconditions.
    let is_self = pid == unsafe { GetCurrentProcessId() };
    let (elevation, exe) = if is_self {
        (ProcessElevation::NotElevated, None)
    } else {
        elevation::process_info(pid)
    };
    let name = exe
        .or_else(|| window_title(hwnd))
        .unwrap_or_else(|| "The program in the foreground".into());

    let mut engine = shared.engine();
    let mut status = engine.shell_status().clone();
    let warning = foreground_warning(
        status.elevated,
        is_self,
        elevation,
        &name,
        status.elevated_foreground.as_deref(),
    );
    if warning == status.elevated_foreground {
        return;
    }
    match &warning {
        Some(name) => log::warn!(
            "{name} runs as administrator; Detent's keys and mouse will not reach it \
             ({elevation:?})"
        ),
        None => log::info!("the foreground program no longer runs as administrator"),
    }
    status.elevated_foreground = warning;
    if engine.set_shell_status(status) {
        shared.changed.store(true, Ordering::Relaxed);
    }
}

fn window_title(hwnd: HWND) -> Option<String> {
    let mut buf = [0u16; 256];
    // SAFETY: `buf` holds the given number of characters.
    let len = unsafe { GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32) };
    (len > 0).then(|| String::from_utf16_lossy(&buf[..len as usize]))
}

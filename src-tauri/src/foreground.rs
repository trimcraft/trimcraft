//! The one foreground-window watcher. It feeds automatic profile switching
//! ([`crate::profile_watch`]) and the warning about an elevated foreground program
//! ([`crate::elevation::check_foreground`]).
//!
//! Event-driven: `SetWinEventHook(EVENT_SYSTEM_FOREGROUND)` on a thread of its own with a
//! message loop; Windows calls back whenever another window comes to the front. Nothing is
//! polled, and nothing is injected into the other process; it only reads the process's name
//! and token.

use crate::elevation;
use detent_core::settings::ProcessElevation;
use std::cell::RefCell;
use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, EVENT_SYSTEM_FOREGROUND, GetForegroundWindow, GetMessageW, GetWindowTextW,
    GetWindowThreadProcessId, MSG, TranslateMessage, WINEVENT_OUTOFCONTEXT,
};

/// The program that just came to the front.
#[derive(Debug, Clone)]
pub struct Foreground {
    /// It is Detent itself (one of its windows).
    pub is_self: bool,
    /// Executable file name, e.g. `DCS.exe`; `None` if it cannot be read.
    pub exe: Option<String>,
    /// Window title, as a fallback name.
    pub title: Option<String>,
    /// Not checked (`NotElevated`) for Detent itself.
    pub elevation: ProcessElevation,
}

impl Foreground {
    /// For the profile switch: `None` for Detent itself, so its windows never switch profiles.
    pub fn program(&self) -> Option<&str> {
        if self.is_self {
            None
        } else {
            self.exe.as_deref()
        }
    }

    /// A name to show to the user.
    pub fn name(&self) -> String {
        self.exe
            .clone()
            .or_else(|| self.title.clone())
            .unwrap_or_else(|| "The program in the foreground".into())
    }
}

type Callback = Box<dyn Fn(&Foreground) + Send>;

thread_local! {
    /// The watcher's callback; only set on the watcher thread, where the hook runs.
    static ON_CHANGE: RefCell<Option<Callback>> = const { RefCell::new(None) };
}

/// Starts the watcher thread. `on_change` runs on that thread, once at the start and then on
/// every foreground change.
pub fn watch(on_change: impl Fn(&Foreground) + Send + 'static) {
    let on_change: Callback = Box::new(on_change);
    let spawned = std::thread::Builder::new()
        .name("detent-foreground".into())
        .spawn(move || {
            // SAFETY: out-of-context hook without a module; the callback is a plain function
            // and runs on this thread, inside GetMessageW below.
            let hook = unsafe {
                SetWinEventHook(
                    EVENT_SYSTEM_FOREGROUND,
                    EVENT_SYSTEM_FOREGROUND,
                    std::ptr::null_mut(),
                    Some(on_event),
                    0,
                    0,
                    WINEVENT_OUTOFCONTEXT,
                )
            };
            if hook.is_null() {
                log::error!(
                    "cannot watch the foreground window; automatic profile switching and the \
                     administrator warning are off"
                );
                return;
            }
            // SAFETY: no arguments.
            if let Some(fg) = inspect(unsafe { GetForegroundWindow() }) {
                on_change(&fg);
            }
            ON_CHANGE.with(|c| *c.borrow_mut() = Some(on_change));
            // SAFETY: a plain message loop; the hook is delivered through it.
            let mut msg: MSG = unsafe { std::mem::zeroed() };
            while unsafe { GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) } > 0 {
                // SAFETY: a message just retrieved by GetMessageW.
                unsafe {
                    TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
        });
    if let Err(e) = spawned {
        log::error!("cannot start the foreground watcher: {e}");
    }
}

unsafe extern "system" fn on_event(
    _hook: HWINEVENTHOOK,
    event: u32,
    hwnd: HWND,
    _object: i32,
    _child: i32,
    _thread: u32,
    _time: u32,
) {
    if event != EVENT_SYSTEM_FOREGROUND {
        return;
    }
    let Some(fg) = inspect(hwnd) else { return };
    ON_CHANGE.with(|c| {
        if let Some(on_change) = &*c.borrow() {
            on_change(&fg);
        }
    });
}

/// What is known about the program owning `hwnd`; `None` for no window.
fn inspect(hwnd: HWND) -> Option<Foreground> {
    if hwnd.is_null() {
        return None;
    }
    let mut pid = 0;
    // SAFETY: `pid` outlives the call; an invalid window gives 0.
    unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
    if pid == 0 {
        return None;
    }
    let is_self = pid == std::process::id();
    let (elevation, exe) = if is_self {
        (ProcessElevation::NotElevated, None)
    } else {
        elevation::process_info(pid)
    };
    Some(Foreground {
        is_self,
        exe,
        title: window_title(hwnd),
        elevation,
    })
}

pub fn window_title(hwnd: HWND) -> Option<String> {
    let mut buf = [0u16; 512];
    // SAFETY: `buf` holds `buf.len()` UTF-16 units.
    let len = unsafe { GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32) };
    (len > 0).then(|| String::from_utf16_lossy(&buf[..len as usize]))
}

#[cfg(test)]
mod tests {
    #[test]
    #[ignore = "needs an interactive desktop"]
    fn reports_the_foreground_program_at_the_start() {
        let (tx, rx) = std::sync::mpsc::channel();
        super::watch(move |fg| {
            let _ = tx.send(fg.clone());
        });
        let first = rx
            .recv_timeout(std::time::Duration::from_secs(2))
            .expect("no report from the watcher");
        println!("foreground: {first:?}");
    }
}

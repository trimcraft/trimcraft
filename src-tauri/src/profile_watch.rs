//! Which program is in the foreground, for automatic profile switching, and the list of
//! programs with windows for "Pick from running processes".
//!
//! The watcher is event-driven: `SetWinEventHook(EVENT_SYSTEM_FOREGROUND)` on a thread of its
//! own with a message loop; Windows calls back whenever another window comes to the front.
//! Nothing is polled. Detent's own windows are reported as `None`, so they never switch
//! profiles.

use serde::Serialize;

/// A program with a visible window, for the picker.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessWindow {
    /// Executable file name, e.g. `DCS.exe`.
    pub exe: String,
    pub title: String,
}

/// Starts the watcher thread. `on_change` gets the foreground program's executable name
/// (`None` for Detent itself or a program whose name cannot be read), once at the start and
/// then on every change, on the watcher thread.
pub fn watch(on_change: impl Fn(Option<String>) + Send + 'static) {
    platform::watch(Box::new(on_change));
}

/// Programs with a visible top-level window and a title, without Detent itself, sorted by
/// executable name.
pub fn windows() -> Vec<ProcessWindow> {
    platform::windows()
}

mod platform {
    use super::ProcessWindow;
    use std::cell::RefCell;
    use windows_sys::Win32::Foundation::{CloseHandle, HWND, LPARAM};
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
        QueryFullProcessImageNameW,
    };
    use windows_sys::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, EVENT_SYSTEM_FOREGROUND, EnumWindows, GW_OWNER, GetForegroundWindow,
        GetMessageW, GetWindow, GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible, MSG,
        TranslateMessage, WINEVENT_OUTOFCONTEXT,
    };
    use windows_sys::core::BOOL;

    type Callback = Box<dyn Fn(Option<String>) + Send>;

    thread_local! {
        /// The watcher's callback; only set on the watcher thread, where the hook runs.
        static ON_CHANGE: RefCell<Option<Callback>> = const { RefCell::new(None) };
    }

    pub fn watch(on_change: Callback) {
        let spawned = std::thread::Builder::new()
            .name("detent-profile-watch".into())
            .spawn(move || {
                // SAFETY: out-of-context hook without a module; the callback is a plain
                // function and runs on this thread, inside GetMessageW below.
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
                        "profiles: cannot watch the foreground window; automatic switching is off"
                    );
                    return;
                }
                // SAFETY: no arguments.
                on_change(exe_of(unsafe { GetForegroundWindow() }));
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
            log::error!("profiles: cannot start the foreground watcher: {e}");
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
        if event != EVENT_SYSTEM_FOREGROUND || hwnd.is_null() {
            return;
        }
        let exe = exe_of(hwnd);
        ON_CHANGE.with(|c| {
            if let Some(on_change) = &*c.borrow() {
                on_change(exe);
            }
        });
    }

    /// Process id of a window's owner.
    fn pid_of(hwnd: HWND) -> u32 {
        let mut pid = 0;
        // SAFETY: `pid` outlives the call; an invalid window gives 0.
        unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
        pid
    }

    /// Executable file name of the program owning `hwnd`; `None` for Detent's own windows and
    /// processes that cannot be opened.
    fn exe_of(hwnd: HWND) -> Option<String> {
        if hwnd.is_null() {
            return None;
        }
        let pid = pid_of(hwnd);
        if pid == 0 || pid == std::process::id() {
            return None;
        }
        exe_name(pid)
    }

    fn exe_name(pid: u32) -> Option<String> {
        // SAFETY: plain call; the handle is closed below.
        let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        if process.is_null() {
            return None;
        }
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        // SAFETY: `buf` holds `len` UTF-16 units.
        let ok = unsafe {
            QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, buf.as_mut_ptr(), &mut len)
        };
        // SAFETY: opened above.
        unsafe { CloseHandle(process) };
        if ok == 0 {
            return None;
        }
        let path = String::from_utf16_lossy(&buf[..len as usize]);
        path.rsplit('\\').next().map(str::to_owned)
    }

    fn title_of(hwnd: HWND) -> String {
        let mut buf = [0u16; 512];
        // SAFETY: `buf` holds `buf.len()` UTF-16 units.
        let len = unsafe { GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32) };
        String::from_utf16_lossy(&buf[..len.max(0) as usize])
    }

    pub fn windows() -> Vec<ProcessWindow> {
        let mut found: Vec<ProcessWindow> = Vec::new();
        // SAFETY: `collect` only runs during this call, with `found` as its parameter.
        unsafe {
            EnumWindows(
                Some(collect),
                &mut found as *mut Vec<ProcessWindow> as LPARAM,
            )
        };
        found.sort();
        found.dedup();
        found
    }

    unsafe extern "system" fn collect(hwnd: HWND, param: LPARAM) -> BOOL {
        // SAFETY: `param` is the `Vec` passed by `windows`, alive for the whole enumeration.
        let found = unsafe { &mut *(param as *mut Vec<ProcessWindow>) };
        // Visible top-level windows without an owner: what the taskbar shows, roughly.
        // SAFETY: plain calls with a window handle from EnumWindows.
        let shown = unsafe { IsWindowVisible(hwnd) != 0 && GetWindow(hwnd, GW_OWNER).is_null() };
        if shown {
            let title = title_of(hwnd);
            if !title.trim().is_empty()
                && let Some(exe) = exe_of(hwnd)
            {
                found.push(ProcessWindow { exe, title });
            }
        }
        1
    }
}

#[cfg(test)]
mod tests {
    #[test]
    #[ignore = "needs an interactive desktop"]
    fn lists_programs_with_windows() {
        let list = super::windows();
        println!("{list:#?}");
        assert!(!list.is_empty());
        assert!(
            list.iter()
                .all(|w| w.exe.contains('.') && !w.title.trim().is_empty())
        );
    }

    #[test]
    #[ignore = "needs an interactive desktop"]
    fn reports_the_foreground_program_at_the_start() {
        let (tx, rx) = std::sync::mpsc::channel();
        super::watch(move |exe| {
            let _ = tx.send(exe);
        });
        let first = rx
            .recv_timeout(std::time::Duration::from_secs(2))
            .expect("no report from the watcher");
        println!("foreground: {first:?}");
    }
}

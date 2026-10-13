//! Automatic profile switching: what the foreground watcher ([`crate::foreground`]) reports
//! goes into the engine. Also the list of programs with windows for "Pick from running
//! processes".

use crate::Shared;
use crate::foreground::Foreground;
use serde::Serialize;

/// A program with a visible window, for the picker.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessWindow {
    /// Executable file name, e.g. `DCS.exe`.
    pub exe: String,
    pub title: String,
}

/// A foreground change: the engine switches profiles as set. Trimcraft's own windows count as
/// no program, so they never switch profiles.
pub fn on_foreground(shared: &Shared, fg: &Foreground) {
    let mut engine = shared.engine();
    if engine.foreground_changed(fg.program()) {
        shared.commit(&mut engine);
    }
}

/// Programs with a visible top-level window and a title, without Trimcraft itself, sorted by
/// executable name.
pub fn windows() -> Vec<ProcessWindow> {
    platform::windows()
}

mod platform {
    use super::ProcessWindow;
    use crate::elevation;
    use crate::foreground::window_title;
    use windows_sys::Win32::Foundation::{HWND, LPARAM};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GW_OWNER, GetWindow, GetWindowThreadProcessId, IsWindowVisible,
    };
    use windows_sys::core::BOOL;

    /// Executable file name of the program owning `hwnd`; `None` for Trimcraft's own windows and
    /// processes that cannot be opened.
    fn exe_of(hwnd: HWND) -> Option<String> {
        let mut pid = 0;
        // SAFETY: `pid` outlives the call; an invalid window gives 0.
        unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
        if pid == 0 || pid == std::process::id() {
            return None;
        }
        elevation::exe_name(pid)
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
        if shown
            && let Some(title) = window_title(hwnd).filter(|t| !t.trim().is_empty())
            && let Some(exe) = exe_of(hwnd)
        {
            found.push(ProcessWindow { exe, title });
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
}

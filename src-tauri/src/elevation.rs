//! Administrator rights: whether Detent or another process runs elevated, relaunching Detent
//! elevated (UAC prompt), and the single-instance handshake across elevation levels.
//!
//! Windows blocks `SendInput` from a normal process into an elevated one (UIPI), so Detent's
//! keys and mouse do not reach a game that runs as administrator unless Detent does too.

use crate::Shared;
use crate::foreground::Foreground;
use detent_core::settings::{AUTOSTART_ARG, ProcessElevation, foreground_warning};
use std::sync::atomic::Ordering;
use std::time::Duration;
use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_ACCESS_DENIED, GetLastError, HANDLE, HWND, WAIT_OBJECT_0,
};
use windows_sys::Win32::Security::{
    GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation,
};
use windows_sys::Win32::System::DataExchange::COPYDATASTRUCT;
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, GetCurrentProcessId, MUTEX_ALL_ACCESS, OpenMutexW, OpenProcess,
    OpenProcessToken, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
    QueryFullProcessImageNameW, SYNCHRONIZATION_SYNCHRONIZE, WaitForSingleObject,
};
use windows_sys::Win32::UI::Shell::ShellExecuteW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AllowSetForegroundWindow, ChangeWindowMessageFilterEx, FindWindowW, GetWindowThreadProcessId,
    MSGFLT_ALLOW, SMTO_NORMAL, SW_SHOWNORMAL, SendMessageTimeoutW, WM_COPYDATA,
};

/// Passed to the elevated relaunch with the old process id: wait for it to exit, and never
/// relaunch again.
pub const RELAUNCH_ARG: &str = "--elevated-relaunch=";
/// Start with the window shown, whatever "Start minimized" says.
pub const SHOW_ARG: &str = "--show";

/// Whether Detent itself runs elevated.
pub fn is_elevated() -> bool {
    // SAFETY: the pseudo handle of the current process needs no closing.
    token_elevation(unsafe { GetCurrentProcess() }) == ProcessElevation::Elevated
}

/// Elevation and executable file name of process `pid`.
pub fn process_info(pid: u32) -> (ProcessElevation, Option<String>) {
    // SAFETY: plain call; the handle is closed below.
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if process.is_null() {
        return (denied_or_unknown(), None);
    }
    let name = image_name(process);
    let elevation = token_elevation(process);
    // SAFETY: opened above.
    unsafe { CloseHandle(process) };
    (elevation, name)
}

/// Executable file name of process `pid`.
pub fn exe_name(pid: u32) -> Option<String> {
    // SAFETY: plain call; the handle is closed below.
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if process.is_null() {
        return None;
    }
    let name = image_name(process);
    // SAFETY: opened above.
    unsafe { CloseHandle(process) };
    name
}

/// A foreground change, for the warning: the foreground program runs as administrator and
/// Detent does not, so Windows drops Detent's keys and mouse (UIPI). Switching to Detent
/// itself keeps the warning so it can be read.
pub fn check_foreground(shared: &Shared, fg: &Foreground) {
    let mut engine = shared.engine();
    let mut status = engine.shell_status().clone();
    let warning = foreground_warning(
        status.elevated,
        fg.is_self,
        fg.elevation,
        &fg.name(),
        status.elevated_foreground.as_deref(),
    );
    if warning == status.elevated_foreground {
        return;
    }
    match &warning {
        Some(name) => log::warn!(
            "{name} runs as administrator; Detent's keys and mouse will not reach it ({:?})",
            fg.elevation
        ),
        None => log::info!("the foreground program no longer runs as administrator"),
    }
    status.elevated_foreground = warning;
    if engine.set_shell_status(status) {
        shared.changed.store(true, Ordering::Relaxed);
    }
}

fn denied_or_unknown() -> ProcessElevation {
    // SAFETY: no preconditions.
    if unsafe { GetLastError() } == ERROR_ACCESS_DENIED {
        ProcessElevation::AccessDenied
    } else {
        ProcessElevation::Unknown
    }
}

fn token_elevation(process: HANDLE) -> ProcessElevation {
    let mut token: HANDLE = std::ptr::null_mut();
    // SAFETY: `process` is a valid process handle; `token` receives a handle closed below.
    if unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) } == 0 {
        return denied_or_unknown();
    }
    let mut elevation = TOKEN_ELEVATION { TokenIsElevated: 0 };
    let mut size = 0u32;
    // SAFETY: the buffer is a TOKEN_ELEVATION of the given size.
    let ok = unsafe {
        GetTokenInformation(
            token,
            TokenElevation,
            (&mut elevation as *mut TOKEN_ELEVATION).cast(),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut size,
        )
    };
    let result = if ok == 0 {
        denied_or_unknown()
    } else if elevation.TokenIsElevated != 0 {
        ProcessElevation::Elevated
    } else {
        ProcessElevation::NotElevated
    };
    // SAFETY: opened above.
    unsafe { CloseHandle(token) };
    result
}

fn image_name(process: HANDLE) -> Option<String> {
    let mut buf = [0u16; 1024];
    let mut len = buf.len() as u32;
    // SAFETY: `buf` holds `len` characters.
    let ok = unsafe {
        QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, buf.as_mut_ptr(), &mut len)
    };
    if ok == 0 {
        return None;
    }
    let path = String::from_utf16_lossy(&buf[..len as usize]);
    path.rsplit('\\').next().map(str::to_owned)
}

/// Starts Detent again with administrator rights (UAC prompt), with the same arguments.
/// `show`: the new instance shows its window even if it would start in the tray (restart
/// from the Settings page). Returns once the user answered the prompt; the caller exits on
/// success.
pub fn relaunch(show: bool) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut args: Vec<String> = std::env::args()
        .skip(1)
        .filter(|a| !a.starts_with(RELAUNCH_ARG) && a != SHOW_ARG)
        .filter(|a| !(show && a == AUTOSTART_ARG))
        .collect();
    if show {
        args.push(SHOW_ARG.into());
    }
    // SAFETY: no preconditions.
    args.push(format!("{RELAUNCH_ARG}{}", unsafe {
        GetCurrentProcessId()
    }));
    let params = args.iter().map(|a| quote(a)).collect::<Vec<_>>().join(" ");
    log::info!("relaunching as administrator: {} {params}", exe.display());
    let verb = wide("runas");
    let file = wide(exe.as_os_str());
    let params = wide(&params);
    // SAFETY: all strings are NUL-terminated and outlive the call.
    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            verb.as_ptr(),
            file.as_ptr(),
            params.as_ptr(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    };
    // Values above 32 mean success; the prompt being declined is an error.
    if result as usize > 32 {
        Ok(())
    } else {
        Err(format!(
            "Windows did not start Detent as administrator (code {}; was the prompt declined?)",
            result as usize
        ))
    }
}

/// The process id of the instance that relaunched this one, from the command line.
pub fn relaunched_from() -> Option<u32> {
    std::env::args().find_map(|a| a.strip_prefix(RELAUNCH_ARG)?.parse().ok())
}

/// Waits until process `pid` has exited (at most `timeout`), so its single-instance lock is
/// gone.
pub fn wait_for_exit(pid: u32, timeout: Duration) {
    // SAFETY: plain call; the handle is closed below.
    let process = unsafe { OpenProcess(SYNCHRONIZATION_SYNCHRONIZE, 0, pid) };
    if process.is_null() {
        return; // already gone
    }
    // SAFETY: valid handle.
    let waited = unsafe { WaitForSingleObject(process, timeout.as_millis() as u32) };
    if waited != WAIT_OBJECT_0 {
        log::warn!("the previous Detent process {pid} did not exit in time");
    }
    // SAFETY: opened above.
    unsafe { CloseHandle(process) };
}

// The single-instance plugin's names and message (tauri-plugin-single-instance, Windows):
// a mutex, and a hidden window that takes WM_COPYDATA "cwd|args".
fn mutex_name(identifier: &str) -> Vec<u16> {
    wide(format!("{identifier}-sim"))
}
fn class_name(identifier: &str) -> Vec<u16> {
    wide(format!("{identifier}-sic"))
}
fn window_name(identifier: &str) -> Vec<u16> {
    wide(format!("{identifier}-siw"))
}
const SINGLE_INSTANCE_DATA: usize = 1542;

/// A non-elevated start while an elevated instance runs: the single-instance plugin cannot
/// open the elevated instance's mutex (access denied) and would start a second instance.
/// Asks the elevated instance to show its window instead. Returns whether one is running, in
/// which case this process must exit.
pub fn forward_to_elevated_instance(identifier: &str) -> bool {
    let name = mutex_name(identifier);
    // The access the plugin asks for when it opens the mutex (CreateMutexW).
    // SAFETY: NUL-terminated name; the handle is closed right away.
    let mutex = unsafe { OpenMutexW(MUTEX_ALL_ACCESS, 0, name.as_ptr()) };
    if !mutex.is_null() {
        // SAFETY: opened above.
        unsafe { CloseHandle(mutex) };
        return false; // accessible: the plugin handles it
    }
    // SAFETY: no preconditions.
    if unsafe { GetLastError() } != ERROR_ACCESS_DENIED {
        return false; // no instance
    }
    let (class, window) = (class_name(identifier), window_name(identifier));
    // SAFETY: NUL-terminated names.
    let hwnd = unsafe { FindWindowW(class.as_ptr(), window.as_ptr()) };
    if !hwnd.is_null() {
        let mut pid = 0;
        let data = b"|\0";
        let cds = COPYDATASTRUCT {
            dwData: SINGLE_INSTANCE_DATA,
            cbData: data.len() as u32,
            lpData: data.as_ptr() as *mut _,
        };
        // SAFETY: `hwnd` came from FindWindowW; `cds` and its data outlive the call.
        unsafe {
            GetWindowThreadProcessId(hwnd, &mut pid);
            AllowSetForegroundWindow(pid);
            SendMessageTimeoutW(
                hwnd,
                WM_COPYDATA,
                0,
                &cds as *const COPYDATASTRUCT as isize,
                SMTO_NORMAL,
                5_000,
                std::ptr::null_mut(),
            );
        }
    }
    true
}

/// In an elevated instance: lets non-elevated processes send the single-instance message, so
/// a second, normal start shows this instance's window (UIPI would drop it otherwise).
pub fn allow_forwarding(identifier: &str) {
    let (class, window) = (class_name(identifier), window_name(identifier));
    // SAFETY: NUL-terminated names.
    let hwnd: HWND = unsafe { FindWindowW(class.as_ptr(), window.as_ptr()) };
    if hwnd.is_null() {
        return log::warn!("single-instance window not found");
    }
    let mut pid = 0;
    // SAFETY: valid window handle.
    unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
    // SAFETY: no preconditions.
    if pid != unsafe { GetCurrentProcessId() } {
        return;
    }
    // SAFETY: our own window; a null filter struct is allowed.
    if unsafe { ChangeWindowMessageFilterEx(hwnd, WM_COPYDATA, MSGFLT_ALLOW, std::ptr::null_mut()) }
        == 0
    {
        log::warn!("could not allow the single-instance message from normal processes");
    }
}

fn quote(arg: &str) -> String {
    if !arg.is_empty() && !arg.contains([' ', '\t', '"']) {
        return arg.to_owned();
    }
    format!("\"{}\"", arg.replace('"', "\\\""))
}

pub fn wide(s: impl AsRef<std::ffi::OsStr>) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    s.as_ref().encode_wide().chain(Some(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arguments_are_quoted_when_needed() {
        assert_eq!(quote("--autostart"), "--autostart");
        assert_eq!(quote("a b"), "\"a b\"");
        assert_eq!(quote(""), "\"\"");
    }

    #[test]
    fn own_process_is_found() {
        // SAFETY: no preconditions.
        let (elevation, name) = process_info(unsafe { GetCurrentProcessId() });
        assert_eq!(elevation == ProcessElevation::Elevated, is_elevated());
        assert!(name.is_some_and(|n| n.ends_with(".exe")));
    }
}

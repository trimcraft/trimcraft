//! "Launch" actions: start a program, open a web page or a file or folder.
//!
//! Programs are started with `CreateProcess` (through `std::process::Command`): the executable
//! path and the argument string are passed as they are, nothing goes through a shell. URLs
//! (http and https only, checked when the action is saved) and files are opened with
//! `ShellExecuteW`, like a double click.
//!
//! A program started from an elevated Trimcraft runs elevated too.

use crate::elevation::wide;
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::Command;
use trimcraft_core::launch::{LaunchTarget, exe_name};
use windows_sys::Win32::Foundation::CloseHandle;
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::UI::Shell::ShellExecuteW;
use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

/// Starts or opens `target`. With `wait`, a started program is waited for until it exits.
/// Returns what happened, e.g. "Started (process 4242)".
pub fn run(target: &LaunchTarget, wait: bool) -> Result<String, String> {
    match target {
        LaunchTarget::Program {
            path,
            arguments,
            working_dir,
            only_if_not_running,
        } => {
            let exe = exe_name(path);
            if *only_if_not_running && running(exe) {
                return Ok(format!("Not started, {exe} is already running"));
            }
            let dir = if working_dir.is_empty() {
                Path::new(path).parent().map(Path::to_path_buf)
            } else {
                Some(working_dir.into())
            };
            let mut command = Command::new(path);
            if !arguments.trim().is_empty() {
                command.raw_arg(arguments);
            }
            if let Some(dir) = dir.filter(|d| !d.as_os_str().is_empty()) {
                command.current_dir(dir);
            }
            let mut child = command
                .spawn()
                .map_err(|e| format!("cannot start {path}: {e}"))?;
            let pid = child.id();
            if !wait {
                return Ok(format!("Started (process {pid})"));
            }
            let status = child.wait().map_err(|e| e.to_string())?;
            Ok(match status.code() {
                Some(code) => format!("{exe} exited with code {code}"),
                None => format!("{exe} exited"),
            })
        }
        LaunchTarget::Url { url } => shell_open(url).map(|()| "Opened".into()),
        LaunchTarget::Open { path } => shell_open(path).map(|()| "Opened".into()),
    }
}

fn shell_open(target: &str) -> Result<(), String> {
    let (verb, file) = (wide("open"), wide(target));
    // SAFETY: NUL-terminated strings that outlive the call.
    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            verb.as_ptr(),
            file.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    };
    // Values above 32 mean success.
    if result as usize > 32 {
        Ok(())
    } else {
        Err(format!(
            "Windows cannot open {target} (error {})",
            result as usize
        ))
    }
}

/// Whether a process with executable name `exe` runs (any user, case-insensitive).
pub fn running(exe: &str) -> bool {
    // SAFETY: a snapshot handle, closed below; the entry struct has its size set.
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot.is_null() || snapshot as isize == -1 {
            return false;
        }
        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut found = false;
        let mut ok = Process32FirstW(snapshot, &mut entry);
        while ok != 0 {
            let len = entry
                .szExeFile
                .iter()
                .position(|&c| c == 0)
                .unwrap_or(entry.szExeFile.len());
            if String::from_utf16_lossy(&entry.szExeFile[..len]).eq_ignore_ascii_case(exe) {
                found = true;
                break;
            }
            ok = Process32NextW(snapshot, &mut entry);
        }
        CloseHandle(snapshot);
        found
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_its_own_process() {
        let me = std::env::current_exe().unwrap();
        let name = me.file_name().unwrap().to_string_lossy().into_owned();
        assert!(running(&name));
        assert!(running(&name.to_uppercase()));
        assert!(!running("no-such-program-trimcraft.exe"));
    }

    #[test]
    fn starts_a_program_without_a_shell_and_waits() {
        // cmd's own parsing is not involved: the argument string reaches the program as is.
        let comspec =
            std::env::var("ComSpec").unwrap_or_else(|_| r"C:\Windows\System32\cmd.exe".into());
        let target = LaunchTarget::Program {
            path: comspec,
            arguments: "/c exit 3".into(),
            working_dir: String::new(),
            only_if_not_running: false,
        };
        assert_eq!(run(&target, true).unwrap(), "cmd.exe exited with code 3");
        let missing = LaunchTarget::Program {
            path: r"C:\no\such\program.exe".into(),
            arguments: String::new(),
            working_dir: String::new(),
            only_if_not_running: false,
        };
        assert!(
            run(&missing, false)
                .unwrap_err()
                .starts_with("cannot start")
        );
    }
}

//! "Start with Windows": a per-user `Run` registry value, or, with "Always run as
//! administrator", a Task Scheduler task at logon with highest privileges (no UAC prompt).
//! Which one is registered, and what happens to the other, is decided in
//! `detent_core::settings::plan_autostart`; this module only reads and writes them.
//!
//! The uninstaller removes both (src-tauri/windows/hooks.nsh).

use crate::elevation::wide;
use detent_core::AppSettings;
use detent_core::settings::{
    AUTOSTART_ARG, AutostartKind, AutostartOp, AutostartPresent, plan_autostart,
};
use std::os::windows::process::CommandExt;
use std::process::Command;
use std::sync::Mutex;
use windows_sys::Win32::Foundation::ERROR_SUCCESS;
use windows_sys::Win32::System::Registry::{
    HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ, RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW,
};

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
/// Name of the `Run` value; the uninstaller uses it too.
const RUN_VALUE: &str = "Detent";
/// schtasks.exe without a console window flashing up.
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Makes the registered autostart match `settings`. Returns what is registered afterwards and
/// a note for the Settings page when that differs from the settings or something failed.
pub fn sync(settings: &AppSettings, elevated: bool) -> (AutostartKind, Option<String>) {
    // Settings can change while a sync runs; one at a time.
    static LOCK: Mutex<()> = Mutex::new(());
    let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let before = present();
    let plan = plan_autostart(settings, before, elevated);
    let mut note = plan.note;
    for op in plan.ops {
        if let Err(e) = apply(op) {
            log::error!("autostart: {op:?} failed: {e}");
            note = Some(format!("Changing autostart failed: {e}"));
        }
    }
    let after = present();
    if after != before {
        log::info!(
            "autostart: {:?} -> {:?} (run key {}, task {})",
            before.kind(),
            after.kind(),
            after.run_key,
            after.task
        );
    }
    (after.kind(), note)
}

pub fn present() -> AutostartPresent {
    AutostartPresent {
        run_key: run_key_present(),
        task: task_present(),
    }
}

fn apply(op: AutostartOp) -> Result<(), String> {
    match op {
        AutostartOp::WriteRunKey => write_run_key(),
        AutostartOp::RemoveRunKey => remove_run_key(),
        AutostartOp::CreateTask => create_task(),
        AutostartOp::RemoveTask => schtasks(&["/Delete", "/TN", &task_name(), "/F"]),
    }
}

/// The command line Windows runs at logon.
fn exe() -> Result<String, String> {
    std::env::current_exe()
        .map(|p| p.display().to_string())
        .map_err(|e| format!("cannot find Detent's executable: {e}"))
}

fn run_key_present() -> bool {
    let (key, value) = (wide(RUN_KEY), wide(RUN_VALUE));
    // SAFETY: NUL-terminated strings; null buffers only check the value exists.
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            value.as_ptr(),
            RRF_RT_REG_SZ,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    status == ERROR_SUCCESS
}

fn write_run_key() -> Result<(), String> {
    let command = wide(format!("\"{}\" {AUTOSTART_ARG}", exe()?));
    let (key, value) = (wide(RUN_KEY), wide(RUN_VALUE));
    // SAFETY: NUL-terminated strings; the data size includes the terminating NUL.
    let status = unsafe {
        RegSetKeyValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            value.as_ptr(),
            REG_SZ,
            command.as_ptr().cast(),
            (command.len() * 2) as u32,
        )
    };
    if status == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(format!(
            "cannot write the Run registry value (error {status})"
        ))
    }
}

fn remove_run_key() -> Result<(), String> {
    let (key, value) = (wide(RUN_KEY), wide(RUN_VALUE));
    // SAFETY: NUL-terminated strings.
    let status = unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), value.as_ptr()) };
    if status == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(format!(
            "cannot remove the Run registry value (error {status})"
        ))
    }
}

/// Task names are machine-wide; one task per Windows user. The uninstaller builds the same
/// name from %USERNAME%.
fn task_name() -> String {
    let user = std::env::var("USERNAME").unwrap_or_default();
    format!("Detent autostart {user}")
}

fn task_present() -> bool {
    schtasks(&["/Query", "/TN", &task_name()]).is_ok()
}

fn create_task() -> Result<(), String> {
    let user = match (std::env::var("USERDOMAIN"), std::env::var("USERNAME")) {
        (Ok(domain), Ok(name)) => format!("{domain}\\{name}"),
        (_, Ok(name)) => name,
        _ => return Err("cannot find the Windows user name".into()),
    };
    let xml = task_xml(&exe()?, &user);
    // schtasks reads UTF-16 with a byte order mark.
    let mut bytes = vec![0xFF, 0xFE];
    bytes.extend(xml.encode_utf16().flat_map(u16::to_le_bytes));
    let path = std::env::temp_dir().join(format!("detent-autostart-{}.xml", std::process::id()));
    std::fs::write(&path, bytes).map_err(|e| format!("cannot write the task file: {e}"))?;
    let result = schtasks(&[
        "/Create",
        "/TN",
        &task_name(),
        "/XML",
        &path.display().to_string(),
        "/F",
    ]);
    let _ = std::fs::remove_file(&path);
    result
}

/// At logon of `user`, elevated ("highest privileges"), normal priority (tasks default to
/// below normal), no time limit, also on battery.
fn task_xml(exe: &str, user: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo>
    <Description>Starts Detent as administrator when you log on (Detent: Settings, Start with Windows).</Description>
  </RegistrationInfo>
  <Triggers>
    <LogonTrigger>
      <Enabled>true</Enabled>
      <UserId>{user}</UserId>
    </LogonTrigger>
  </Triggers>
  <Principals>
    <Principal id="Author">
      <UserId>{user}</UserId>
      <LogonType>InteractiveToken</LogonType>
      <RunLevel>HighestAvailable</RunLevel>
    </Principal>
  </Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
    <Priority>4</Priority>
  </Settings>
  <Actions Context="Author">
    <Exec>
      <Command>{exe}</Command>
      <Arguments>{AUTOSTART_ARG}</Arguments>
    </Exec>
  </Actions>
</Task>
"#,
        user = xml_escape(user),
        exe = xml_escape(exe),
    )
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn schtasks(args: &[&str]) -> Result<(), String> {
    let output = Command::new("schtasks.exe")
        .args(args)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("cannot run schtasks: {e}"))?;
    if output.status.success() {
        Ok(())
    } else {
        let err = String::from_utf8_lossy(&output.stderr);
        Err(format!("schtasks {}: {}", args[0], err.trim()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_definition_escapes_and_starts_elevated() {
        let xml = task_xml(r"C:\Apps & Tools\detent.exe", r"PC\sam");
        assert!(xml.contains(r"<Command>C:\Apps &amp; Tools\detent.exe</Command>"));
        assert!(xml.contains(r"<UserId>PC\sam</UserId>"));
        assert!(xml.contains("<RunLevel>HighestAvailable</RunLevel>"));
        assert!(xml.contains("<Arguments>--autostart</Arguments>"));
    }

    /// Writes, reads back and removes the Run value of the current user. Leaves nothing
    /// behind, but restores no value that was there before, so it only runs on request.
    #[test]
    #[ignore]
    fn run_key_round_trip() {
        assert!(
            !run_key_present(),
            "a Detent autostart is registered already"
        );
        write_run_key().unwrap();
        assert!(run_key_present());
        remove_run_key().unwrap();
        assert!(!run_key_present());
    }
}

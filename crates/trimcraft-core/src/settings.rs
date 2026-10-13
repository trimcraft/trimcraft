//! App-wide settings (Settings page, "General") and the decisions the app shell takes from
//! them: how to start, which autostart mechanism to register, when to warn about an elevated
//! foreground program.
//!
//! The settings are global, not part of a game profile. The OS side (registry, Task
//! Scheduler, tokens, tray) lives in the Tauri crate; everything here is pure.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AppSettings {
    /// The window's close button hides the window; Trimcraft keeps running in the tray.
    pub close_to_tray: bool,
    /// Start Trimcraft when the user logs on to Windows.
    pub start_with_windows: bool,
    /// Start with the window hidden in the tray, also when started by hand. A start by
    /// Windows always goes to the tray.
    pub start_minimized: bool,
    /// Relaunch elevated (UAC prompt) when started without administrator rights, so input
    /// reaches games that run as administrator.
    pub run_as_admin: bool,
    /// The one-time "Trimcraft keeps running in the tray" notification was shown. Set by the
    /// app, not by the Settings page.
    pub tray_notice_shown: bool,
    /// Global hotkey that stops all macros and releases every input they hold. Missing in
    /// files from before it.
    pub panic_hotkey: String,
    /// Look for a new release when Trimcraft starts. Missing in files from before updates.
    pub check_updates_on_startup: bool,
}

/// Default panic hotkey.
pub const DEFAULT_PANIC_HOTKEY: &str = "Ctrl+Shift+F12";

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            close_to_tray: true,
            start_with_windows: false,
            start_minimized: true,
            run_as_admin: false,
            tray_notice_shown: false,
            panic_hotkey: DEFAULT_PANIC_HOTKEY.into(),
            check_updates_on_startup: true,
        }
    }
}

/// Command line argument the autostart entry passes, so Trimcraft knows Windows started it.
pub const AUTOSTART_ARG: &str = "--autostart";

/// How Trimcraft gets started at logon.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AutostartKind {
    #[default]
    Off,
    /// Per-user `Run` key in the registry: starts without administrator rights.
    RunKey,
    /// Task Scheduler task at logon with "Run with highest privileges": starts elevated
    /// without a UAC prompt.
    Task,
}

/// The autostart entries currently registered.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutostartPresent {
    pub run_key: bool,
    pub task: bool,
}

impl AutostartPresent {
    /// What Windows will do at logon. With both present the task wins: it starts elevated
    /// and the second, non-elevated start ends as a second instance.
    pub fn kind(self) -> AutostartKind {
        if self.task {
            AutostartKind::Task
        } else if self.run_key {
            AutostartKind::RunKey
        } else {
            AutostartKind::Off
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutostartOp {
    /// Writes (or rewrites, e.g. after the exe moved) the `Run` value.
    WriteRunKey,
    RemoveRunKey,
    /// Creates or replaces the task. Needs administrator rights.
    CreateTask,
    /// Needs administrator rights.
    RemoveTask,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutostartPlan {
    /// In this order.
    pub ops: Vec<AutostartOp>,
    /// Why the result differs from the settings, for the Settings page.
    pub note: Option<String>,
}

/// The mechanism the settings ask for.
pub fn wanted_autostart(settings: &AppSettings) -> AutostartKind {
    match (settings.start_with_windows, settings.run_as_admin) {
        (false, _) => AutostartKind::Off,
        (true, false) => AutostartKind::RunKey,
        (true, true) => AutostartKind::Task,
    }
}

/// What to change so the registered autostart matches the settings, given what is registered
/// now and whether Trimcraft runs elevated. Only one mechanism stays registered.
///
/// The task can only be created or removed with administrator rights. Without them an
/// elevated autostart falls back to the `Run` key (Trimcraft then relaunches itself elevated with
/// a UAC prompt at logon) until Trimcraft runs elevated, and a task that should go stays.
pub fn plan_autostart(
    settings: &AppSettings,
    present: AutostartPresent,
    elevated: bool,
) -> AutostartPlan {
    use AutostartOp::*;
    let mut ops = Vec::new();
    let mut note = None;
    let stale_task = |ops: &mut Vec<AutostartOp>, note: &mut Option<String>| {
        if present.task {
            if elevated {
                ops.push(RemoveTask);
            } else {
                *note = Some(
                    "The elevated autostart task can only be removed while Trimcraft runs as \
                     administrator; it is removed at the next start as administrator."
                        .into(),
                );
            }
        }
    };
    match wanted_autostart(settings) {
        AutostartKind::Off => {
            if present.run_key {
                ops.push(RemoveRunKey);
            }
            stale_task(&mut ops, &mut note);
        }
        AutostartKind::RunKey => {
            ops.push(WriteRunKey);
            stale_task(&mut ops, &mut note);
        }
        AutostartKind::Task if elevated => {
            ops.push(CreateTask);
            if present.run_key {
                ops.push(RemoveRunKey);
            }
        }
        AutostartKind::Task => {
            if present.task {
                // Registered by an earlier elevated run; it cannot be refreshed now, but works.
                if present.run_key {
                    ops.push(RemoveRunKey);
                }
            } else {
                ops.push(WriteRunKey);
                note = Some(
                    "Registered without administrator rights for now (UAC prompt at logon); \
                     Trimcraft switches to a Task Scheduler task at the next start as \
                     administrator."
                        .into(),
                );
            }
        }
    }
    AutostartPlan { ops, note }
}

/// Whether the main window starts hidden in the tray. A start by Windows always does; a start
/// by hand does when "Start minimized" is on, except on the very first run (no setup saved
/// yet), so a fresh install shows its window.
pub fn start_hidden(settings: &AppSettings, autostarted: bool, first_run: bool) -> bool {
    autostarted || (settings.start_minimized && !first_run)
}

/// Whether Trimcraft should relaunch itself elevated at startup. `attempted`: this process is
/// already the result of such a relaunch (never loop, e.g. when UAC is turned off).
pub fn relaunch_elevated(settings: &AppSettings, elevated: bool, attempted: bool) -> bool {
    settings.run_as_admin && !elevated && !attempted
}

/// What Trimcraft found out about the elevation of the foreground program.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessElevation {
    Elevated,
    NotElevated,
    /// Its token could not be read. Windows denies that for elevated processes, so this most
    /// likely means elevated.
    AccessDenied,
    /// Any other failure (e.g. the process already ended).
    Unknown,
}

/// The foreground warning after the foreground changed to a program called `name`.
/// `current` is the warning shown so far. Switching to Trimcraft itself keeps it, so the user can
/// read it.
pub fn foreground_warning(
    self_elevated: bool,
    is_self: bool,
    elevation: ProcessElevation,
    name: &str,
    current: Option<&str>,
) -> Option<String> {
    if self_elevated {
        return None;
    }
    if is_self {
        return current.map(str::to_owned);
    }
    match elevation {
        ProcessElevation::Elevated | ProcessElevation::AccessDenied => Some(name.to_owned()),
        ProcessElevation::NotElevated | ProcessElevation::Unknown => None,
    }
}

/// Live facts about the app shell for the UI. Not saved.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShellStatus {
    /// Trimcraft runs as administrator.
    pub elevated: bool,
    /// What is registered for autostart right now.
    pub autostart: AutostartKind,
    /// Why autostart differs from the settings, or why changing it failed.
    pub autostart_note: Option<String>,
    /// The foreground program runs as administrator and Trimcraft does not: its name.
    pub elevated_foreground: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use AutostartOp::*;

    fn settings(start_with_windows: bool, run_as_admin: bool) -> AppSettings {
        AppSettings {
            start_with_windows,
            run_as_admin,
            ..AppSettings::default()
        }
    }

    const NONE: AutostartPresent = AutostartPresent {
        run_key: false,
        task: false,
    };
    const RUN_KEY: AutostartPresent = AutostartPresent {
        run_key: true,
        task: false,
    };
    const TASK: AutostartPresent = AutostartPresent {
        run_key: false,
        task: true,
    };
    const BOTH: AutostartPresent = AutostartPresent {
        run_key: true,
        task: true,
    };

    #[test]
    fn defaults() {
        let s = AppSettings::default();
        assert!(s.close_to_tray);
        assert!(!s.start_with_windows);
        assert!(s.start_minimized);
        assert!(!s.run_as_admin);
        assert!(!s.tray_notice_shown);
        assert_eq!(s.panic_hotkey, "Ctrl+Shift+F12");
        assert!(s.check_updates_on_startup);
    }

    #[test]
    fn serialises_camel_case_and_fills_missing_fields() {
        let json = serde_json::to_value(AppSettings::default()).unwrap();
        assert_eq!(json["closeToTray"], true);
        assert_eq!(json["startWithWindows"], false);
        assert_eq!(json["startMinimized"], true);
        assert_eq!(json["runAsAdmin"], false);
        assert_eq!(json["trayNoticeShown"], false);
        let s: AppSettings = serde_json::from_str(r#"{"runAsAdmin":true}"#).unwrap();
        assert_eq!(
            s,
            AppSettings {
                run_as_admin: true,
                ..AppSettings::default()
            }
        );
    }

    #[test]
    fn wanted_mechanism() {
        assert_eq!(
            wanted_autostart(&settings(false, false)),
            AutostartKind::Off
        );
        assert_eq!(wanted_autostart(&settings(false, true)), AutostartKind::Off);
        assert_eq!(
            wanted_autostart(&settings(true, false)),
            AutostartKind::RunKey
        );
        assert_eq!(wanted_autostart(&settings(true, true)), AutostartKind::Task);
    }

    #[test]
    fn off_removes_everything_it_may() {
        let off = settings(false, false);
        assert_eq!(plan_autostart(&off, NONE, false).ops, vec![]);
        assert_eq!(plan_autostart(&off, RUN_KEY, false).ops, vec![RemoveRunKey]);
        assert_eq!(
            plan_autostart(&off, BOTH, true).ops,
            vec![RemoveRunKey, RemoveTask]
        );
        // The task needs administrator rights to go.
        let plan = plan_autostart(&off, BOTH, false);
        assert_eq!(plan.ops, vec![RemoveRunKey]);
        assert!(plan.note.is_some());
    }

    #[test]
    fn run_key_is_written_and_the_task_removed() {
        let s = settings(true, false);
        let plan = plan_autostart(&s, NONE, false);
        assert_eq!(plan.ops, vec![WriteRunKey]);
        assert_eq!(plan.note, None);
        // Rewritten every time, so a moved exe is picked up.
        assert_eq!(plan_autostart(&s, RUN_KEY, false).ops, vec![WriteRunKey]);
        assert_eq!(
            plan_autostart(&s, TASK, true).ops,
            vec![WriteRunKey, RemoveTask]
        );
        let plan = plan_autostart(&s, TASK, false);
        assert_eq!(plan.ops, vec![WriteRunKey]);
        assert!(plan.note.is_some());
    }

    #[test]
    fn elevated_autostart_uses_the_task_when_elevated() {
        let s = settings(true, true);
        let plan = plan_autostart(&s, RUN_KEY, true);
        assert_eq!(plan.ops, vec![CreateTask, RemoveRunKey]);
        assert_eq!(plan.note, None);
        assert_eq!(plan_autostart(&s, TASK, true).ops, vec![CreateTask]);
        assert_eq!(plan_autostart(&s, NONE, true).ops, vec![CreateTask]);
    }

    #[test]
    fn elevated_autostart_without_rights_falls_back_or_keeps_the_task() {
        let s = settings(true, true);
        let plan = plan_autostart(&s, NONE, false);
        assert_eq!(plan.ops, vec![WriteRunKey]);
        assert!(plan.note.is_some());
        let plan = plan_autostart(&s, TASK, false);
        assert_eq!(plan.ops, vec![]);
        assert_eq!(plan.note, None);
        assert_eq!(plan_autostart(&s, BOTH, false).ops, vec![RemoveRunKey]);
    }

    #[test]
    fn present_kind() {
        assert_eq!(NONE.kind(), AutostartKind::Off);
        assert_eq!(RUN_KEY.kind(), AutostartKind::RunKey);
        assert_eq!(TASK.kind(), AutostartKind::Task);
        assert_eq!(BOTH.kind(), AutostartKind::Task);
    }

    #[test]
    fn hidden_start() {
        let on = AppSettings::default();
        let off = AppSettings {
            start_minimized: false,
            ..AppSettings::default()
        };
        assert!(start_hidden(&on, true, false));
        assert!(start_hidden(&off, true, false));
        assert!(start_hidden(&on, true, true));
        assert!(start_hidden(&on, false, false));
        assert!(!start_hidden(&off, false, false));
        // A fresh install shows its window.
        assert!(!start_hidden(&on, false, true));
    }

    #[test]
    fn relaunch_only_once_and_only_when_asked() {
        let admin = settings(false, true);
        assert!(relaunch_elevated(&admin, false, false));
        assert!(!relaunch_elevated(&admin, true, false));
        assert!(!relaunch_elevated(&admin, false, true));
        assert!(!relaunch_elevated(&settings(false, false), false, false));
    }

    #[test]
    fn foreground_warning_rules() {
        use ProcessElevation::*;
        let warn =
            |elevation, current| foreground_warning(false, false, elevation, "game.exe", current);
        assert_eq!(warn(Elevated, None).as_deref(), Some("game.exe"));
        assert_eq!(warn(AccessDenied, None).as_deref(), Some("game.exe"));
        assert_eq!(warn(NotElevated, Some("game.exe")), None);
        assert_eq!(warn(Unknown, Some("game.exe")), None);
        // Switching to Trimcraft keeps the warning so it can be read.
        assert_eq!(
            foreground_warning(false, true, NotElevated, "trimcraft.exe", Some("game.exe"))
                .as_deref(),
            Some("game.exe")
        );
        assert_eq!(
            foreground_warning(false, true, NotElevated, "trimcraft.exe", None),
            None
        );
        // An elevated Trimcraft reaches everything.
        assert_eq!(
            foreground_warning(true, false, Elevated, "game.exe", None),
            None
        );
    }
}

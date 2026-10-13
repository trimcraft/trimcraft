//! Updates: what the Settings page ("Updates") and the header badge show about new releases.
//!
//! The app checks the release feed (the updater plugin: `latest.json` of the latest GitHub
//! release, signed) and reports here; installing runs the signed NSIS installer per user.

use crate::diagnostics::Level;
use serde::Serialize;

/// Release notes are shown up to this many characters.
pub const NOTES_EXCERPT_CHARS: usize = 600;

/// Where the update check stands.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "state"
)]
pub enum UpdateState {
    /// Not checked yet.
    #[default]
    Idle,
    Checking,
    /// Nothing newer. `no_releases`: the feed has no release yet (before the first one).
    UpToDate {
        no_releases: bool,
    },
    Available {
        version: String,
        /// Release notes, shortened.
        notes: String,
        /// RFC 3339, if the feed gives one.
        date: Option<String>,
    },
    Downloading {
        version: String,
        /// 0..=100, if the size is known.
        percent: Option<u8>,
    },
    /// The installer runs; Trimcraft quits and restarts.
    Installing {
        version: String,
    },
    Failed {
        message: String,
    },
}

impl UpdateState {
    /// The version the header badge points to.
    pub fn available_version(&self) -> Option<&str> {
        match self {
            UpdateState::Available { version, .. } => Some(version),
            _ => None,
        }
    }
}

/// What started a check; the event log says so.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckOrigin {
    /// Shortly after Trimcraft starts (the automatic check is on).
    Startup,
    /// Repeated while Trimcraft runs (the automatic check is on).
    Scheduled,
    /// "Check now".
    Manual,
}

impl CheckOrigin {
    /// Startup and scheduled checks run on their own; "Check now" was asked for.
    pub fn is_automatic(self) -> bool {
        !matches!(self, CheckOrigin::Manual)
    }
}

/// Whether an automatic check should run now. It never replaces a found update (the badge
/// would disappear while it runs) and never interrupts a check, download or install.
pub fn automatic_check_due(enabled: bool, state: &UpdateState) -> bool {
    enabled
        && matches!(
            state,
            UpdateState::Idle | UpdateState::UpToDate { .. } | UpdateState::Failed { .. }
        )
}

/// The version to announce once after an automatic check found it (a notification while the
/// main window is hidden; the header badge shows it otherwise). `announced` is the version
/// announced last in this run, so each version is announced at most once.
pub fn announcement<'a>(
    origin: CheckOrigin,
    state: &'a UpdateState,
    announced: Option<&str>,
) -> Option<&'a str> {
    let version = state.available_version()?;
    (origin.is_automatic() && announced != Some(version)).then_some(version)
}

/// The event log entry for a finished check. Every outcome is logged, so a startup check that
/// found nothing can be told apart from one that never ran. `current` is the installed version.
pub fn check_log(
    origin: CheckOrigin,
    current: &str,
    state: &UpdateState,
) -> Option<(Level, String)> {
    let check = match origin {
        CheckOrigin::Startup => "Startup update check",
        CheckOrigin::Scheduled => "Scheduled update check",
        CheckOrigin::Manual => "Update check",
    };
    match state {
        UpdateState::Available { version, .. } => Some((
            Level::Info,
            format!("{check}: Trimcraft {version} is available (installed: {current})"),
        )),
        UpdateState::UpToDate { no_releases: false } => Some((
            Level::Info,
            format!("{check}: Trimcraft {current} is up to date"),
        )),
        UpdateState::UpToDate { no_releases: true } => {
            Some((Level::Info, format!("{check}: no release published yet")))
        }
        UpdateState::Failed { message } => Some((Level::Warn, format!("{check}: {message}"))),
        UpdateState::Idle
        | UpdateState::Checking
        | UpdateState::Downloading { .. }
        | UpdateState::Installing { .. } => None,
    }
}

/// The start of release notes: at most [`NOTES_EXCERPT_CHARS`], cut at a line end where
/// possible, with "…" when shortened.
pub fn notes_excerpt(notes: &str) -> String {
    let notes = notes.trim();
    if notes.chars().count() <= NOTES_EXCERPT_CHARS {
        return notes.to_owned();
    }
    let cut: String = notes.chars().take(NOTES_EXCERPT_CHARS).collect();
    let cut = match cut.rfind('\n') {
        Some(i) if i > NOTES_EXCERPT_CHARS / 2 => &cut[..i],
        _ => cut.as_str(),
    };
    format!("{}\n…", cut.trim_end())
}

/// Download progress in percent, if the size is known.
pub fn percent(done: u64, total: Option<u64>) -> Option<u8> {
    let total = total.filter(|&t| t > 0)?;
    Some(((done.min(total) * 100) / total) as u8)
}

/// What is running that an update would interrupt (it quits Trimcraft).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateBusy {
    /// Macros running now.
    pub macros: usize,
    /// vJoy devices are driven by Trimcraft.
    pub vjoy: bool,
}

impl UpdateBusy {
    /// Whether to ask before installing.
    pub fn needs_confirmation(self) -> bool {
        self.macros > 0 || self.vjoy
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn states_serialise_for_the_ui() {
        let json = serde_json::to_value(UpdateState::UpToDate { no_releases: true }).unwrap();
        assert_eq!(json["state"], "upToDate");
        assert_eq!(json["noReleases"], true);
        let available = UpdateState::Available {
            version: "0.2.0".into(),
            notes: "Fixes".into(),
            date: None,
        };
        assert_eq!(available.available_version(), Some("0.2.0"));
        let json = serde_json::to_value(&available).unwrap();
        assert_eq!(json["state"], "available");
        assert_eq!(json["version"], "0.2.0");
        let json = serde_json::to_value(UpdateState::Downloading {
            version: "0.2.0".into(),
            percent: Some(40),
        })
        .unwrap();
        assert_eq!(json["percent"], 40);
        assert_eq!(UpdateState::default(), UpdateState::Idle);
        assert_eq!(UpdateState::Checking.available_version(), None);
    }

    #[test]
    fn every_check_outcome_is_logged() {
        let available = UpdateState::Available {
            version: "0.1.2".into(),
            notes: String::new(),
            date: None,
        };
        assert_eq!(
            check_log(CheckOrigin::Startup, "0.1.1", &available),
            Some((
                Level::Info,
                "Startup update check: Trimcraft 0.1.2 is available (installed: 0.1.1)".into()
            ))
        );
        let up_to_date = UpdateState::UpToDate { no_releases: false };
        assert_eq!(
            check_log(CheckOrigin::Manual, "0.1.1", &up_to_date),
            Some((
                Level::Info,
                "Update check: Trimcraft 0.1.1 is up to date".into()
            ))
        );
        let none = UpdateState::UpToDate { no_releases: true };
        let (level, text) = check_log(CheckOrigin::Startup, "0.1.1", &none).unwrap();
        assert_eq!(level, Level::Info);
        assert!(text.contains("no release"), "{text}");
        let failed = UpdateState::Failed {
            message: "timed out".into(),
        };
        assert_eq!(
            check_log(CheckOrigin::Startup, "0.1.1", &failed),
            Some((Level::Warn, "Startup update check: timed out".into()))
        );
        // Intermediate states are no outcome.
        let checking = UpdateState::Checking;
        assert_eq!(check_log(CheckOrigin::Startup, "0.1.1", &checking), None);
        let (_, text) = check_log(CheckOrigin::Scheduled, "0.1.1", &up_to_date).unwrap();
        assert!(text.starts_with("Scheduled update check"), "{text}");
    }

    #[test]
    fn automatic_checks_keep_a_found_update() {
        let available = UpdateState::Available {
            version: "0.1.2".into(),
            notes: String::new(),
            date: None,
        };
        // Due when nothing was found yet, also after "up to date" or a failure: a release
        // published after the startup check is found by a later one.
        assert!(automatic_check_due(true, &UpdateState::Idle));
        assert!(automatic_check_due(
            true,
            &UpdateState::UpToDate { no_releases: false }
        ));
        assert!(automatic_check_due(
            true,
            &UpdateState::UpToDate { no_releases: true }
        ));
        assert!(automatic_check_due(
            true,
            &UpdateState::Failed {
                message: "offline".into()
            }
        ));
        // Never when switched off, and never over a found update or a running check/install.
        assert!(!automatic_check_due(false, &UpdateState::Idle));
        assert!(!automatic_check_due(true, &available));
        assert!(!automatic_check_due(true, &UpdateState::Checking));
        assert!(!automatic_check_due(
            true,
            &UpdateState::Downloading {
                version: "0.1.2".into(),
                percent: None
            }
        ));
        assert!(!automatic_check_due(
            true,
            &UpdateState::Installing {
                version: "0.1.2".into()
            }
        ));
    }

    #[test]
    fn a_found_update_is_announced_once() {
        let available = |v: &str| UpdateState::Available {
            version: v.into(),
            notes: String::new(),
            date: None,
        };
        let found = available("0.1.2");
        assert_eq!(
            announcement(CheckOrigin::Startup, &found, None),
            Some("0.1.2")
        );
        assert_eq!(
            announcement(CheckOrigin::Scheduled, &found, None),
            Some("0.1.2")
        );
        assert_eq!(
            announcement(CheckOrigin::Startup, &found, Some("0.1.2")),
            None
        );
        // A newer one is announced again.
        assert_eq!(
            announcement(CheckOrigin::Scheduled, &available("0.1.3"), Some("0.1.2")),
            Some("0.1.3")
        );
        // "Check now" shows the result on the page; nothing else to announce.
        assert_eq!(announcement(CheckOrigin::Manual, &found, None), None);
        let up_to_date = UpdateState::UpToDate { no_releases: false };
        assert_eq!(announcement(CheckOrigin::Startup, &up_to_date, None), None);
    }

    #[test]
    fn notes_are_shortened_at_a_line_end() {
        assert_eq!(notes_excerpt("  short  "), "short");
        let long = format!("{}\n{}", "a".repeat(400), "b".repeat(400));
        let excerpt = notes_excerpt(&long);
        assert_eq!(excerpt, format!("{}\n…", "a".repeat(400)));
        let one_line = "x".repeat(1000);
        assert_eq!(
            notes_excerpt(&one_line).chars().count(),
            NOTES_EXCERPT_CHARS + 2
        );
    }

    #[test]
    fn progress_and_confirmation() {
        assert_eq!(percent(50, Some(200)), Some(25));
        assert_eq!(percent(300, Some(200)), Some(100));
        assert_eq!(percent(1, None), None);
        assert_eq!(percent(1, Some(0)), None);
        assert!(!UpdateBusy::default().needs_confirmation());
        assert!(
            UpdateBusy {
                macros: 1,
                vjoy: false
            }
            .needs_confirmation()
        );
        assert!(
            UpdateBusy {
                macros: 0,
                vjoy: true
            }
            .needs_confirmation()
        );
    }
}

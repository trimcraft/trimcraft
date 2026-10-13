//! Updates: what the Settings page ("Updates") and the header badge show about new releases.
//!
//! The app checks the release feed (the updater plugin: `latest.json` of the latest GitHub
//! release, signed) and reports here; installing runs the signed NSIS installer per user.

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

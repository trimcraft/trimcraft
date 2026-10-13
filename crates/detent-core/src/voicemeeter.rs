//! Voicemeeter (VB-Audio): gain and mute of input strips and output buses, through the
//! VoicemeeterRemote API. The app loads the API's DLL from the Voicemeeter installation and
//! sets parameters such as `Strip[0].Gain`; this module describes the actions and computes the
//! parameter values.

use serde::{Deserialize, Serialize};

/// Voicemeeter's fader range in dB.
pub const MIN_GAIN_DB: f32 = -60.0;
pub const MAX_GAIN_DB: f32 = 12.0;
/// Potato, the largest edition, has 8 strips and 8 buses.
pub const MAX_INDEX: u8 = 7;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum VmKind {
    /// An input strip.
    Strip,
    /// An output bus.
    Bus,
}

/// A strip or bus, by index from 0 (the UI shows it from 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VmTarget {
    pub kind: VmKind,
    pub index: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "type"
)]
pub enum VmCommand {
    SetGain { db: f32 },
    ChangeGain { delta_db: f32 },
    Mute,
    Unmute,
    ToggleMute,
}

impl VmTarget {
    /// The parameter name, e.g. `Strip[0].Gain`.
    pub fn parameter(self, field: &str) -> String {
        let kind = match self.kind {
            VmKind::Strip => "Strip",
            VmKind::Bus => "Bus",
        };
        format!("{kind}[{}].{field}", self.index)
    }

    pub fn describe(self) -> String {
        let kind = match self.kind {
            VmKind::Strip => "strip",
            VmKind::Bus => "bus",
        };
        format!("{kind} {}", u32::from(self.index) + 1)
    }
}

impl VmCommand {
    /// The parameter field this command changes.
    pub fn field(self) -> &'static str {
        match self {
            VmCommand::SetGain { .. } | VmCommand::ChangeGain { .. } => "Gain",
            VmCommand::Mute | VmCommand::Unmute | VmCommand::ToggleMute => "Mute",
        }
    }

    /// Whether the new value depends on the current one (it has to be read first).
    pub fn reads_current(self) -> bool {
        matches!(self, VmCommand::ChangeGain { .. } | VmCommand::ToggleMute)
    }

    /// The new parameter value, given the current one (gain in dB, mute 0.0 or 1.0).
    pub fn value(self, current: f32) -> f32 {
        let gain = |db: f32| db.clamp(MIN_GAIN_DB, MAX_GAIN_DB);
        match self {
            VmCommand::SetGain { db } => gain(db),
            VmCommand::ChangeGain { delta_db } => gain(current + delta_db),
            VmCommand::Mute => 1.0,
            VmCommand::Unmute => 0.0,
            VmCommand::ToggleMute => {
                if current >= 0.5 {
                    0.0
                } else {
                    1.0
                }
            }
        }
    }

    pub fn describe(self) -> String {
        match self {
            VmCommand::SetGain { db } => format!("gain {db} dB"),
            VmCommand::ChangeGain { delta_db } => format!("gain {delta_db:+} dB"),
            VmCommand::Mute => "mute".into(),
            VmCommand::Unmute => "unmute".into(),
            VmCommand::ToggleMute => "toggle mute".into(),
        }
    }

    pub fn valid(self) -> bool {
        match self {
            VmCommand::SetGain { db } => (MIN_GAIN_DB..=MAX_GAIN_DB).contains(&db),
            VmCommand::ChangeGain { delta_db } => {
                delta_db.is_finite() && delta_db.abs() <= MAX_GAIN_DB - MIN_GAIN_DB
            }
            _ => true,
        }
    }
}

/// The Voicemeeter edition `VBVMR_GetVoicemeeterType` reports.
pub fn edition_name(kind: i32) -> &'static str {
    match kind {
        1 => "Voicemeeter",
        2 => "Voicemeeter Banana",
        3 => "Voicemeeter Potato",
        _ => "Voicemeeter (unknown edition)",
    }
}

/// The install directory from the uninstall entry's `UninstallString`, e.g.
/// `"C:\Program Files (x86)\VB\Voicemeeter\voicemeetersetup.exe"` (quotes optional).
pub fn install_dir(uninstall: &str) -> Option<String> {
    let path = uninstall.trim().trim_matches('"');
    let cut = path.rfind(['\\', '/'])?;
    Some(path[..cut].to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parameters_and_values() {
        let strip = VmTarget {
            kind: VmKind::Strip,
            index: 0,
        };
        let bus = VmTarget {
            kind: VmKind::Bus,
            index: 2,
        };
        assert_eq!(strip.parameter(VmCommand::Mute.field()), "Strip[0].Mute");
        assert_eq!(
            bus.parameter(VmCommand::SetGain { db: 0.0 }.field()),
            "Bus[2].Gain"
        );
        assert_eq!(bus.describe(), "bus 3");
        assert_eq!(VmCommand::ChangeGain { delta_db: 3.0 }.value(10.0), 12.0);
        assert_eq!(VmCommand::ChangeGain { delta_db: -6.0 }.value(-3.0), -9.0);
        assert_eq!(VmCommand::SetGain { db: -80.0 }.value(0.0), -60.0);
        assert_eq!(VmCommand::ToggleMute.value(1.0), 0.0);
        assert_eq!(VmCommand::ToggleMute.value(0.0), 1.0);
        assert!(VmCommand::ToggleMute.reads_current());
        assert!(!VmCommand::Mute.reads_current());
        assert!(!VmCommand::SetGain { db: 20.0 }.valid());
        assert!(!VmCommand::ChangeGain { delta_db: f32::NAN }.valid());
    }

    #[test]
    fn install_dir_from_uninstall_string() {
        assert_eq!(
            install_dir(r#""C:\Program Files (x86)\VB\Voicemeeter\voicemeetersetup.exe""#)
                .as_deref(),
            Some(r"C:\Program Files (x86)\VB\Voicemeeter")
        );
        assert_eq!(install_dir("setup.exe"), None);
    }

    #[test]
    fn serialises_for_the_ui() {
        let c: VmCommand = serde_json::from_str(r#"{"type":"changeGain","deltaDb":-3}"#).unwrap();
        assert_eq!(c, VmCommand::ChangeGain { delta_db: -3.0 });
        let t: VmTarget = serde_json::from_str(r#"{"kind":"bus","index":1}"#).unwrap();
        assert_eq!(t.kind, VmKind::Bus);
    }
}

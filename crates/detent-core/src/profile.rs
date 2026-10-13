//! Per-game profiles and automatic switching.
//!
//! A profile holds what belongs to one game: its virtual controls and its overlay widgets
//! (the active radio states are kept per control, and control ids are unique across all
//! profiles). Devices, encoders, counters, connections and the overlay's monitor, hotkey and
//! visibility are global.
//!
//! Each profile lists executable names. When the foreground window changes to a program a
//! profile lists, that profile becomes active. When no profile lists it, the current profile
//! stays, or (if so set) the default profile, the first in the list, takes over. A profile
//! chosen by hand stays until the next foreground change to a program that a profile lists.
//! Switching never runs actions.

use crate::overlay::WidgetList;
use crate::virtual_control::VirtualControl;
use crate::vjoy::VjoyMapping;
use serde::{Deserialize, Serialize};

/// Id of the profile the setup of a version without profiles becomes.
pub const DEFAULT_PROFILE_ID: &str = "profile-1";
pub const DEFAULT_PROFILE_NAME: &str = "Default";

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Profile {
    pub id: String,
    pub name: String,
    /// Executable names (e.g. `DCS.exe`) that make this profile active; compared without
    /// regard to case.
    pub executables: Vec<String>,
    pub virtual_controls: Vec<VirtualControl>,
    pub overlay_widgets: WidgetList,
    /// Top-left corner of the toast area on the overlay; `None`: the default place (top centre).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub toast_position: Option<crate::overlay::WidgetPosition>,
    /// Physical axes, buttons and hats passed through to vJoy.
    pub vjoy_mappings: Vec<VjoyMapping>,
}

impl Profile {
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            ..Self::default()
        }
    }

    /// The vJoy devices this profile writes to: state outputs, vJoy actions and mappings.
    pub fn vjoy_devices(&self) -> std::collections::BTreeSet<u8> {
        self.virtual_controls
            .iter()
            .flat_map(VirtualControl::vjoy_devices)
            .chain(self.vjoy_mappings.iter().map(VjoyMapping::target_device))
            .collect()
    }

    /// Whether `exe` (a file name like `dcs.exe`) is one of this profile's executables.
    pub fn matches(&self, exe: &str) -> bool {
        self.executables.iter().any(|e| same_executable(e, exe))
    }
}

/// What happens when the foreground program matches no profile.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NoMatch {
    /// The current profile stays (alt-tabbing to a chat program keeps the game's profile).
    #[default]
    KeepCurrent,
    /// The default profile (the first one) becomes active.
    SwitchToDefault,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ProfileSwitching {
    /// Switch profiles by the foreground program.
    pub auto_switch: bool,
    pub no_match: NoMatch,
}

impl Default for ProfileSwitching {
    fn default() -> Self {
        Self {
            auto_switch: true,
            no_match: NoMatch::KeepCurrent,
        }
    }
}

/// Which profile a "Switch profile" action makes active.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum ProfileTarget {
    Profile {
        profile: String,
    },
    /// The next profile in the list, after the last one the first.
    Next,
    /// The previous profile in the list, before the first one the last.
    Previous,
}

impl ProfileTarget {
    /// Index of the profile to switch to, from `active`. `None` if the profile is gone.
    pub fn resolve(&self, profiles: &[Profile], active: usize) -> Option<usize> {
        let n = profiles.len();
        if n == 0 {
            return None;
        }
        match self {
            ProfileTarget::Profile { profile } => profiles.iter().position(|p| &p.id == profile),
            ProfileTarget::Next => Some((active + 1) % n),
            ProfileTarget::Previous => Some((active + n - 1) % n),
        }
    }

    pub fn describe(&self) -> String {
        match self {
            ProfileTarget::Profile { profile } => profile.clone(),
            ProfileTarget::Next => "next".into(),
            ProfileTarget::Previous => "previous".into(),
        }
    }
}

/// Characters Windows does not allow in file names.
const INVALID: &[char] = &['<', '>', ':', '"', '/', '\\', '|', '?', '*'];

/// Cleans up an executable name as typed: a full path keeps only the file name, surrounding
/// quotes and spaces go, and a name without an extension gets `.exe`. `None` if nothing
/// usable is left.
pub fn normalize_executable(name: &str) -> Option<String> {
    let name = name.trim().trim_matches('"');
    let name = name.rsplit(['\\', '/']).next().unwrap_or(name).trim();
    if name.is_empty() || name.contains(INVALID) || name.chars().any(char::is_control) {
        return None;
    }
    if name.contains('.') {
        Some(name.to_owned())
    } else {
        Some(format!("{name}.exe"))
    }
}

/// Executable names compare without regard to case, as Windows does.
pub fn same_executable(a: &str, b: &str) -> bool {
    a.to_lowercase() == b.to_lowercase()
}

/// Remembers the foreground program and whether the user chose the profile by hand, and
/// decides the switches.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AutoSwitch {
    foreground: Option<String>,
    manual: bool,
}

impl AutoSwitch {
    /// The program in the foreground, if known and not Detent itself.
    pub fn foreground(&self) -> Option<&str> {
        self.foreground.as_deref()
    }

    /// The user chose a profile (selector or "Switch profile" action). It stays until the
    /// next foreground change to a program a profile lists.
    pub fn manual_switch(&mut self) {
        self.manual = true;
    }

    /// The foreground window changed to `exe` (`None`: Detent itself, or a program whose name
    /// cannot be read). Returns the index of the profile to switch to, if any.
    ///
    /// Only a change to another program counts: a new window of the same program (a dialog,
    /// the game's launcher window) does not undo a manual choice.
    pub fn foreground_changed(
        &mut self,
        exe: Option<&str>,
        profiles: &[Profile],
        active: usize,
        settings: ProfileSwitching,
    ) -> Option<usize> {
        let same = match (&self.foreground, exe) {
            (Some(old), Some(new)) => same_executable(old, new),
            (None, None) => true,
            _ => false,
        };
        if same {
            return None;
        }
        self.foreground = exe.map(str::to_owned);
        let exe = exe?;
        if !settings.auto_switch {
            return None;
        }
        if profiles.get(active).is_some_and(|p| p.matches(exe)) {
            self.manual = false;
            return None;
        }
        if let Some(index) = profiles.iter().position(|p| p.matches(exe)) {
            self.manual = false;
            return Some(index);
        }
        match settings.no_match {
            NoMatch::SwitchToDefault if !self.manual && active != 0 => Some(0),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profiles() -> Vec<Profile> {
        let mut default = Profile::new("profile-1", "Default");
        default.executables = vec![];
        let mut dcs = Profile::new("profile-2", "DCS");
        dcs.executables = vec!["DCS.exe".into()];
        let mut il2 = Profile::new("profile-3", "IL-2");
        il2.executables = vec!["Il-2.exe".into(), "il2launcher.exe".into()];
        vec![default, dcs, il2]
    }

    const KEEP: ProfileSwitching = ProfileSwitching {
        auto_switch: true,
        no_match: NoMatch::KeepCurrent,
    };
    const TO_DEFAULT: ProfileSwitching = ProfileSwitching {
        auto_switch: true,
        no_match: NoMatch::SwitchToDefault,
    };

    #[test]
    fn executables_are_cleaned_up() {
        assert_eq!(
            normalize_executable(" DCS.exe ").as_deref(),
            Some("DCS.exe")
        );
        assert_eq!(normalize_executable("DCS").as_deref(), Some("DCS.exe"));
        assert_eq!(
            normalize_executable(r#""C:\Games\DCS World\bin\DCS.exe""#).as_deref(),
            Some("DCS.exe")
        );
        assert_eq!(
            normalize_executable("C:/x/game.bin").as_deref(),
            Some("game.bin")
        );
        assert_eq!(normalize_executable("   "), None);
        assert_eq!(normalize_executable(r"C:\Games\"), None);
        assert_eq!(normalize_executable("a?.exe"), None);
        assert!(same_executable("DCS.exe", "dcs.EXE"));
        assert!(profiles()[2].matches("IL-2.EXE"));
        assert!(!profiles()[2].matches("Il-2"));
    }

    #[test]
    fn matching_program_switches_and_others_keep_the_profile() {
        let p = profiles();
        let mut a = AutoSwitch::default();
        assert_eq!(a.foreground_changed(Some("dcs.exe"), &p, 0, KEEP), Some(1));
        // Alt-tab to Discord: the game's profile stays.
        assert_eq!(a.foreground_changed(Some("Discord.exe"), &p, 1, KEEP), None);
        assert_eq!(a.foreground(), Some("Discord.exe"));
        assert_eq!(a.foreground_changed(Some("Il-2.exe"), &p, 1, KEEP), Some(2));
        // Already active: nothing to do.
        assert_eq!(
            a.foreground_changed(Some("il2launcher.exe"), &p, 2, KEEP),
            None
        );
    }

    #[test]
    fn no_match_can_fall_back_to_the_default_profile() {
        let p = profiles();
        let mut a = AutoSwitch::default();
        assert_eq!(
            a.foreground_changed(Some("DCS.exe"), &p, 0, TO_DEFAULT),
            Some(1)
        );
        assert_eq!(
            a.foreground_changed(Some("Discord.exe"), &p, 1, TO_DEFAULT),
            Some(0)
        );
        // Already on the default profile.
        assert_eq!(
            a.foreground_changed(Some("explorer.exe"), &p, 0, TO_DEFAULT),
            None
        );
    }

    #[test]
    fn detent_itself_and_unknown_programs_never_switch() {
        let p = profiles();
        let mut a = AutoSwitch::default();
        assert_eq!(
            a.foreground_changed(Some("DCS.exe"), &p, 0, TO_DEFAULT),
            Some(1)
        );
        assert_eq!(a.foreground_changed(None, &p, 1, TO_DEFAULT), None);
        assert_eq!(a.foreground(), None);
        // Back to the game counts as a change again.
        assert_eq!(
            a.foreground_changed(Some("DCS.exe"), &p, 0, TO_DEFAULT),
            Some(1)
        );
    }

    #[test]
    fn manual_choice_stays_until_a_listed_program_comes_to_the_front() {
        let p = profiles();
        let mut a = AutoSwitch::default();
        assert_eq!(
            a.foreground_changed(Some("DCS.exe"), &p, 0, TO_DEFAULT),
            Some(1)
        );
        // A controller button picks IL-2 while DCS has the focus.
        a.manual_switch();
        // Another DCS window: same program, no change.
        assert_eq!(
            a.foreground_changed(Some("dcs.exe"), &p, 2, TO_DEFAULT),
            None
        );
        // Discord matches nothing: the fallback does not override the manual choice.
        assert_eq!(
            a.foreground_changed(Some("Discord.exe"), &p, 2, TO_DEFAULT),
            None
        );
        // Back to DCS: a different profile matches and wins.
        assert_eq!(
            a.foreground_changed(Some("DCS.exe"), &p, 2, TO_DEFAULT),
            Some(1)
        );
        // The manual choice is over: the fallback applies again.
        assert_eq!(
            a.foreground_changed(Some("Discord.exe"), &p, 1, TO_DEFAULT),
            Some(0)
        );
    }

    #[test]
    fn auto_switch_off_only_tracks_the_foreground() {
        let p = profiles();
        let mut a = AutoSwitch::default();
        let off = ProfileSwitching {
            auto_switch: false,
            no_match: NoMatch::SwitchToDefault,
        };
        assert_eq!(a.foreground_changed(Some("DCS.exe"), &p, 0, off), None);
        assert_eq!(a.foreground(), Some("DCS.exe"));
        assert_eq!(a.foreground_changed(Some("x.exe"), &p, 2, off), None);
    }

    #[test]
    fn targets_wrap_around() {
        let p = profiles();
        assert_eq!(ProfileTarget::Next.resolve(&p, 2), Some(0));
        assert_eq!(ProfileTarget::Previous.resolve(&p, 0), Some(2));
        assert_eq!(ProfileTarget::Next.resolve(&p, 0), Some(1));
        let to = |id: &str| ProfileTarget::Profile { profile: id.into() };
        assert_eq!(to("profile-3").resolve(&p, 0), Some(2));
        assert_eq!(to("gone").resolve(&p, 0), None);
        assert_eq!(ProfileTarget::Next.resolve(&[], 0), None);
    }

    #[test]
    fn json_shape_is_stable() {
        assert_eq!(
            serde_json::to_string(&ProfileSwitching::default()).unwrap(),
            r#"{"autoSwitch":true,"noMatch":"keepCurrent"}"#
        );
        assert_eq!(
            serde_json::to_string(&ProfileTarget::Profile {
                profile: "profile-2".into()
            })
            .unwrap(),
            r#"{"type":"profile","profile":"profile-2"}"#
        );
        assert_eq!(
            serde_json::to_string(&ProfileTarget::Next).unwrap(),
            r#"{"type":"next"}"#
        );
    }
}

//! The user's configuration: encoders, functions, profiles (virtual controls and overlay
//! widgets per game), overlay settings. Stored as one JSON file.
//!
//! Schema 1 had one set of virtual controls and overlay widgets; loading such a file turns
//! them into the profile "Default" (see [`migrate_v1`]).

use crate::connection::Connections;
use crate::device::DeviceId;
use crate::encoder::{DetectorConfig, EncoderBinding};
use crate::function::Function;
use crate::overlay::OverlayConfig;
use crate::profile::{DEFAULT_PROFILE_ID, DEFAULT_PROFILE_NAME, Profile, ProfileSwitching};
use crate::recording::RecorderConfig;
use crate::settings::AppSettings;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use thiserror::Error;

/// 3: virtual controls of more kinds (toggle, momentary, counter, mode) and triggers with
/// press kinds, modifiers, conditions and axis zones. Schema 2 files load unchanged (all new
/// fields have defaults); the bump makes older versions refuse the file instead of failing on
/// the new kinds.
pub const SETUP_SCHEMA_VERSION: u32 = 3;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ButtonPair {
    pub device: DeviceId,
    pub decrement: u16,
    pub increment: u16,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Setup {
    pub schema_version: u32,
    pub encoders: Vec<EncoderBinding>,
    pub functions: Vec<Function>,
    /// Never empty; the first one is the default profile.
    pub profiles: Vec<Profile>,
    /// Id of the active profile; always one of `profiles`.
    pub active_profile: String,
    pub profile_switching: ProfileSwitching,
    /// Global overlay settings. The widgets are stored per profile, so `overlay.widgets` is
    /// always empty here.
    pub overlay: OverlayConfig,
    pub detector: DetectorConfig,
    /// Macro recorder settings. Missing in files from before the recorder.
    pub recorder: RecorderConfig,
    /// Connector settings (Soundpad). Missing in files from before connections.
    pub connections: Connections,
    /// Encoder suggestions the user rejected; not suggested again.
    pub dismissed_suggestions: Vec<ButtonPair>,
    /// App-wide settings (Settings page), not part of a profile. Missing in files from before
    /// the Settings page.
    pub settings: AppSettings,
}

impl Default for Setup {
    fn default() -> Self {
        Self {
            schema_version: SETUP_SCHEMA_VERSION,
            encoders: Vec::new(),
            functions: Vec::new(),
            profiles: vec![Profile::new(DEFAULT_PROFILE_ID, DEFAULT_PROFILE_NAME)],
            active_profile: DEFAULT_PROFILE_ID.into(),
            profile_switching: ProfileSwitching::default(),
            overlay: OverlayConfig::default(),
            detector: DetectorConfig::default(),
            recorder: RecorderConfig::default(),
            connections: Connections::default(),
            dismissed_suggestions: Vec::new(),
            settings: AppSettings::default(),
        }
    }
}

#[derive(Debug, Error)]
pub enum SetupError {
    #[error(
        "setup file was written by a newer Detent (schema {found}, supported {SETUP_SCHEMA_VERSION})"
    )]
    NewerSchema { found: u64 },
    #[error("setup file is not valid: {0}")]
    Invalid(#[from] serde_json::Error),
}

/// Schema 1 to 2: the virtual controls and the overlay widgets become the profile "Default",
/// which lists no executables. Everything else stays where it was.
pub fn migrate_v1(setup: &mut Value) {
    let Some(obj) = setup.as_object_mut() else {
        return;
    };
    let controls = obj
        .remove("virtualControls")
        .unwrap_or_else(|| Value::Array(Vec::new()));
    let widgets = obj
        .get_mut("overlay")
        .and_then(Value::as_object_mut)
        .and_then(|o| o.remove("widgets"))
        .unwrap_or_else(|| Value::Array(Vec::new()));
    obj.insert(
        "profiles".into(),
        serde_json::json!([{
            "id": DEFAULT_PROFILE_ID,
            "name": DEFAULT_PROFILE_NAME,
            "executables": [],
            "virtualControls": controls,
            "overlayWidgets": widgets,
        }]),
    );
    obj.insert("activeProfile".into(), DEFAULT_PROFILE_ID.into());
    obj.insert("schemaVersion".into(), 2.into());
}

impl Setup {
    pub fn from_json(json: &str) -> Result<Self, SetupError> {
        let mut value: Value = serde_json::from_str(json)?;
        let version = value
            .get("schemaVersion")
            .and_then(Value::as_u64)
            .unwrap_or(1);
        if version > u64::from(SETUP_SCHEMA_VERSION) {
            return Err(SetupError::NewerSchema { found: version });
        }
        if version < 2 {
            migrate_v1(&mut value);
        }
        let mut setup: Setup = serde_json::from_value(value)?;
        setup.schema_version = SETUP_SCHEMA_VERSION;
        setup.repair();
        Ok(setup)
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("setup serialises")
    }

    /// Restores the invariants a hand-edited file may break: at least one profile, unique
    /// profile ids, an active profile that exists, control ids unique across profiles, and
    /// no overlay widgets outside the profiles.
    fn repair(&mut self) {
        if self.profiles.is_empty() {
            self.profiles
                .push(Profile::new(DEFAULT_PROFILE_ID, DEFAULT_PROFILE_NAME));
        }
        let mut ids = HashSet::new();
        for i in 0..self.profiles.len() {
            if self.profiles[i].id.is_empty() || !ids.insert(self.profiles[i].id.clone()) {
                let id = next_id("profile", self.profiles.iter().map(|p| p.id.as_str()));
                ids.insert(id.clone());
                self.profiles[i].id = id;
            }
        }
        let mut taken: HashSet<String> = self
            .profiles
            .iter()
            .flat_map(|p| p.virtual_controls.iter().map(|c| c.id.clone()))
            .collect();
        let mut seen = HashSet::new();
        for i in 0..self.profiles.len() {
            let mut renamed = HashMap::new();
            for j in 0..self.profiles[i].virtual_controls.len() {
                let id = self.profiles[i].virtual_controls[j].id.clone();
                if seen.insert(id.clone()) {
                    continue;
                }
                let new = next_id("vc", taken.iter().map(String::as_str));
                taken.insert(new.clone());
                seen.insert(new.clone());
                self.profiles[i].virtual_controls[j].id = new.clone();
                renamed.insert(id, new);
            }
            self.profiles[i]
                .overlay_widgets
                .rename_virtual_controls(&renamed);
        }
        if !self.profiles.iter().any(|p| p.id == self.active_profile) {
            self.active_profile = self.profiles[0].id.clone();
        }
        let stray = std::mem::take(&mut self.overlay.widgets);
        let active = self.active_index();
        self.profiles[active].overlay_widgets.extend(stray.0);
    }

    /// Index of the active profile in `profiles`.
    pub fn active_index(&self) -> usize {
        self.profiles
            .iter()
            .position(|p| p.id == self.active_profile)
            .unwrap_or(0)
    }

    pub fn profile(&self) -> &Profile {
        &self.profiles[self.active_index()]
    }

    pub fn profile_mut(&mut self) -> &mut Profile {
        let i = self.active_index();
        &mut self.profiles[i]
    }
}

/// `<prefix>-<n>` with `n` one more than the highest in use.
pub fn next_id<'a>(prefix: &str, existing: impl Iterator<Item = &'a str>) -> String {
    let max = existing
        .filter_map(|id| {
            id.strip_prefix(prefix)?
                .strip_prefix('-')?
                .parse::<u32>()
                .ok()
        })
        .max()
        .unwrap_or(0);
    format!("{prefix}-{}", max + 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::overlay::OverlayWidget;
    use crate::profile::NoMatch;

    #[test]
    fn empty_object_gives_defaults() {
        assert_eq!(Setup::from_json("{}").unwrap(), Setup::default());
    }

    #[test]
    fn newer_schema_is_refused() {
        let err = Setup::from_json(r#"{"schemaVersion": 99}"#).unwrap_err();
        assert!(matches!(err, SetupError::NewerSchema { found: 99 }));
    }

    #[test]
    fn file_from_before_virtual_controls_loads() {
        let json = r#"{
            "schemaVersion": 1,
            "encoders": [{"id":"enc-1","name":"Knob","device":"f00d:0003#1","decrement":10,"increment":11}],
            "functions": [],
            "overlay": {"visible": true, "hotkey": "Ctrl+Shift+F10", "anchor": "bottomLeft",
                        "margin": 32, "scale": 1.0, "widgets": [{"type":"function","function":"fn-1"}]},
            "detector": {"maxPulseMs": 200, "maxRunGapMs": 600, "minRunLength": 3},
            "dismissedSuggestions": []
        }"#;
        let s = Setup::from_json(json).unwrap();
        assert!(s.profile().virtual_controls.is_empty());
        assert_eq!(s.profile().overlay_widgets.len(), 1);
        assert_eq!(s.encoders.len(), 1);
    }

    #[test]
    fn file_from_stage_1_loads_with_default_playback_and_recorder() {
        let json = r#"{
            "schemaVersion": 1,
            "virtualControls": [{"id":"vc-1","name":"Comms","kind":{"type":"radio","states":[
                {"id":"st-1","name":"A","select":[],
                 "onActivate":[{"type":"keyboardMacro","steps":[{"type":"keyTap","key":"F1","holdMs":null}]}],
                 "onDeactivate":[{"type":"mouseMacro","steps":[{"type":"moveRelative","dx":5,"dy":0}]}]}],
                "increment":[],"decrement":[],"clear":[],"toggleOff":[],
                "end":"wrap","toggleOnReselect":false,"startup":"restoreLast"}}]
        }"#;
        let s = Setup::from_json(json).unwrap();
        assert_eq!(s.recorder, RecorderConfig::default());
        let state = &s.profile().virtual_controls[0].radio().unwrap().states[0];
        let crate::action::Action::MouseMacro {
            playback, movement, ..
        } = &state.on_deactivate[0]
        else {
            panic!("mouse macro expected");
        };
        assert_eq!(playback.speed_percent, 100);
        assert_eq!(*movement, crate::action::Movement::Absolute);
        assert_eq!(Setup::from_json(&s.to_json()).unwrap(), s);
    }

    #[test]
    fn file_from_stage_2_loads_with_discord_disabled() {
        let json = r#"{
            "schemaVersion": 1,
            "connections": {"soundpad": {"enabled": true, "timeoutMs": 800, "listMaxAgeMs": 10000}}
        }"#;
        let s = Setup::from_json(json).unwrap();
        assert!(s.connections.soundpad.enabled);
        assert_eq!(
            s.connections.discord,
            crate::connection::DiscordSettings::default()
        );
        assert!(!s.connections.discord.enabled);
        // The secret itself never ends up in the file, only where it is stored.
        let mut s = s;
        s.connections.discord.client_secret =
            Some(crate::connection::DiscordSettings::secret_ref());
        let out = s.to_json();
        assert!(out.contains("\"target\": \"detent/discord/client-secret\""));
        assert_eq!(Setup::from_json(&out).unwrap(), s);
    }

    /// A complete schema 1 file as the version before profiles wrote it.
    const SCHEMA_1: &str = r#"{
        "schemaVersion": 1,
        "encoders": [{"id":"enc-1","name":"Knob","device":"f00d:0003#1","decrement":10,"increment":11}],
        "functions": [{"id":"fn-1","name":"Channel","kind":{"type":"counter",
            "spec":{"min":1,"max":20,"step":1,"initial":1,"end":"wrap","decimals":0},
            "input":{"type":"encoder","encoder":"enc-1"}}}],
        "virtualControls": [{"id":"vc-1","name":"Comms","kind":{"type":"radio","states":[
            {"id":"st-1","name":"A","select":[],"onActivate":[],"onDeactivate":[]},
            {"id":"st-2","name":"B","select":[],"onActivate":[],"onDeactivate":[]}],
            "increment":[{"type":"encoderStep","encoder":"enc-1","direction":"increment"}],
            "decrement":[],"clear":[],"toggleOff":[],
            "end":"wrap","toggleOnReselect":false,"startup":"restoreLast"}}],
        "overlay": {"visible": false, "hotkey": "Ctrl+Alt+O", "anchor": "topRight",
            "monitor": {"name":"\\\\.\\DISPLAY2","x":2560,"y":0,"width":1920,"height":1080},
            "margin": 40, "scale": 1.5,
            "widgets": [{"type":"radio","control":"vc-1","layout":"horizontal","position":{"x":0.1,"y":0.2}},
                        {"type":"function","function":"fn-1","position":null}]},
        "detector": {"maxPulseMs": 150, "maxRunGapMs": 500, "minRunLength": 4},
        "recorder": {"stopHotkey":"Ctrl+Shift+F8","tolerance":3,"sampleIntervalMs":10},
        "connections": {"soundpad": {"enabled": true, "timeoutMs": 800, "listMaxAgeMs": 10000}},
        "dismissedSuggestions": [{"device":"f00d:0003#1","decrement":1,"increment":2}]
    }"#;

    #[test]
    fn schema_1_becomes_the_default_profile() {
        let s = Setup::from_json(SCHEMA_1).unwrap();
        assert_eq!(s.schema_version, SETUP_SCHEMA_VERSION);
        assert_eq!(s.profiles.len(), 1);
        let p = &s.profiles[0];
        assert_eq!(p.id, DEFAULT_PROFILE_ID);
        assert_eq!(p.name, "Default");
        assert!(p.executables.is_empty());
        assert_eq!(s.active_profile, p.id);
        // Per profile: controls and widgets with their positions and layout.
        assert_eq!(p.virtual_controls.len(), 1);
        assert_eq!(p.virtual_controls[0].id, "vc-1");
        assert_eq!(p.overlay_widgets.len(), 2);
        assert!(matches!(
            p.overlay_widgets[0].widget,
            OverlayWidget::Radio { ref control, layout: crate::overlay::RadioLayout::Horizontal } if control == "vc-1"
        ));
        assert!(p.overlay_widgets[0].position.is_some());
        // Global: everything else, unchanged.
        assert!(s.overlay.widgets.is_empty());
        assert!(!s.overlay.visible);
        assert_eq!(s.overlay.hotkey, "Ctrl+Alt+O");
        assert_eq!(s.overlay.monitor.as_ref().unwrap().x, 2560);
        assert_eq!(s.overlay.margin, 40);
        assert_eq!(s.encoders.len(), 1);
        assert_eq!(s.functions.len(), 1);
        assert_eq!(s.detector.min_run_length, 4);
        assert_eq!(s.recorder.stop_hotkey, "Ctrl+Shift+F8");
        assert!(s.connections.soundpad.enabled);
        assert_eq!(s.dismissed_suggestions.len(), 1);
        assert_eq!(s.profile_switching, ProfileSwitching::default());
        assert!(s.profile_switching.auto_switch);
        assert_eq!(s.profile_switching.no_match, NoMatch::KeepCurrent);
    }

    #[test]
    fn schema_2_is_written_and_read_back() {
        let s = Setup::from_json(SCHEMA_1).unwrap();
        let out = s.to_json();
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["schemaVersion"], SETUP_SCHEMA_VERSION);
        assert!(v.get("virtualControls").is_none());
        assert!(v["overlay"].get("widgets").is_none());
        assert_eq!(v["profiles"][0]["overlayWidgets"][0]["type"], "radio");
        assert_eq!(v["activeProfile"], DEFAULT_PROFILE_ID);
        assert_eq!(v["profileSwitching"]["noMatch"], "keepCurrent");
        assert_eq!(Setup::from_json(&out).unwrap(), s);
    }

    #[test]
    fn broken_profile_data_is_repaired() {
        // No profiles at all.
        let s =
            Setup::from_json(r#"{"schemaVersion":2,"profiles":[],"activeProfile":"x"}"#).unwrap();
        assert_eq!(s.profiles.len(), 1);
        assert_eq!(s.active_profile, s.profiles[0].id);
        // Unknown active profile, the same control id in two profiles, a duplicate profile
        // id, stray widgets.
        let radio = |id: &str| {
            serde_json::json!({"id": id, "name": "R", "kind": {"type": "radio",
                "states": [{"id":"st-1","name":"A","select":[],"onActivate":[],"onDeactivate":[]}]}})
        };
        let json = serde_json::json!({
            "schemaVersion": 2,
            "profiles": [
                {"id": "profile-1", "name": "A", "virtualControls": [radio("vc-1")]},
                {"id": "profile-1", "name": "B", "virtualControls": [radio("vc-1"), radio("vc-2")],
                 "overlayWidgets": [{"type":"radio","control":"vc-1"}]}
            ],
            "activeProfile": "gone",
            "overlay": {"widgets": [{"type":"profileName"}]}
        });
        let s = Setup::from_json(&json.to_string()).unwrap();
        assert_eq!(s.profiles[0].id, "profile-1");
        assert_eq!(s.profiles[1].id, "profile-2");
        assert_eq!(s.active_profile, "profile-1");
        assert_eq!(s.profiles[1].virtual_controls[0].id, "vc-3");
        assert!(s.profiles[1].overlay_widgets.has_virtual_control("vc-3"));
        assert!(s.overlay.widgets.is_empty());
        assert!(s.profiles[0].overlay_widgets.has_profile_name());
    }

    #[test]
    fn file_from_before_settings_loads_with_default_settings() {
        let json = r#"{
            "schemaVersion": 1,
            "connections": {"soundpad": {"enabled": false, "timeoutMs": 800, "listMaxAgeMs": 10000}}
        }"#;
        let s = Setup::from_json(json).unwrap();
        assert_eq!(s.settings, AppSettings::default());
        let mut s = s;
        s.settings.start_with_windows = true;
        s.settings.close_to_tray = false;
        let out = s.to_json();
        assert!(out.contains("\"startWithWindows\": true"));
        assert_eq!(Setup::from_json(&out).unwrap(), s);
    }

    #[test]
    fn file_from_before_profiles_keeps_its_settings_global() {
        let json = r#"{
            "schemaVersion": 1,
            "virtualControls": [],
            "overlay": {"visible": true, "hotkey": "Ctrl+Shift+F10", "anchor": "bottomLeft",
                        "margin": 32, "scale": 1.0, "widgets": []},
            "settings": {"closeToTray": false, "startWithWindows": true, "startMinimized": false,
                         "runAsAdmin": true, "trayNoticeShown": true}
        }"#;
        let s = Setup::from_json(json).unwrap();
        assert_eq!(s.schema_version, SETUP_SCHEMA_VERSION);
        assert_eq!(s.profiles.len(), 1);
        assert!(s.settings.start_with_windows && s.settings.run_as_admin);
        assert!(!s.settings.close_to_tray && !s.settings.start_minimized);
        assert!(s.settings.tray_notice_shown);
        let out: serde_json::Value = serde_json::from_str(&s.to_json()).unwrap();
        assert_eq!(out["settings"]["runAsAdmin"], true);
        assert!(out["profiles"][0].get("settings").is_none());
    }

    #[test]
    fn round_trip() {
        let mut s = Setup::default();
        s.dismissed_suggestions.push(ButtonPair {
            device: DeviceId("d".into()),
            decrement: 1,
            increment: 2,
        });
        assert_eq!(Setup::from_json(&s.to_json()).unwrap(), s);
    }

    #[test]
    fn ids_continue_after_highest() {
        assert_eq!(next_id("vc", ["vc-1", "vc-7", "x-9"].into_iter()), "vc-8");
        assert_eq!(next_id("profile", std::iter::empty()), "profile-1");
    }

    #[test]
    fn schema_2_loads_with_plain_triggers() {
        let json = r#"{
            "schemaVersion": 2,
            "profiles": [{"id": "profile-1", "name": "Default", "virtualControls": [
                {"id":"vc-1","name":"Comms","kind":{"type":"radio","states":[
                    {"id":"st-1","name":"A","select":[{"type":"button","device":"d","index":3}]}],
                 "increment":[{"type":"encoderStep","encoder":"enc-1","direction":"increment"}],
                 "clear":[{"type":"hat","device":"d","index":0,"direction":"up"}]}}]}],
            "activeProfile": "profile-1"
        }"#;
        let s = Setup::from_json(json).unwrap();
        assert_eq!(s.schema_version, 3);
        let control = &s.profile().virtual_controls[0];
        for t in control.triggers() {
            assert_eq!(t.press, crate::trigger::PressKind::Press);
            assert!(t.modifiers.is_empty() && t.conditions.is_empty());
        }
        assert_eq!(control.triggers().count(), 3);
        // Written back as schema 3, read again unchanged.
        assert_eq!(Setup::from_json(&s.to_json()).unwrap(), s);
        assert!(Setup::from_json(r#"{"schemaVersion": 4}"#).is_err());
    }
}

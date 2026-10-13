//! The user's configuration: encoders, functions, virtual controls, overlay. Stored as one
//! JSON file.
//!
//! Stage 2 turns this into per-game profiles; until then there is a single setup.

use crate::connection::Connections;
use crate::device::DeviceId;
use crate::encoder::{DetectorConfig, EncoderBinding};
use crate::function::Function;
use crate::overlay::OverlayConfig;
use crate::recording::RecorderConfig;
use crate::settings::AppSettings;
use crate::virtual_control::VirtualControl;
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const SETUP_SCHEMA_VERSION: u32 = 1;

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
    /// Radios and later other virtual controls. Missing in files from before they existed.
    pub virtual_controls: Vec<VirtualControl>,
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
            virtual_controls: Vec::new(),
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
    NewerSchema { found: u32 },
    #[error("setup file is not valid: {0}")]
    Invalid(#[from] serde_json::Error),
}

impl Setup {
    pub fn from_json(json: &str) -> Result<Self, SetupError> {
        let setup: Setup = serde_json::from_str(json)?;
        if setup.schema_version > SETUP_SCHEMA_VERSION {
            return Err(SetupError::NewerSchema {
                found: setup.schema_version,
            });
        }
        Ok(Setup {
            schema_version: SETUP_SCHEMA_VERSION,
            ..setup
        })
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("setup serialises")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert!(s.virtual_controls.is_empty());
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
        let state = &s.virtual_controls[0].radio().states[0];
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
    fn round_trip() {
        let mut s = Setup::default();
        s.dismissed_suggestions.push(ButtonPair {
            device: DeviceId("d".into()),
            decrement: 1,
            increment: 2,
        });
        assert_eq!(Setup::from_json(&s.to_json()).unwrap(), s);
    }
}

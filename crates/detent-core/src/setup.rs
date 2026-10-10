//! The user's configuration: encoders, functions, virtual controls, overlay. Stored as one
//! JSON file.
//!
//! Stage 2 turns this into per-game profiles; until then there is a single setup.

use crate::device::DeviceId;
use crate::encoder::{DetectorConfig, EncoderBinding};
use crate::function::Function;
use crate::overlay::OverlayConfig;
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
    /// Encoder suggestions the user rejected; not suggested again.
    pub dismissed_suggestions: Vec<ButtonPair>,
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
            dismissed_suggestions: Vec::new(),
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

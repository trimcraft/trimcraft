//! The user's configuration: encoders, functions, overlay. Stored as one JSON file.
//!
//! Stage 2 turns this into per-game profiles; until then there is a single setup.

use crate::device::DeviceId;
use crate::encoder::{DetectorConfig, EncoderBinding};
use crate::function::Function;
use crate::overlay::OverlayConfig;
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

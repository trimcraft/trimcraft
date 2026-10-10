//! Connections: external programs Detent talks to (Soundpad now, Discord later).
//!
//! Each connector has persisted settings (in `setup.json`, under `connections`) and a live
//! status the app reports. A connector's actions are only offered once it is enabled; actions
//! of a disabled connector stay stored but are skipped when they fire.
//!
//! Settings never hold secrets. A connector that needs credentials (Discord: client secret,
//! OAuth tokens) stores a [`SecretRef`] that names an entry in the operating system's
//! credential store; the app reads and writes the secret there.

use crate::action::Action;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Identifies a connector; also its key in the UI and the logs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ConnectorId {
    Soundpad,
}

impl ConnectorId {
    pub const ALL: [ConnectorId; 1] = [ConnectorId::Soundpad];

    pub fn name(self) -> &'static str {
        match self {
            ConnectorId::Soundpad => "Soundpad",
        }
    }

    /// The connector an action needs, if any. Keyboard and mouse macros are built in.
    pub fn for_action(action: &Action) -> Option<ConnectorId> {
        match action {
            Action::Soundpad { .. } => Some(ConnectorId::Soundpad),
            Action::KeyboardMacro { .. } | Action::MouseMacro { .. } => None,
        }
    }
}

/// Where a secret lives, never the secret itself. For future connectors with credentials.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "store")]
pub enum SecretRef {
    /// An entry ("target name") in the Windows Credential Manager.
    WindowsCredentialManager { target: String },
}

/// Bounds for pipe timeouts.
pub const MIN_TIMEOUT_MS: u32 = 100;
pub const MAX_TIMEOUT_MS: u32 = 5_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SoundpadSettings {
    pub enabled: bool,
    /// Upper bound for one request to Soundpad's pipe (connect, write, read).
    pub timeout_ms: u32,
    /// A cached sound list older than this is fetched again before a sound is looked up.
    pub list_max_age_ms: u32,
}

impl Default for SoundpadSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            timeout_ms: 500,
            list_max_age_ms: 10_000,
        }
    }
}

/// Settings of all connectors, as stored in the setup.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Connections {
    pub soundpad: SoundpadSettings,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ConnectionError {
    #[error("timeout must be between {MIN_TIMEOUT_MS} and {MAX_TIMEOUT_MS} ms")]
    BadTimeout,
    #[error("sound list age must be at most 10 minutes")]
    BadListAge,
}

impl Connections {
    pub fn enabled(&self, id: ConnectorId) -> bool {
        match id {
            ConnectorId::Soundpad => self.soundpad.enabled,
        }
    }

    /// Whether an action can run: built-in actions always, others when their connector is on.
    pub fn action_enabled(&self, action: &Action) -> bool {
        ConnectorId::for_action(action).is_none_or(|id| self.enabled(id))
    }

    pub fn validate(&self) -> Result<(), ConnectionError> {
        let sp = &self.soundpad;
        if !(MIN_TIMEOUT_MS..=MAX_TIMEOUT_MS).contains(&sp.timeout_ms) {
            return Err(ConnectionError::BadTimeout);
        }
        if sp.list_max_age_ms > 600_000 {
            return Err(ConnectionError::BadListAge);
        }
        Ok(())
    }
}

/// A connector's live state, as the app reports it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", tag = "state")]
pub enum ConnectorStatus {
    /// Disabled (or missing settings).
    #[default]
    NotConfigured,
    Connecting,
    Connected,
    Error {
        message: String,
    },
}

/// What the app knows about a connector right now.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConnectorState {
    pub status: ConnectorStatus,
    /// E.g. "26 sounds".
    pub detail: Option<String>,
    /// The last problem running one of its actions, until one succeeds again.
    pub warning: Option<String>,
}

/// A connector as the UI shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectorView {
    pub id: ConnectorId,
    pub name: String,
    pub enabled: bool,
    pub status: ConnectorStatus,
    pub detail: Option<String>,
    pub warning: Option<String>,
}

impl ConnectorView {
    /// A disabled connector is "not configured" whatever was last reported.
    pub fn new(id: ConnectorId, enabled: bool, state: Option<&ConnectorState>) -> Self {
        let (status, detail, warning) = match (enabled, state) {
            (false, _) => (ConnectorStatus::NotConfigured, None, None),
            (true, None) => (ConnectorStatus::Connecting, None, None),
            (true, Some(s)) => (s.status.clone(), s.detail.clone(), s.warning.clone()),
        };
        Self {
            id,
            name: id.name().into(),
            enabled,
            status,
            detail,
            warning,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action::KeyStep;
    use crate::soundpad::SoundpadCommand;

    fn stop() -> Action {
        Action::Soundpad {
            command: SoundpadCommand::Stop,
        }
    }

    #[test]
    fn actions_are_gated_by_their_connector() {
        let mut c = Connections::default();
        let keys = Action::keyboard(vec![KeyStep::Delay { ms: 1 }]);
        let mouse = Action::mouse(vec![]);
        assert_eq!(
            ConnectorId::for_action(&stop()),
            Some(ConnectorId::Soundpad)
        );
        assert_eq!(ConnectorId::for_action(&keys), None);
        // Soundpad is off by default; macros never need a connector.
        assert!(!c.action_enabled(&stop()));
        assert!(c.action_enabled(&keys) && c.action_enabled(&mouse));
        c.soundpad.enabled = true;
        assert!(c.action_enabled(&stop()));
    }

    #[test]
    fn settings_serialise_with_defaults() {
        let c: Connections = serde_json::from_str("{}").unwrap();
        assert_eq!(c, Connections::default());
        let c: Connections = serde_json::from_str(r#"{"soundpad":{"enabled":true}}"#).unwrap();
        assert!(c.soundpad.enabled);
        assert_eq!(c.soundpad.timeout_ms, 500);
        assert_eq!(
            serde_json::to_string(&c).unwrap(),
            r#"{"soundpad":{"enabled":true,"timeoutMs":500,"listMaxAgeMs":10000}}"#
        );
        let secret = SecretRef::WindowsCredentialManager {
            target: "detent/discord/client-secret".into(),
        };
        assert_eq!(
            serde_json::to_string(&secret).unwrap(),
            r#"{"store":"windowsCredentialManager","target":"detent/discord/client-secret"}"#
        );
    }

    #[test]
    fn validation() {
        let mut c = Connections::default();
        assert_eq!(c.validate(), Ok(()));
        c.soundpad.timeout_ms = 50;
        assert_eq!(c.validate(), Err(ConnectionError::BadTimeout));
        c.soundpad.timeout_ms = 500;
        c.soundpad.list_max_age_ms = 700_000;
        assert_eq!(c.validate(), Err(ConnectionError::BadListAge));
    }

    #[test]
    fn view_reflects_enabled_and_reported_state() {
        let reported = ConnectorState {
            status: ConnectorStatus::Connected,
            detail: Some("26 sounds".into()),
            warning: None,
        };
        let off = ConnectorView::new(ConnectorId::Soundpad, false, Some(&reported));
        assert_eq!(off.status, ConnectorStatus::NotConfigured);
        assert_eq!(off.detail, None);
        let starting = ConnectorView::new(ConnectorId::Soundpad, true, None);
        assert_eq!(starting.status, ConnectorStatus::Connecting);
        let on = ConnectorView::new(ConnectorId::Soundpad, true, Some(&reported));
        assert_eq!(on.detail.as_deref(), Some("26 sounds"));
        let json = serde_json::to_value(ConnectorView::new(
            ConnectorId::Soundpad,
            true,
            Some(&ConnectorState {
                status: ConnectorStatus::Error {
                    message: "not running".into(),
                },
                ..Default::default()
            }),
        ))
        .unwrap();
        assert_eq!(json["id"], "soundpad");
        assert_eq!(json["status"]["state"], "error");
        assert_eq!(json["status"]["message"], "not running");
    }
}

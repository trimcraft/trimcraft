//! Connections: external programs and drivers Trimcraft talks to (Soundpad, Discord, Voicemeeter,
//! vJoy).
//!
//! Each connector has persisted settings (in `setup.json`, under `connections`) and a live
//! status the app reports. A connector's actions are only offered once it is enabled; actions
//! of a disabled connector stay stored but are skipped when they fire.
//!
//! Settings never hold secrets. A connector that needs credentials (Discord: client secret,
//! OAuth tokens) stores a [`SecretRef`] that names an entry in the operating system's
//! credential store; the app reads and writes the secret there.

use crate::action::Action;
use crate::discord::{GuideStep, VoiceState};
use crate::vjoy::VjoyInfo;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Identifies a connector; also its key in the UI and the logs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ConnectorId {
    Soundpad,
    Discord,
    Voicemeeter,
    Vjoy,
}

impl ConnectorId {
    pub const ALL: [ConnectorId; 4] = [
        ConnectorId::Soundpad,
        ConnectorId::Discord,
        ConnectorId::Voicemeeter,
        ConnectorId::Vjoy,
    ];

    pub fn name(self) -> &'static str {
        match self {
            ConnectorId::Soundpad => "Soundpad",
            ConnectorId::Discord => "Discord",
            ConnectorId::Voicemeeter => "Voicemeeter",
            ConnectorId::Vjoy => "vJoy",
        }
    }

    /// The connector an action needs, if any. Everything else is built in.
    pub fn for_action(action: &Action) -> Option<ConnectorId> {
        match action {
            Action::Soundpad { .. } => Some(ConnectorId::Soundpad),
            Action::Discord { .. } => Some(ConnectorId::Discord),
            Action::Voicemeeter { .. } => Some(ConnectorId::Voicemeeter),
            Action::Vjoy { .. } => Some(ConnectorId::Vjoy),
            Action::InputMacro(_)
            | Action::StopMacros { .. }
            | Action::SwitchProfile { .. }
            | Action::Speak(_)
            | Action::PlaySound(_)
            | Action::Volume { .. }
            | Action::MediaKey { .. }
            | Action::Launch(_)
            | Action::Http(_) => None,
        }
    }
}

/// Where a secret lives, never the secret itself.
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

/// Discord: the user's own application in the Discord Developer Portal. The client secret
/// and the OAuth tokens live in the credential store (see [`crate::discord`]).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DiscordSettings {
    pub enabled: bool,
    /// The application's ID ("Client ID"), a number; empty until entered.
    pub client_id: String,
    /// Where the client secret is stored; `None` until one was saved.
    pub client_secret: Option<SecretRef>,
}

impl DiscordSettings {
    pub fn secret_ref() -> SecretRef {
        SecretRef::WindowsCredentialManager {
            target: crate::discord::SECRET_TARGET.into(),
        }
    }
}

/// Voicemeeter: the VoicemeeterRemote API of a local installation. Nothing to configure but
/// whether it is used.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct VoicemeeterSettings {
    pub enabled: bool,
}

/// Settings of all connectors, as stored in the setup.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Connections {
    pub soundpad: SoundpadSettings,
    /// Missing in files from before the Discord connector.
    pub discord: DiscordSettings,
    /// Missing in files from before the Voicemeeter connector.
    pub voicemeeter: VoicemeeterSettings,
    /// Missing in files from before vJoy output.
    pub vjoy: VjoySettings,
}

/// vJoy output: while enabled, Trimcraft owns the vJoy devices the active profile uses.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct VjoySettings {
    pub enabled: bool,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ConnectionError {
    #[error("timeout must be between {MIN_TIMEOUT_MS} and {MAX_TIMEOUT_MS} ms")]
    BadTimeout,
    #[error("sound list age must be at most 10 minutes")]
    BadListAge,
    #[error("the Discord Client ID is a number (Developer Portal, OAuth2 page)")]
    BadClientId,
}

/// A Discord application ID: a snowflake, digits only.
pub fn valid_client_id(id: &str) -> bool {
    (15..=22).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_digit())
}

impl Connections {
    pub fn enabled(&self, id: ConnectorId) -> bool {
        match id {
            ConnectorId::Soundpad => self.soundpad.enabled,
            ConnectorId::Discord => self.discord.enabled,
            ConnectorId::Voicemeeter => self.voicemeeter.enabled,
            ConnectorId::Vjoy => self.vjoy.enabled,
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
        let id = &self.discord.client_id;
        if !id.is_empty() && !valid_client_id(id) {
            return Err(ConnectionError::BadClientId);
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
    /// The program shows a consent request the user has to approve (Discord).
    WaitingForApproval,
    Connected,
    Error {
        message: String,
    },
}

/// Which credentials a connector has in the credential store (never the values).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CredentialState {
    pub secret_saved: bool,
    /// OAuth tokens are stored.
    pub authorized: bool,
}

/// What the app knows about a connector right now.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConnectorState {
    pub status: ConnectorStatus,
    /// E.g. "26 sounds", "Connected as Sam".
    pub detail: Option<String>,
    /// The last problem running one of its actions, until one succeeds again.
    pub warning: Option<String>,
    /// Live mute/deaf state (Discord, while connected).
    pub voice: Option<VoiceState>,
    /// For connectors with credentials (Discord).
    pub credentials: Option<CredentialState>,
    /// The setup guide step that fixes the last problem.
    pub guide: Option<GuideStep>,
    /// The program the connector needs is not installed (Voicemeeter), with what to do.
    pub missing: Option<String>,
    /// Driver version and devices (vJoy).
    pub vjoy: Option<VjoyInfo>,
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
    pub voice: Option<VoiceState>,
    pub credentials: Option<CredentialState>,
    /// The setup guide step that fixes the last problem (Discord), also while disabled.
    pub guide: Option<GuideStep>,
    /// The program the connector needs is not installed; its actions are not offered. Also
    /// while disabled.
    pub missing: Option<String>,
    /// Driver version and devices (vJoy), also while disabled, so they can be checked first.
    pub vjoy: Option<VjoyInfo>,
}

impl ConnectorView {
    /// A disabled connector is "not configured" whatever was last reported. Credentials are
    /// shown either way, so they can be entered before enabling.
    pub fn new(id: ConnectorId, enabled: bool, state: Option<&ConnectorState>) -> Self {
        let credentials = state.and_then(|s| s.credentials);
        let guide = state.and_then(|s| s.guide);
        let missing = state.and_then(|s| s.missing.clone());
        let vjoy = state.and_then(|s| s.vjoy.clone());
        let (status, detail, warning, voice) = match (enabled, state) {
            (false, _) => (ConnectorStatus::NotConfigured, None, None, None),
            (true, None) => (ConnectorStatus::Connecting, None, None, None),
            (true, Some(s)) => (
                s.status.clone(),
                s.detail.clone(),
                s.warning.clone(),
                s.voice,
            ),
        };
        Self {
            id,
            name: id.name().into(),
            enabled,
            status,
            detail,
            warning,
            voice,
            credentials,
            guide,
            missing,
            vjoy,
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
            r#"{"soundpad":{"enabled":true,"timeoutMs":500,"listMaxAgeMs":10000},"discord":{"enabled":false,"clientId":"","clientSecret":null},"voicemeeter":{"enabled":false},"vjoy":{"enabled":false}}"#
        );
        let c: Connections = serde_json::from_str(
            r#"{"discord":{"enabled":true,"clientId":"1234567890123456789",
                "clientSecret":{"store":"windowsCredentialManager","target":"trimcraft/discord/client-secret"}}}"#,
        )
        .unwrap();
        assert_eq!(c.discord.client_secret, Some(DiscordSettings::secret_ref()));
        assert!(c.validate().is_ok() && c.enabled(ConnectorId::Discord));
        let secret = SecretRef::WindowsCredentialManager {
            target: "trimcraft/discord/client-secret".into(),
        };
        assert_eq!(
            serde_json::to_string(&secret).unwrap(),
            r#"{"store":"windowsCredentialManager","target":"trimcraft/discord/client-secret"}"#
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
        c.soundpad.list_max_age_ms = 0;
        c.discord.client_id = "my-app".into();
        assert_eq!(c.validate(), Err(ConnectionError::BadClientId));
        c.discord.client_id = "123".into();
        assert_eq!(c.validate(), Err(ConnectionError::BadClientId));
        c.discord.client_id = "1234567890123456789".into();
        assert_eq!(c.validate(), Ok(()));
    }

    #[test]
    fn discord_actions_are_gated() {
        use crate::discord::DiscordCommand;
        let mut c = Connections::default();
        let mute = Action::Discord {
            command: DiscordCommand::ToggleMute,
        };
        assert_eq!(ConnectorId::for_action(&mute), Some(ConnectorId::Discord));
        assert!(!c.action_enabled(&mute));
        c.soundpad.enabled = true;
        assert!(!c.action_enabled(&mute));
        c.discord.enabled = true;
        assert!(c.action_enabled(&mute));
    }

    #[test]
    fn voicemeeter_actions_are_gated_and_new_built_ins_are_not() {
        use crate::audio::{MediaKey, Speech};
        use crate::voicemeeter::{VmCommand, VmKind, VmTarget};
        let mut c = Connections::default();
        let mute = Action::Voicemeeter {
            target: VmTarget {
                kind: VmKind::Strip,
                index: 0,
            },
            command: VmCommand::ToggleMute,
        };
        assert_eq!(
            ConnectorId::for_action(&mute),
            Some(ConnectorId::Voicemeeter)
        );
        assert!(!c.action_enabled(&mute));
        c.voicemeeter.enabled = true;
        assert!(c.action_enabled(&mute));
        let speak = Action::Speak(Speech::default());
        let media = Action::MediaKey {
            key: MediaKey::PlayPause,
        };
        assert!(ConnectorId::for_action(&speak).is_none());
        assert!(Connections::default().action_enabled(&media));
        let old: Connections = serde_json::from_str(r#"{"soundpad":{"enabled":true}}"#).unwrap();
        assert!(!old.voicemeeter.enabled);
    }

    #[test]
    fn view_reflects_enabled_and_reported_state() {
        let reported = ConnectorState {
            status: ConnectorStatus::Connected,
            detail: Some("26 sounds".into()),
            ..Default::default()
        };
        // Credentials show while disabled; live voice state only while enabled.
        let discord = ConnectorState {
            status: ConnectorStatus::Connected,
            voice: Some(VoiceState {
                mute: true,
                deaf: false,
            }),
            credentials: Some(CredentialState {
                secret_saved: true,
                authorized: true,
            }),
            guide: Some(GuideStep::AppTesters),
            ..Default::default()
        };
        let off = ConnectorView::new(ConnectorId::Discord, false, Some(&discord));
        assert_eq!((off.voice, off.credentials.is_some()), (None, true));
        assert_eq!(off.guide, Some(GuideStep::AppTesters));
        let json = serde_json::to_value(ConnectorView::new(
            ConnectorId::Discord,
            true,
            Some(&discord),
        ))
        .unwrap();
        assert_eq!(json["voice"]["mute"], true);
        assert_eq!(json["credentials"]["secretSaved"], true);
        assert_eq!(
            serde_json::to_value(ConnectorStatus::WaitingForApproval).unwrap()["state"],
            "waitingForApproval"
        );
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

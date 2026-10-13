//! Discord: mute and deafen through Discord's local RPC server.
//!
//! The Discord client listens on a local IPC endpoint (`\\?\pipe\discord-ipc-N` on Windows,
//! N = 0..9). Every message is a frame: an 8-byte header (opcode and payload length, both
//! `u32` little-endian) followed by a JSON payload. After the handshake Discord sends a
//! `READY` dispatch; commands then go out as `{"cmd":…,"args":…,"nonce":…}` and the answer
//! carries the same nonce. See <https://docs.discord.com/developers/topics/rpc>.
//!
//! Voice settings need OAuth2: `AUTHORIZE` shows a consent popup in the Discord client and
//! returns a code, the app exchanges it (with the client secret) for tokens at Discord's
//! token endpoint, and `AUTHENTICATE` presents the access token. Later connections only
//! authenticate, refreshing the token when it expired. `SET_VOICE_SETTINGS` really sets
//! mute and deaf (it does not toggle), and `VOICE_SETTINGS_UPDATE` reports every change, so
//! actions can skip calls whose state already holds.
//!
//! Discord allows one app at a time to change voice settings and reverts that app's changes
//! when it disconnects, so the app keeps its connection open.
//!
//! This module is pure: the IPC endpoint and the HTTPS call are behind [`Transport`] and
//! [`TokenEndpoint`], which the app implements and the tests fake. Secrets and tokens never
//! appear in `Debug` output or error messages.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::io::Read;
use std::time::{Duration, Instant};
use thiserror::Error;

/// OAuth2 scopes Trimcraft asks for: RPC access, reading and changing voice settings.
pub const SCOPES: [&str; 3] = ["rpc", "rpc.voice.read", "rpc.voice.write"];
/// The redirect URI sent with the code exchange; must be registered in the Developer Portal.
/// Nothing is ever served there; RPC hands the code over the pipe.
pub const REDIRECT_URI: &str = "https://localhost/";
pub const TOKEN_URL: &str = "https://discord.com/api/oauth2/token";
/// Where the user creates the application (the setup guide links here).
pub const PORTAL_URL: &str = "https://discord.com/developers/applications";

/// The values the in-app setup guide shows, from the same constants the code uses.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SetupInfo {
    pub redirect_uri: &'static str,
    pub scopes: [&'static str; 3],
    pub portal_url: &'static str,
}

pub fn setup_info() -> SetupInfo {
    SetupInfo {
        redirect_uri: REDIRECT_URI,
        scopes: SCOPES,
        portal_url: PORTAL_URL,
    }
}

/// The step of the in-app setup guide that fixes a problem (the UI numbers the steps).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum GuideStep {
    ClientId,
    ClientSecret,
    Redirect,
    DiscordRunning,
    Authorize,
    AppTesters,
}
/// Discord tries `discord-ipc-0` to `discord-ipc-9`.
pub const PIPE_COUNT: u32 = 10;
/// Larger frames are refused as corrupt. Voice settings with device lists are a few KB.
pub const MAX_FRAME: u32 = 1 << 20;
/// Credential store targets (Windows Credential Manager "generic credentials").
pub const SECRET_TARGET: &str = "trimcraft/discord/client-secret";
pub const TOKENS_TARGET: &str = "trimcraft/discord/oauth-tokens";
/// A token this close to its expiry is refreshed before use.
const EXPIRY_MARGIN_S: u64 = 300;
const VOICE_SETTINGS_UPDATE: &str = "VOICE_SETTINGS_UPDATE";

/// Name of the n-th IPC pipe on Windows.
pub fn pipe_name(n: u32) -> String {
    format!(r"\\?\pipe\discord-ipc-{n}")
}

// --- Frames --------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Opcode {
    Handshake = 0,
    Frame = 1,
    Close = 2,
    Ping = 3,
    Pong = 4,
}

impl Opcode {
    pub fn from_u32(v: u32) -> Option<Self> {
        Some(match v {
            0 => Self::Handshake,
            1 => Self::Frame,
            2 => Self::Close,
            3 => Self::Ping,
            4 => Self::Pong,
            _ => return None,
        })
    }
}

pub fn encode_frame(op: Opcode, payload: &str) -> Vec<u8> {
    let len = u32::try_from(payload.len()).expect("frame fits in u32");
    let mut out = Vec::with_capacity(8 + payload.len());
    out.extend_from_slice(&(op as u32).to_le_bytes());
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(payload.as_bytes());
    out
}

/// Opcode and payload length from a frame header.
pub fn parse_header(header: [u8; 8]) -> Result<(Opcode, u32), DiscordError> {
    let op = u32::from_le_bytes([header[0], header[1], header[2], header[3]]);
    let len = u32::from_le_bytes([header[4], header[5], header[6], header[7]]);
    let op = Opcode::from_u32(op)
        .ok_or_else(|| DiscordError::Protocol(format!("unknown opcode {op}")))?;
    if len > MAX_FRAME {
        return Err(DiscordError::Protocol(format!("frame of {len} bytes")));
    }
    Ok((op, len))
}

/// Reads one whole frame. I/O errors (the pipe closed) become [`DiscordError::Disconnected`].
pub fn read_frame(r: &mut impl Read) -> Result<(Opcode, String), DiscordError> {
    let io = |e: std::io::Error| DiscordError::Disconnected(e.to_string());
    let mut header = [0u8; 8];
    r.read_exact(&mut header).map_err(io)?;
    let (op, len) = parse_header(header)?;
    let mut body = vec![0u8; len as usize];
    r.read_exact(&mut body).map_err(io)?;
    let body = String::from_utf8(body)
        .map_err(|_| DiscordError::Protocol("payload is not UTF-8".into()))?;
    Ok((op, body))
}

// --- Messages ------------------------------------------------------------------------------

pub fn handshake(client_id: &str) -> String {
    json!({ "v": 1, "client_id": client_id }).to_string()
}

/// A command payload. `evt` is only used by `SUBSCRIBE`/`UNSUBSCRIBE`.
pub fn command(cmd: &str, args: Value, evt: Option<&str>, nonce: &str) -> String {
    let mut m = json!({ "cmd": cmd, "args": args, "nonce": nonce });
    if let Some(evt) = evt {
        m["evt"] = Value::String(evt.into());
    }
    m.to_string()
}

pub fn authorize_args(client_id: &str) -> Value {
    json!({ "client_id": client_id, "scopes": SCOPES })
}

pub fn authenticate_args(access_token: &str) -> Value {
    json!({ "access_token": access_token })
}

/// What Discord sent, as far as Trimcraft cares.
#[derive(Debug, Clone, PartialEq)]
pub enum Incoming {
    Ready,
    /// The answer to a command.
    Reply {
        nonce: String,
        data: Value,
    },
    /// A command failed.
    Failure {
        nonce: Option<String>,
        cmd: String,
        code: i64,
        message: String,
    },
    /// A subscribed event.
    Event {
        evt: String,
        data: Value,
    },
}

pub fn parse_incoming(payload: &str) -> Result<Incoming, DiscordError> {
    let v: Value = serde_json::from_str(payload)
        .map_err(|e| DiscordError::Protocol(format!("invalid JSON from Discord: {e}")))?;
    let text = |k: &str| v.get(k).and_then(Value::as_str).map(str::to_owned);
    let cmd = text("cmd").unwrap_or_default();
    let evt = text("evt");
    let nonce = text("nonce");
    let data = v.get("data").cloned().unwrap_or(Value::Null);
    if evt.as_deref() == Some("ERROR") {
        return Ok(Incoming::Failure {
            nonce,
            cmd,
            code: data.get("code").and_then(Value::as_i64).unwrap_or(0),
            message: data
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .into(),
        });
    }
    if cmd == "DISPATCH" {
        let evt = evt.unwrap_or_default();
        return Ok(if evt == "READY" {
            Incoming::Ready
        } else {
            Incoming::Event { evt, data }
        });
    }
    match nonce {
        Some(nonce) => Ok(Incoming::Reply { nonce, data }),
        None => Err(DiscordError::Protocol(format!(
            "answer to {cmd} without nonce"
        ))),
    }
}

// --- Voice state and actions ---------------------------------------------------------------

/// The user's own mute and deaf state in Discord.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct VoiceState {
    pub mute: bool,
    pub deaf: bool,
}

impl VoiceState {
    /// From a `GET_VOICE_SETTINGS` / `SET_VOICE_SETTINGS` answer or a
    /// `VOICE_SETTINGS_UPDATE` event (other fields are ignored).
    pub fn from_data(data: &Value) -> Result<Self, DiscordError> {
        let flag = |k: &str| {
            data.get(k)
                .and_then(Value::as_bool)
                .ok_or_else(|| DiscordError::Protocol(format!("voice settings without {k}")))
        };
        Ok(Self {
            mute: flag("mute")?,
            deaf: flag("deaf")?,
        })
    }

    pub fn describe(self) -> String {
        let yes = |b: bool| if b { "yes" } else { "no" };
        format!(
            "mic muted: {}, deafened: {}",
            yes(self.mute),
            yes(self.deaf)
        )
    }
}

/// Fields to change with `SET_VOICE_SETTINGS`; missing fields stay as they are.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct VoicePatch {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mute: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deaf: Option<bool>,
}

/// What a Discord action does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DiscordCommand {
    Mute,
    Unmute,
    Deafen,
    Undeafen,
    /// Mute if unmuted, else unmute, by the live state.
    ToggleMute,
    ToggleDeafen,
}

impl DiscordCommand {
    pub const ALL: [DiscordCommand; 6] = [
        Self::Mute,
        Self::Unmute,
        Self::Deafen,
        Self::Undeafen,
        Self::ToggleMute,
        Self::ToggleDeafen,
    ];

    /// The change to send given the live state, or `None` when it already holds (the call
    /// is skipped).
    pub fn patch(self, live: VoiceState) -> Option<VoicePatch> {
        let (mute, deaf) = match self {
            Self::Mute => (Some(true), None),
            Self::Unmute => (Some(false), None),
            Self::ToggleMute => (Some(!live.mute), None),
            Self::Deafen => (None, Some(true)),
            Self::Undeafen => (None, Some(false)),
            Self::ToggleDeafen => (None, Some(!live.deaf)),
        };
        let mute = mute.filter(|&m| m != live.mute);
        let deaf = deaf.filter(|&d| d != live.deaf);
        (mute.is_some() || deaf.is_some()).then_some(VoicePatch { mute, deaf })
    }

    pub fn describe(self) -> &'static str {
        match self {
            Self::Mute => "mute",
            Self::Unmute => "unmute",
            Self::Deafen => "deafen",
            Self::Undeafen => "undeafen",
            Self::ToggleMute => "toggle mute",
            Self::ToggleDeafen => "toggle deafen",
        }
    }
}

// --- Secrets and tokens --------------------------------------------------------------------

/// A secret string that never shows up in `Debug` output.
#[derive(Clone, PartialEq, Eq, Default)]
pub struct Secret(String);

impl Secret {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }
    pub fn expose(&self) -> &str {
        &self.0
    }
    pub fn is_empty(&self) -> bool {
        self.0.trim().is_empty()
    }
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Secret(***)")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Credentials {
    pub client_id: String,
    pub client_secret: Secret,
}

/// OAuth2 tokens as kept in the credential store (serialised as JSON there).
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tokens {
    pub access_token: String,
    pub refresh_token: String,
    /// Unix seconds.
    pub expires_at: u64,
}

impl std::fmt::Debug for Tokens {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Tokens")
            .field("expires_at", &self.expires_at)
            .finish_non_exhaustive()
    }
}

impl Tokens {
    pub fn expired(&self, now: u64) -> bool {
        now + EXPIRY_MARGIN_S >= self.expires_at
    }
}

/// Form fields for exchanging an authorization code.
pub fn code_exchange_form(creds: &Credentials, code: &str) -> Vec<(&'static str, String)> {
    vec![
        ("grant_type", "authorization_code".into()),
        ("code", code.into()),
        ("redirect_uri", REDIRECT_URI.into()),
        ("client_id", creds.client_id.clone()),
        ("client_secret", creds.client_secret.expose().into()),
    ]
}

/// Form fields for refreshing tokens.
pub fn refresh_form(creds: &Credentials, refresh_token: &str) -> Vec<(&'static str, String)> {
    vec![
        ("grant_type", "refresh_token".into()),
        ("refresh_token", refresh_token.into()),
        ("client_id", creds.client_id.clone()),
        ("client_secret", creds.client_secret.expose().into()),
    ]
}

/// Tokens from the token endpoint's answer (HTTP status and body), or what went wrong.
/// Only the `error` and `error_description` fields of a failure are used, never the body.
pub fn parse_token_response(status: u16, body: &str, now: u64) -> Result<Tokens, DiscordError> {
    let v: Value = serde_json::from_str(body).unwrap_or(Value::Null);
    let text = |k: &str| {
        v.get(k)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    if (200..300).contains(&status) {
        let access_token = text("access_token");
        let refresh_token = text("refresh_token");
        if access_token.is_empty() || refresh_token.is_empty() {
            return Err(DiscordError::Protocol(
                "token answer without access or refresh token".into(),
            ));
        }
        let expires_in = v.get("expires_in").and_then(Value::as_u64).unwrap_or(0);
        return Ok(Tokens {
            access_token,
            refresh_token,
            expires_at: now + expires_in,
        });
    }
    let error = text("error");
    let description = text("error_description");
    Err(match error.as_str() {
        "invalid_client" => DiscordError::InvalidSecret,
        "invalid_grant" => DiscordError::TokenRejected,
        _ if description.to_lowercase().contains("redirect_uri") => DiscordError::RedirectMismatch,
        _ => {
            let detail = if description.is_empty() {
                error
            } else {
                format!("{error}: {description}")
            };
            DiscordError::Http(format!("token request failed (HTTP {status}) {detail}"))
        }
    })
}

/// Discord's OAuth2 token endpoint ([`TOKEN_URL`]); the app does the HTTPS POST.
pub trait TokenEndpoint {
    /// Posts the form (urlencoded) and returns the HTTP status and body. Network failures
    /// are [`DiscordError::Http`].
    fn post(&mut self, form: &[(&'static str, String)]) -> Result<(u16, String), DiscordError>;
}

// --- Errors --------------------------------------------------------------------------------

/// Everything that can go wrong, worded for the Connections page.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum DiscordError {
    #[error("Discord is not running (no RPC pipe found)")]
    NotRunning,
    #[error("Enter the Client ID and Client Secret of your Discord application and save them")]
    NoCredentials,
    #[error("Not authorized yet: press Test connection and approve the request in Discord")]
    NotAuthorized,
    #[error("Discord rejected the Client ID; copy it from the Developer Portal ({0})")]
    InvalidClientId(String),
    #[error(
        "Discord refused the RPC scopes for this application. Unapproved applications may only \
         use RPC for their testers: in the Developer Portal, App Testers, add your own Discord \
         account and accept the invitation ({0})"
    )]
    AppNotApproved(String),
    #[error("The request was declined in Discord ({0})")]
    Declined(String),
    #[error(
        "Discord rejected the Client Secret (or Client ID): copy both again from the Developer \
         Portal, OAuth2 page (invalid_client)"
    )]
    InvalidSecret,
    #[error("The stored authorization is no longer valid (invalid_grant)")]
    TokenRejected,
    #[error(
        "Discord rejected the redirect URI: add {REDIRECT_URI} under OAuth2, Redirects in the \
         Developer Portal (invalid_request)"
    )]
    RedirectMismatch,
    #[error(
        "Another app controls Discord's voice settings right now; Discord allows only one at a \
         time. Close it (e.g. a Stream Deck plugin) and try again ({0})"
    )]
    VoiceLocked(String),
    #[error("Discord: {message} (code {code})")]
    Rpc { code: i64, message: String },
    #[error("Discord did not answer {0} in time")]
    Timeout(&'static str),
    #[error("Discord closed the connection ({0})")]
    Disconnected(String),
    #[error("{0}")]
    Http(String),
    #[error("unexpected data from Discord: {0}")]
    Protocol(String),
}

impl DiscordError {
    /// The setup guide step to point at, if one fixes this.
    pub fn guide_step(&self) -> Option<GuideStep> {
        Some(match self {
            Self::NoCredentials | Self::InvalidClientId(_) => GuideStep::ClientId,
            Self::InvalidSecret => GuideStep::ClientSecret,
            Self::RedirectMismatch => GuideStep::Redirect,
            Self::NotRunning | Self::VoiceLocked(_) => GuideStep::DiscordRunning,
            Self::NotAuthorized | Self::Declined(_) | Self::TokenRejected => GuideStep::Authorize,
            Self::AppNotApproved(_) => GuideStep::AppTesters,
            _ => return None,
        })
    }

    /// Whether reconnecting on its own can help. Otherwise the user has to act first
    /// (credentials, approval, Developer Portal settings).
    pub fn retry(&self) -> bool {
        matches!(
            self,
            Self::NotRunning
                | Self::Timeout(_)
                | Self::Disconnected(_)
                | Self::Http(_)
                | Self::Protocol(_)
                | Self::Rpc { .. }
                | Self::VoiceLocked(_)
        )
    }
}

fn code_text(code: i64, message: &str) -> String {
    if message.is_empty() {
        format!("code {code}")
    } else {
        format!("code {code}: {message}")
    }
}

/// Maps a failed command to an error. Discord's codes are generic (5000 is any OAuth2
/// error), so the message is looked at too.
pub fn classify_failure(cmd: &str, code: i64, message: &str) -> DiscordError {
    let lower = message.to_lowercase();
    let detail = code_text(code, message);
    match cmd {
        "AUTHORIZE" if lower.contains("denied") || lower.contains("cancel") => {
            DiscordError::Declined(detail)
        }
        "AUTHORIZE" if code == 4007 => DiscordError::InvalidClientId(detail),
        // Unapproved apps get an OAuth2 or permission error mentioning the scope, depending on
        // the Discord version; every other AUTHORIZE failure is treated the same way.
        "AUTHORIZE" => DiscordError::AppNotApproved(detail),
        "AUTHENTICATE" if code == 4009 || lower.contains("token") => DiscordError::TokenRejected,
        "AUTHENTICATE" if code == 4007 => DiscordError::InvalidClientId(detail),
        "SET_VOICE_SETTINGS" if lower.contains("lock") || lower.contains("another") => {
            DiscordError::VoiceLocked(detail)
        }
        _ => DiscordError::Rpc {
            code,
            message: message.into(),
        },
    }
}

/// Maps a `CLOSE` frame (`{"code":…,"message":…}`) to an error.
pub fn classify_close(payload: &str) -> DiscordError {
    let v: Value = serde_json::from_str(payload).unwrap_or(Value::Null);
    let code = v.get("code").and_then(Value::as_i64).unwrap_or(0);
    let message = v.get("message").and_then(Value::as_str).unwrap_or_default();
    let detail = code_text(code, message);
    match code {
        4000 => DiscordError::InvalidClientId(detail),
        4003 => DiscordError::TokenRejected,
        _ => DiscordError::Disconnected(detail),
    }
}

// --- Client --------------------------------------------------------------------------------

/// One connection to Discord's IPC endpoint.
pub trait Transport {
    fn send(&mut self, frame: &[u8]) -> Result<(), DiscordError>;
    /// The next frame. `Ok(None)` when `timeout` passed, or early when the app has something
    /// else to do; callers check their own deadline.
    fn recv(&mut self, timeout: Duration) -> Result<Option<(Opcode, String)>, DiscordError>;
}

/// How long ordinary commands may take.
pub const COMMAND_TIMEOUT: Duration = Duration::from_secs(5);
/// How long the user has to approve the consent popup.
pub const APPROVAL_TIMEOUT: Duration = Duration::from_secs(120);

/// Result of `AUTHENTICATE`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    /// Display name, else user name.
    pub user: String,
    pub scopes: Vec<String>,
}

/// What running a [`DiscordCommand`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// The state already held; nothing was sent.
    Skipped(VoiceState),
    Changed(VoiceState),
}

pub struct Client<T> {
    transport: T,
    nonce: u64,
    voice: Option<VoiceState>,
    /// The voice state changed since [`Client::take_changed`].
    changed: bool,
}

impl<T: Transport> Client<T> {
    /// Handshake; returns once Discord sent `READY`.
    pub fn connect(transport: T, client_id: &str) -> Result<Self, DiscordError> {
        let mut client = Self {
            transport,
            nonce: 0,
            voice: None,
            changed: false,
        };
        client
            .transport
            .send(&encode_frame(Opcode::Handshake, &handshake(client_id)))?;
        let deadline = Instant::now() + COMMAND_TIMEOUT;
        loop {
            match client.next(deadline, "the handshake")? {
                Incoming::Ready => return Ok(client),
                Incoming::Failure { code, message, .. } => {
                    return Err(classify_failure("HANDSHAKE", code, &message));
                }
                Incoming::Event { .. } | Incoming::Reply { .. } => {}
            }
        }
    }

    /// The live voice state, once read or reported.
    pub fn voice(&self) -> Option<VoiceState> {
        self.voice
    }

    /// Whether the voice state changed since the last call.
    pub fn take_changed(&mut self) -> bool {
        std::mem::take(&mut self.changed)
    }

    fn set_voice(&mut self, voice: VoiceState) {
        self.changed |= self.voice != Some(voice);
        self.voice = Some(voice);
    }

    /// Events are applied in the order they arrive, also while waiting for an answer.
    fn on_event(&mut self, evt: &str, data: &Value) {
        if evt == VOICE_SETTINGS_UPDATE
            && let Ok(v) = VoiceState::from_data(data)
        {
            self.set_voice(v);
        }
    }

    /// The next message before `deadline`, answering pings and turning `CLOSE` into an error.
    fn next(&mut self, deadline: Instant, what: &'static str) -> Result<Incoming, DiscordError> {
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(DiscordError::Timeout(what));
            }
            let Some((op, payload)) = self.transport.recv(left)? else {
                continue;
            };
            match op {
                Opcode::Frame => return parse_incoming(&payload),
                Opcode::Ping => self.transport.send(&encode_frame(Opcode::Pong, &payload))?,
                Opcode::Close => return Err(classify_close(&payload)),
                Opcode::Pong | Opcode::Handshake => {}
            }
        }
    }

    /// Sends a command and waits for its answer.
    pub fn request(
        &mut self,
        cmd: &'static str,
        args: Value,
        evt: Option<&str>,
        timeout: Duration,
    ) -> Result<Value, DiscordError> {
        self.nonce += 1;
        let nonce = format!("trimcraft-{}", self.nonce);
        self.transport.send(&encode_frame(
            Opcode::Frame,
            &command(cmd, args, evt, &nonce),
        ))?;
        let deadline = Instant::now() + timeout;
        loop {
            match self.next(deadline, cmd)? {
                Incoming::Reply { nonce: n, data } if n == nonce => return Ok(data),
                Incoming::Failure {
                    nonce: n,
                    code,
                    message,
                    ..
                } if n.as_deref() == Some(&nonce) || n.is_none() => {
                    return Err(classify_failure(cmd, code, &message));
                }
                Incoming::Event { evt, data } => self.on_event(&evt, &data),
                _ => {}
            }
        }
    }

    /// Shows the consent popup in Discord; returns the authorization code.
    pub fn authorize(&mut self, client_id: &str) -> Result<String, DiscordError> {
        let data = self.request(
            "AUTHORIZE",
            authorize_args(client_id),
            None,
            APPROVAL_TIMEOUT,
        )?;
        data.get("code")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| DiscordError::Protocol("AUTHORIZE answer without code".into()))
    }

    pub fn authenticate(&mut self, access_token: &str) -> Result<Session, DiscordError> {
        let data = self.request(
            "AUTHENTICATE",
            authenticate_args(access_token),
            None,
            COMMAND_TIMEOUT,
        )?;
        let user = &data["user"];
        let name = ["global_name", "username"]
            .iter()
            .filter_map(|k| user.get(*k).and_then(Value::as_str))
            .find(|s| !s.is_empty())
            .unwrap_or("unknown user")
            .to_owned();
        let scopes = data
            .get("scopes")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|s| s.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default();
        Ok(Session { user: name, scopes })
    }

    /// Reads the voice settings and subscribes to their changes.
    pub fn start_voice(&mut self) -> Result<VoiceState, DiscordError> {
        self.refresh_voice()?;
        self.request(
            "SUBSCRIBE",
            json!({}),
            Some(VOICE_SETTINGS_UPDATE),
            COMMAND_TIMEOUT,
        )?;
        Ok(self.voice.unwrap_or_default())
    }

    /// Reads the voice settings again.
    pub fn refresh_voice(&mut self) -> Result<VoiceState, DiscordError> {
        let data = self.request("GET_VOICE_SETTINGS", json!({}), None, COMMAND_TIMEOUT)?;
        let voice = VoiceState::from_data(&data)?;
        self.set_voice(voice);
        Ok(voice)
    }

    /// Waits up to `timeout` for events (returns early when the transport is interrupted).
    /// See [`Client::take_changed`] for whether the voice state changed.
    pub fn poll(&mut self, timeout: Duration) -> Result<(), DiscordError> {
        let deadline = Instant::now() + timeout;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Ok(());
            }
            match self.transport.recv(left)? {
                None => return Ok(()),
                Some((Opcode::Frame, payload)) => {
                    if let Incoming::Event { evt, data } = parse_incoming(&payload)? {
                        self.on_event(&evt, &data);
                    }
                }
                Some((Opcode::Ping, payload)) => {
                    self.transport.send(&encode_frame(Opcode::Pong, &payload))?
                }
                Some((Opcode::Close, payload)) => return Err(classify_close(&payload)),
                Some(_) => {}
            }
        }
    }

    /// Runs an action against the live state; skips the call when the state already holds.
    pub fn run(&mut self, command: DiscordCommand) -> Result<Outcome, DiscordError> {
        let live = match self.voice {
            Some(v) => v,
            None => self.refresh_voice()?,
        };
        let Some(patch) = command.patch(live) else {
            return Ok(Outcome::Skipped(live));
        };
        let data = self.request(
            "SET_VOICE_SETTINGS",
            serde_json::to_value(patch).expect("patch serialises"),
            None,
            COMMAND_TIMEOUT,
        )?;
        // The answer holds the settings after the change.
        let voice = VoiceState::from_data(&data).unwrap_or(VoiceState {
            mute: patch.mute.unwrap_or(live.mute),
            deaf: patch.deaf.unwrap_or(live.deaf),
        });
        self.set_voice(voice);
        Ok(Outcome::Changed(voice))
    }
}

/// Authenticates: with the stored access token, else after refreshing it, else (only when
/// `interactive`) through the consent popup. `on_tokens` gets new tokens as soon as they
/// exist (refresh tokens are single-use, so they must be stored even if a later step fails);
/// `on_approval` is called right before the popup appears.
#[allow(clippy::too_many_arguments)]
pub fn login<T: Transport>(
    client: &mut Client<T>,
    endpoint: &mut dyn TokenEndpoint,
    creds: &Credentials,
    stored: Option<Tokens>,
    now: u64,
    interactive: bool,
    on_tokens: &mut dyn FnMut(&Tokens),
    on_approval: &mut dyn FnMut(),
) -> Result<Session, DiscordError> {
    let has_scopes = |s: &Session| SCOPES.iter().all(|w| s.scopes.iter().any(|g| g == w));
    if let Some(tokens) = stored {
        if !tokens.expired(now) {
            match client.authenticate(&tokens.access_token) {
                Ok(s) if has_scopes(&s) => return Ok(s),
                Ok(_) | Err(DiscordError::TokenRejected) => {}
                Err(e) => return Err(e),
            }
        }
        let form = refresh_form(creds, &tokens.refresh_token);
        let (status, body) = endpoint.post(&form)?;
        match parse_token_response(status, &body, now) {
            Ok(fresh) => {
                on_tokens(&fresh);
                match client.authenticate(&fresh.access_token) {
                    Ok(s) if has_scopes(&s) => return Ok(s),
                    Ok(_) | Err(DiscordError::TokenRejected) => {}
                    Err(e) => return Err(e),
                }
            }
            Err(DiscordError::TokenRejected) => {}
            Err(e) => return Err(e),
        }
    }
    if !interactive {
        return Err(DiscordError::NotAuthorized);
    }
    on_approval();
    let code = client.authorize(&creds.client_id)?;
    let (status, body) = endpoint.post(&code_exchange_form(creds, &code))?;
    let tokens = parse_token_response(status, &body, now)?;
    on_tokens(&tokens);
    let session = client.authenticate(&tokens.access_token)?;
    if !has_scopes(&session) {
        return Err(DiscordError::AppNotApproved(format!(
            "granted scopes: {}",
            session.scopes.join(" ")
        )));
    }
    Ok(session)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::rc::Rc;

    #[test]
    fn frames_round_trip() {
        let payload = handshake("123");
        assert_eq!(payload, r#"{"client_id":"123","v":1}"#);
        let frame = encode_frame(Opcode::Handshake, &payload);
        assert_eq!(&frame[..4], &[0, 0, 0, 0]);
        assert_eq!(&frame[4..8], &(payload.len() as u32).to_le_bytes());
        let mut bytes = frame.clone();
        bytes.extend(encode_frame(Opcode::Ping, "{}"));
        let mut r = std::io::Cursor::new(bytes);
        assert_eq!(read_frame(&mut r).unwrap(), (Opcode::Handshake, payload));
        assert_eq!(read_frame(&mut r).unwrap(), (Opcode::Ping, "{}".into()));
        assert!(matches!(
            read_frame(&mut r),
            Err(DiscordError::Disconnected(_))
        ));
    }

    #[test]
    fn bad_headers_are_refused() {
        let mut h = [0u8; 8];
        h[0] = 9;
        assert!(matches!(parse_header(h), Err(DiscordError::Protocol(_))));
        let mut h = [1u8, 0, 0, 0, 0, 0, 0, 0];
        h[4..].copy_from_slice(&(MAX_FRAME + 1).to_le_bytes());
        assert!(parse_header(h).is_err());
        // Truncated body.
        let mut frame = encode_frame(Opcode::Frame, "{\"a\":1}");
        frame.pop();
        assert!(read_frame(&mut std::io::Cursor::new(frame)).is_err());
        assert_eq!(pipe_name(3), r"\\?\pipe\discord-ipc-3");
    }

    #[test]
    fn message_builders() {
        let m: Value =
            serde_json::from_str(&command("AUTHORIZE", authorize_args("42"), None, "n1")).unwrap();
        assert_eq!(
            m,
            json!({"cmd":"AUTHORIZE","nonce":"n1","args":{"client_id":"42",
                "scopes":["rpc","rpc.voice.read","rpc.voice.write"]}})
        );
        let m: Value = serde_json::from_str(&command(
            "SUBSCRIBE",
            json!({}),
            Some("VOICE_SETTINGS_UPDATE"),
            "n2",
        ))
        .unwrap();
        assert_eq!(m["evt"], "VOICE_SETTINGS_UPDATE");
        assert_eq!(
            serde_json::to_value(VoicePatch {
                mute: Some(true),
                deaf: None
            })
            .unwrap(),
            json!({"mute": true})
        );
        assert_eq!(authenticate_args("t")["access_token"], "t");
    }

    #[test]
    fn incoming_messages_parse() {
        assert_eq!(
            parse_incoming(r#"{"cmd":"DISPATCH","evt":"READY","data":{"v":1},"nonce":null}"#)
                .unwrap(),
            Incoming::Ready
        );
        assert_eq!(
            parse_incoming(
                r#"{"cmd":"AUTHORIZE","evt":"ERROR","nonce":"n","data":{"code":5000,"message":"OAuth2 Error: invalid_scope"}}"#
            )
            .unwrap(),
            Incoming::Failure {
                nonce: Some("n".into()),
                cmd: "AUTHORIZE".into(),
                code: 5000,
                message: "OAuth2 Error: invalid_scope".into()
            }
        );
        let Incoming::Event { evt, data } = parse_incoming(
            r#"{"cmd":"DISPATCH","evt":"VOICE_SETTINGS_UPDATE","data":{"mute":true,"deaf":false,"input":{}}}"#,
        )
        .unwrap() else {
            panic!()
        };
        assert_eq!(evt, "VOICE_SETTINGS_UPDATE");
        assert_eq!(
            VoiceState::from_data(&data).unwrap(),
            VoiceState {
                mute: true,
                deaf: false
            }
        );
        assert!(parse_incoming("not json").is_err());
        assert!(parse_incoming(r#"{"cmd":"GET_VOICE_SETTINGS","data":{}}"#).is_err());
    }

    #[test]
    fn commands_are_idempotent() {
        let s = |mute, deaf| VoiceState { mute, deaf };
        let p = |mute, deaf| Some(VoicePatch { mute, deaf });
        assert_eq!(
            DiscordCommand::Mute.patch(s(false, false)),
            p(Some(true), None)
        );
        assert_eq!(DiscordCommand::Mute.patch(s(true, false)), None);
        assert_eq!(DiscordCommand::Unmute.patch(s(false, true)), None);
        assert_eq!(
            DiscordCommand::Unmute.patch(s(true, true)),
            p(Some(false), None)
        );
        assert_eq!(DiscordCommand::Deafen.patch(s(true, true)), None);
        assert_eq!(
            DiscordCommand::Deafen.patch(s(true, false)),
            p(None, Some(true))
        );
        assert_eq!(DiscordCommand::Undeafen.patch(s(false, false)), None);
        assert_eq!(
            DiscordCommand::Undeafen.patch(s(true, true)),
            p(None, Some(false))
        );
        assert_eq!(
            DiscordCommand::ToggleMute.patch(s(true, false)),
            p(Some(false), None)
        );
        assert_eq!(
            DiscordCommand::ToggleMute.patch(s(false, false)),
            p(Some(true), None)
        );
        assert_eq!(
            DiscordCommand::ToggleDeafen.patch(s(false, true)),
            p(None, Some(false))
        );
        assert_eq!(
            DiscordCommand::ToggleDeafen.patch(s(false, false)),
            p(None, Some(true))
        );
        assert_eq!(
            serde_json::to_string(&DiscordCommand::ALL).unwrap(),
            r#"["mute","unmute","deafen","undeafen","toggleMute","toggleDeafen"]"#
        );
        assert_eq!(s(true, false).describe(), "mic muted: yes, deafened: no");
    }

    #[test]
    fn token_responses() {
        let creds = Credentials {
            client_id: "42".into(),
            client_secret: Secret::new("s3cret"),
        };
        let t = parse_token_response(
            200,
            r#"{"access_token":"a","refresh_token":"r","expires_in":604800,"token_type":"Bearer"}"#,
            1000,
        )
        .unwrap();
        assert_eq!(t.expires_at, 605_800);
        assert!(!t.expired(1000) && t.expired(605_600));
        assert_eq!(
            parse_token_response(401, r#"{"error":"invalid_client"}"#, 0),
            Err(DiscordError::InvalidSecret)
        );
        assert_eq!(
            parse_token_response(400, r#"{"error":"invalid_grant"}"#, 0),
            Err(DiscordError::TokenRejected)
        );
        assert_eq!(
            parse_token_response(
                400,
                r#"{"error":"invalid_request","error_description":"Invalid \"redirect_uri\" in request."}"#,
                0
            ),
            Err(DiscordError::RedirectMismatch)
        );
        assert!(matches!(
            parse_token_response(500, "<html>", 0),
            Err(DiscordError::Http(_))
        ));
        let form = code_exchange_form(&creds, "c");
        assert!(form.contains(&("redirect_uri", REDIRECT_URI.into())));
        assert!(form.contains(&("grant_type", "authorization_code".into())));
        assert!(refresh_form(&creds, "r").contains(&("refresh_token", "r".into())));
        // Secrets stay out of debug output.
        let dbg = format!("{creds:?} {t:?}");
        assert!(!dbg.contains("s3cret") && !dbg.contains("\"a\"") && !dbg.contains("\"r\""));
    }

    #[test]
    fn failures_are_classified() {
        assert!(matches!(
            classify_failure("AUTHORIZE", 5000, "OAuth2 Error: invalid_scope"),
            DiscordError::AppNotApproved(_)
        ));
        assert!(matches!(
            classify_failure("AUTHORIZE", 5000, "OAuth2 Error: access_denied"),
            DiscordError::Declined(_)
        ));
        assert_eq!(
            classify_failure("AUTHENTICATE", 4009, "Invalid token"),
            DiscordError::TokenRejected
        );
        assert!(matches!(
            classify_failure(
                "SET_VOICE_SETTINGS",
                4006,
                "voice settings locked by another app"
            ),
            DiscordError::VoiceLocked(_)
        ));
        let e = classify_failure("GET_VOICE_SETTINGS", 4006, "Not authenticated");
        assert_eq!(e.to_string(), "Discord: Not authenticated (code 4006)");
        let e = classify_close(r#"{"code":4000,"message":"Invalid Client ID"}"#);
        assert!(e.to_string().contains("(code 4000: Invalid Client ID)"));
        assert!(!e.retry());
        assert!(DiscordError::NotRunning.retry());
        assert!(!DiscordError::NotAuthorized.retry());
        assert!(
            DiscordError::AppNotApproved("x".into())
                .to_string()
                .contains("App Testers")
        );
        // Errors point at the guide step that fixes them.
        assert_eq!(
            DiscordError::AppNotApproved("x".into()).guide_step(),
            Some(GuideStep::AppTesters)
        );
        assert_eq!(
            DiscordError::InvalidSecret.guide_step(),
            Some(GuideStep::ClientSecret)
        );
        assert_eq!(
            DiscordError::RedirectMismatch.guide_step(),
            Some(GuideStep::Redirect)
        );
        assert_eq!(
            DiscordError::NotRunning.guide_step(),
            Some(GuideStep::DiscordRunning)
        );
        assert_eq!(DiscordError::Timeout("x").guide_step(), None);
        let info = serde_json::to_value(setup_info()).unwrap();
        assert_eq!(info["redirectUri"], "https://localhost/");
        assert_eq!(info["scopes"][2], "rpc.voice.write");
        assert_eq!(
            serde_json::to_value(GuideStep::AppTesters).unwrap(),
            "appTesters"
        );
    }

    // --- Fake Discord ------------------------------------------------------------------

    /// What the fake Discord knows and what it was asked.
    #[derive(Default)]
    struct World {
        voice: VoiceState,
        valid_token: Option<String>,
        approve: bool,
        sent: Vec<String>,
        /// Frames to deliver before answering the next command.
        inject: Vec<(Opcode, String)>,
        locked: bool,
        bad_client: bool,
    }

    struct Fake {
        world: Rc<RefCell<World>>,
        inbox: VecDeque<(Opcode, String)>,
    }

    impl Transport for Fake {
        fn send(&mut self, frame: &[u8]) -> Result<(), DiscordError> {
            let (op, len) = parse_header(frame[..8].try_into().unwrap()).unwrap();
            assert_eq!(len as usize, frame.len() - 8);
            let payload = std::str::from_utf8(&frame[8..]).unwrap();
            let mut w = self.world.borrow_mut();
            let v: Value = serde_json::from_str(payload).unwrap();
            if op == Opcode::Pong {
                w.sent.push("PONG".into());
                return Ok(());
            }
            if op == Opcode::Handshake {
                w.sent.push("HANDSHAKE".into());
                if w.bad_client {
                    self.inbox.push_back((
                        Opcode::Close,
                        r#"{"code":4000,"message":"Invalid Client ID"}"#.into(),
                    ));
                } else {
                    self.inbox.push_back((
                        Opcode::Frame,
                        r#"{"cmd":"DISPATCH","evt":"READY","data":{"v":1}}"#.into(),
                    ));
                }
                return Ok(());
            }
            let cmd = v["cmd"].as_str().unwrap().to_owned();
            let nonce = v["nonce"].as_str().unwrap().to_owned();
            w.sent.push(cmd.clone());
            self.inbox.extend(w.inject.drain(..));
            let ok = |data: Value| json!({"cmd": cmd, "nonce": nonce, "data": data, "evt": null});
            let err = |code: i64, msg: &str| {
                json!({"cmd": cmd, "nonce": nonce, "evt": "ERROR",
                       "data": {"code": code, "message": msg}})
            };
            let voice = |v: VoiceState| json!({"mute": v.mute, "deaf": v.deaf, "mode": {}});
            let reply = match cmd.as_str() {
                "AUTHORIZE" if w.approve => ok(json!({"code": "the-code"})),
                "AUTHORIZE" => err(5000, "OAuth2 Error: access_denied"),
                "AUTHENTICATE" => {
                    if w.valid_token.as_deref() == v["args"]["access_token"].as_str() {
                        ok(json!({"user": {"username": "sam", "global_name": "Sam"},
                                  "scopes": SCOPES}))
                    } else {
                        err(4009, "Invalid access token")
                    }
                }
                "GET_VOICE_SETTINGS" => ok(voice(w.voice)),
                "SET_VOICE_SETTINGS" if w.locked => err(4006, "Voice settings locked"),
                "SET_VOICE_SETTINGS" => {
                    if let Some(m) = v["args"]["mute"].as_bool() {
                        w.voice.mute = m;
                    }
                    if let Some(d) = v["args"]["deaf"].as_bool() {
                        w.voice.deaf = d;
                    }
                    ok(voice(w.voice))
                }
                "SUBSCRIBE" => ok(json!({"evt": v["evt"]})),
                _ => err(4002, "Invalid command"),
            };
            self.inbox.push_back((Opcode::Frame, reply.to_string()));
            Ok(())
        }

        fn recv(&mut self, _: Duration) -> Result<Option<(Opcode, String)>, DiscordError> {
            // Nothing queued: as if the app interrupted the wait.
            Ok(self.inbox.pop_front())
        }
    }

    /// The token endpoint: knows one code and one refresh token.
    struct Endpoint {
        world: Rc<RefCell<World>>,
        secret_ok: bool,
        refresh_ok: bool,
        posts: Vec<String>,
    }

    impl TokenEndpoint for Endpoint {
        fn post(&mut self, form: &[(&'static str, String)]) -> Result<(u16, String), DiscordError> {
            let get = |k: &str| form.iter().find(|(n, _)| *n == k).map(|(_, v)| v.clone());
            let grant = get("grant_type").unwrap();
            self.posts.push(grant.clone());
            if !self.secret_ok {
                return Ok((401, r#"{"error":"invalid_client"}"#.into()));
            }
            let ok = match grant.as_str() {
                "authorization_code" => get("code").as_deref() == Some("the-code"),
                _ => self.refresh_ok,
            };
            if !ok {
                return Ok((400, r#"{"error":"invalid_grant"}"#.into()));
            }
            let access = format!("access-{}", self.posts.len());
            self.world.borrow_mut().valid_token = Some(access.clone());
            Ok((
                200,
                json!({"access_token": access, "refresh_token": "refresh-2", "expires_in": 3600})
                    .to_string(),
            ))
        }
    }

    fn setup(world: World) -> (Rc<RefCell<World>>, Client<Fake>, Endpoint) {
        let world = Rc::new(RefCell::new(world));
        let fake = Fake {
            world: world.clone(),
            inbox: VecDeque::new(),
        };
        let client = Client::connect(fake, "42").unwrap();
        let endpoint = Endpoint {
            world: world.clone(),
            secret_ok: true,
            refresh_ok: true,
            posts: vec![],
        };
        (world, client, endpoint)
    }

    fn creds() -> Credentials {
        Credentials {
            client_id: "42".into(),
            client_secret: Secret::new("secret"),
        }
    }

    fn tokens(access: &str, expires_at: u64) -> Tokens {
        Tokens {
            access_token: access.into(),
            refresh_token: "refresh-1".into(),
            expires_at,
        }
    }

    /// Runs `login`, returning the result, the tokens handed out and whether the popup came.
    fn run_login(
        client: &mut Client<Fake>,
        endpoint: &mut Endpoint,
        stored: Option<Tokens>,
        interactive: bool,
    ) -> (Result<Session, DiscordError>, Vec<Tokens>, bool) {
        let mut saved = vec![];
        let mut popup = false;
        let r = login(
            client,
            endpoint,
            &creds(),
            stored,
            1000,
            interactive,
            &mut |t| saved.push(t.clone()),
            &mut || popup = true,
        );
        (r, saved, popup)
    }

    #[test]
    fn first_login_authorizes_and_exchanges_the_code() {
        let (world, mut client, mut endpoint) = setup(World {
            approve: true,
            ..Default::default()
        });
        let (r, saved, popup) = run_login(&mut client, &mut endpoint, None, true);
        assert_eq!(r.unwrap().user, "Sam");
        assert!(popup);
        assert_eq!(saved.len(), 1);
        assert_eq!(saved[0].expires_at, 4600);
        assert_eq!(endpoint.posts, ["authorization_code"]);
        assert_eq!(
            world.borrow().sent,
            ["HANDSHAKE", "AUTHORIZE", "AUTHENTICATE"]
        );
    }

    #[test]
    fn stored_token_needs_no_popup() {
        let (world, mut client, mut endpoint) = setup(World {
            valid_token: Some("good".into()),
            ..Default::default()
        });
        let (r, saved, popup) = run_login(
            &mut client,
            &mut endpoint,
            Some(tokens("good", 99_999)),
            false,
        );
        assert!(r.is_ok() && saved.is_empty() && !popup);
        assert!(endpoint.posts.is_empty());
        assert_eq!(world.borrow().sent, ["HANDSHAKE", "AUTHENTICATE"]);
    }

    #[test]
    fn expired_or_rejected_token_is_refreshed() {
        // Expired: refreshed without trying it.
        let (world, mut client, mut endpoint) = setup(World::default());
        let (r, saved, popup) =
            run_login(&mut client, &mut endpoint, Some(tokens("old", 500)), false);
        assert!(r.is_ok() && !popup);
        assert_eq!(saved[0].refresh_token, "refresh-2");
        assert_eq!(endpoint.posts, ["refresh_token"]);
        assert_eq!(world.borrow().sent, ["HANDSHAKE", "AUTHENTICATE"]);
        // Valid by date but revoked: rejected, then refreshed.
        let (world, mut client, mut endpoint) = setup(World::default());
        let (r, saved, _) = run_login(
            &mut client,
            &mut endpoint,
            Some(tokens("revoked", 99_999)),
            false,
        );
        assert!(r.is_ok());
        assert_eq!(saved.len(), 1);
        assert_eq!(
            world.borrow().sent,
            ["HANDSHAKE", "AUTHENTICATE", "AUTHENTICATE"]
        );
    }

    #[test]
    fn failed_refresh_falls_back_to_the_popup_only_when_interactive() {
        let (_, mut client, mut endpoint) = setup(World {
            approve: true,
            ..Default::default()
        });
        endpoint.refresh_ok = false;
        let (r, saved, popup) =
            run_login(&mut client, &mut endpoint, Some(tokens("old", 500)), false);
        assert_eq!(r, Err(DiscordError::NotAuthorized));
        assert!(saved.is_empty() && !popup);
        let (r, saved, popup) =
            run_login(&mut client, &mut endpoint, Some(tokens("old", 500)), true);
        assert!(r.is_ok() && popup);
        assert_eq!(saved.len(), 1);
        assert_eq!(
            endpoint.posts,
            ["refresh_token", "refresh_token", "authorization_code"]
        );
    }

    #[test]
    fn login_errors() {
        // Wrong secret: refresh fails with invalid_client; no popup follows.
        let (_, mut client, mut endpoint) = setup(World {
            approve: true,
            ..Default::default()
        });
        endpoint.secret_ok = false;
        let (r, _, popup) = run_login(&mut client, &mut endpoint, Some(tokens("old", 500)), true);
        assert_eq!(r, Err(DiscordError::InvalidSecret));
        assert!(!popup);
        // Wrong secret on the first login: after the popup.
        let (r, saved, popup) = run_login(&mut client, &mut endpoint, None, true);
        assert_eq!(r, Err(DiscordError::InvalidSecret));
        assert!(popup && saved.is_empty());
        // Declined.
        let (_, mut client, mut endpoint) = setup(World::default());
        let (r, _, _) = run_login(&mut client, &mut endpoint, None, true);
        assert!(matches!(r, Err(DiscordError::Declined(_))));
        // Nothing stored and not interactive.
        let (r, _, popup) = run_login(&mut client, &mut endpoint, None, false);
        assert_eq!(r, Err(DiscordError::NotAuthorized));
        assert!(!popup);
        // Invalid client id: Discord closes during the handshake.
        let world = Rc::new(RefCell::new(World {
            bad_client: true,
            ..Default::default()
        }));
        let fake = Fake {
            world,
            inbox: VecDeque::new(),
        };
        assert!(matches!(
            Client::connect(fake, "1"),
            Err(DiscordError::InvalidClientId(_))
        ));
    }

    #[test]
    fn actions_skip_calls_whose_state_holds() {
        let (world, mut client, _) = setup(World::default());
        assert_eq!(client.start_voice().unwrap(), VoiceState::default());
        world.borrow_mut().sent.clear();
        assert_eq!(
            client.run(DiscordCommand::Unmute).unwrap(),
            Outcome::Skipped(VoiceState::default())
        );
        assert!(world.borrow().sent.is_empty());
        assert_eq!(
            client.run(DiscordCommand::ToggleMute).unwrap(),
            Outcome::Changed(VoiceState {
                mute: true,
                deaf: false
            })
        );
        assert!(matches!(
            client.run(DiscordCommand::Mute).unwrap(),
            Outcome::Skipped(_)
        ));
        assert_eq!(world.borrow().sent, ["SET_VOICE_SETTINGS"]);
        // Unmuted and deafened by hand in Discord: the event updates the live state, so
        // "unmute" then sends nothing.
        world.borrow_mut().voice = VoiceState {
            mute: false,
            deaf: true,
        };
        client.transport.inbox.push_back((
            Opcode::Frame,
            r#"{"cmd":"DISPATCH","evt":"VOICE_SETTINGS_UPDATE","data":{"mute":false,"deaf":true}}"#
                .into(),
        ));
        client.poll(Duration::from_secs(1)).unwrap();
        assert!(client.take_changed());
        world.borrow_mut().sent.clear();
        assert!(matches!(
            client.run(DiscordCommand::Unmute).unwrap(),
            Outcome::Skipped(_)
        ));
        assert_eq!(
            client.run(DiscordCommand::Undeafen).unwrap(),
            Outcome::Changed(VoiceState::default())
        );
        assert_eq!(world.borrow().sent, ["SET_VOICE_SETTINGS"]);
        // An event that arrives before an answer is older than the answer.
        world.borrow_mut().inject.push((
            Opcode::Frame,
            r#"{"cmd":"DISPATCH","evt":"VOICE_SETTINGS_UPDATE","data":{"mute":true,"deaf":false}}"#
                .into(),
        ));
        assert_eq!(client.refresh_voice(), Ok(VoiceState::default()));
        assert_eq!(client.voice(), Some(VoiceState::default()));
        // Lock held by another app.
        world.borrow_mut().locked = true;
        assert!(matches!(
            client.run(DiscordCommand::Deafen),
            Err(DiscordError::VoiceLocked(_))
        ));
    }

    #[test]
    fn poll_tracks_events_and_answers_pings() {
        let (world, mut client, _) = setup(World::default());
        client.start_voice().unwrap();
        client.take_changed();
        let inbox = &mut client.transport.inbox;
        inbox.push_back((Opcode::Ping, "{\"x\":1}".into()));
        inbox.push_back((
            Opcode::Frame,
            r#"{"cmd":"DISPATCH","evt":"VOICE_SETTINGS_UPDATE","data":{"mute":true,"deaf":false}}"#
                .into(),
        ));
        client.poll(Duration::from_secs(1)).unwrap();
        assert!(client.take_changed());
        assert!(client.voice().unwrap().mute);
        client.poll(Duration::from_secs(1)).unwrap();
        assert!(!client.take_changed());
        assert!(world.borrow().sent.contains(&"PONG".to_owned()));
        client
            .transport
            .inbox
            .push_back((Opcode::Close, r#"{"code":1000,"message":"bye"}"#.into()));
        assert!(matches!(
            client.poll(Duration::from_secs(1)),
            Err(DiscordError::Disconnected(_))
        ));
    }
}

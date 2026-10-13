//! Sound actions: speech (text to speech), playing a sound file, Windows audio volume (master
//! or per app) and media keys. The app does the playing; this module describes the actions,
//! checks them and computes volume changes.

use serde::{Deserialize, Serialize};

/// Speech rate range (SAPI: -10 slowest, 0 normal, 10 fastest).
pub const MIN_RATE: i32 = -10;
pub const MAX_RATE: i32 = 10;
/// Upper bound for spoken text, so a pasted document cannot block speech for hours.
pub const MAX_SPEECH_CHARS: usize = 2_000;

/// An installed Windows voice.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceRef {
    /// The voice's token id (registry path); what the app looks it up by.
    pub id: String,
    /// Display name, e.g. "Microsoft Zira Desktop - English (United States)".
    pub name: String,
}

/// Speaks a text with Windows speech. A new speech interrupts the one still speaking.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Speech {
    /// May contain placeholders (`{state}`, ...).
    pub text: String,
    /// `None`: the Windows default voice.
    pub voice: Option<VoiceRef>,
    /// -10..=10, 0 is normal.
    pub rate: i32,
    /// 0..=100.
    pub volume: u8,
    /// The action queue waits until the text is spoken.
    pub wait: bool,
}

impl Default for Speech {
    fn default() -> Self {
        Self {
            text: String::new(),
            voice: None,
            rate: 0,
            volume: 100,
            wait: false,
        }
    }
}

/// Plays a sound file (wav, mp3, ogg, flac).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SoundFile {
    pub path: String,
    /// 0..=100.
    pub volume: u8,
    /// Output device by name; `None` is the Windows default device.
    pub device: Option<String>,
    /// Stops the sounds Detent is still playing first.
    pub stop_previous: bool,
    /// The action queue waits until the sound has played.
    pub wait: bool,
}

impl Default for SoundFile {
    fn default() -> Self {
        Self {
            path: String::new(),
            volume: 100,
            device: None,
            stop_previous: false,
            wait: false,
        }
    }
}

/// Whose volume a volume action changes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "type"
)]
pub enum VolumeTarget {
    /// The default output device.
    Master,
    /// Every audio session of a program, by executable name (e.g. `Discord.exe`), on every
    /// output device.
    App { process: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "type"
)]
pub enum VolumeCommand {
    /// Set to a percentage, 0..=100.
    Set {
        percent: u8,
    },
    /// Change by percentage points, -100..=100.
    Change {
        delta: i8,
    },
    Mute,
    Unmute,
    ToggleMute,
}

/// What a [`VolumeCommand`] changes: the new level (0.0..=1.0) and/or the new mute state.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VolumeChange {
    pub level: Option<f32>,
    pub mute: Option<bool>,
}

impl VolumeCommand {
    /// The change for a session or device at `level` (0.0..=1.0), currently `muted`.
    pub fn apply(self, level: f32, muted: bool) -> VolumeChange {
        let set = |l: f32| VolumeChange {
            level: Some(l.clamp(0.0, 1.0)),
            mute: None,
        };
        let mute = |m: bool| VolumeChange {
            level: None,
            mute: Some(m),
        };
        match self {
            VolumeCommand::Set { percent } => set(f32::from(percent) / 100.0),
            // Rounded to whole percent, so repeated steps land on round values.
            VolumeCommand::Change { delta } => {
                set(((level * 100.0).round() + f32::from(delta)) / 100.0)
            }
            VolumeCommand::Mute => mute(true),
            VolumeCommand::Unmute => mute(false),
            VolumeCommand::ToggleMute => mute(!muted),
        }
    }

    pub fn describe(self) -> String {
        match self {
            VolumeCommand::Set { percent } => format!("set {percent}%"),
            VolumeCommand::Change { delta } => format!("{delta:+}%"),
            VolumeCommand::Mute => "mute".into(),
            VolumeCommand::Unmute => "unmute".into(),
            VolumeCommand::ToggleMute => "toggle mute".into(),
        }
    }
}

impl VolumeTarget {
    pub fn describe(&self) -> String {
        match self {
            VolumeTarget::Master => "master".into(),
            VolumeTarget::App { process } => process.clone(),
        }
    }
}

/// Media and volume keys, sent like a keyboard's media keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MediaKey {
    PlayPause,
    Next,
    Previous,
    Stop,
    VolumeUp,
    VolumeDown,
    Mute,
}

impl MediaKey {
    /// The Windows virtual-key code (`VK_MEDIA_*`, `VK_VOLUME_*`).
    pub fn vk(self) -> u16 {
        match self {
            MediaKey::PlayPause => 0xB3,
            MediaKey::Next => 0xB0,
            MediaKey::Previous => 0xB1,
            MediaKey::Stop => 0xB2,
            MediaKey::VolumeUp => 0xAF,
            MediaKey::VolumeDown => 0xAE,
            MediaKey::Mute => 0xAD,
        }
    }

    pub fn describe(self) -> &'static str {
        match self {
            MediaKey::PlayPause => "play/pause",
            MediaKey::Next => "next track",
            MediaKey::Previous => "previous track",
            MediaKey::Stop => "stop",
            MediaKey::VolumeUp => "volume up",
            MediaKey::VolumeDown => "volume down",
            MediaKey::Mute => "mute",
        }
    }
}

/// The file name of a path, for descriptions.
pub fn file_name(path: &str) -> &str {
    path.rsplit(['\\', '/']).next().unwrap_or(path)
}

/// Short text for logs, at most `max` characters.
pub fn shorten(text: &str, max: usize) -> String {
    let mut out: String = text.chars().take(max).collect();
    if text.chars().count() > max {
        out.push('…');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn volume_changes() {
        let set = VolumeCommand::Set { percent: 40 }.apply(0.9, false);
        assert_eq!(set.level, Some(0.4));
        assert_eq!(set.mute, None);
        let up = VolumeCommand::Change { delta: 10 }.apply(0.954, true);
        assert_eq!(up.level, Some(1.0));
        let down = VolumeCommand::Change { delta: -10 }.apply(0.351, false);
        assert!((down.level.unwrap() - 0.25).abs() < 1e-6);
        assert_eq!(
            VolumeCommand::Change { delta: -50 }.apply(0.2, false).level,
            Some(0.0)
        );
        assert_eq!(VolumeCommand::ToggleMute.apply(0.5, true).mute, Some(false));
        assert_eq!(VolumeCommand::ToggleMute.apply(0.5, false).mute, Some(true));
        assert_eq!(VolumeCommand::Mute.apply(0.5, true).mute, Some(true));
        assert_eq!(VolumeCommand::Unmute.apply(0.5, true).level, None);
    }

    #[test]
    fn serialises_for_the_ui() {
        let json = serde_json::to_value(VolumeTarget::App {
            process: "Discord.exe".into(),
        })
        .unwrap();
        assert_eq!(json["type"], "app");
        assert_eq!(json["process"], "Discord.exe");
        assert_eq!(
            serde_json::to_value(VolumeCommand::Change { delta: -5 }).unwrap()["delta"],
            -5
        );
        assert_eq!(
            serde_json::to_value(MediaKey::PlayPause).unwrap(),
            "playPause"
        );
        let speech: Speech = serde_json::from_str(r#"{"text":"hi"}"#).unwrap();
        assert_eq!((speech.rate, speech.volume, speech.wait), (0, 100, false));
        let sound: SoundFile =
            serde_json::from_str(r#"{"path":"a.wav","stopPrevious":true}"#).unwrap();
        assert!(sound.stop_previous && sound.device.is_none() && sound.volume == 100);
    }

    #[test]
    fn names_for_descriptions() {
        assert_eq!(file_name(r"C:\Sounds\siren.mp3"), "siren.mp3");
        assert_eq!(file_name("siren.mp3"), "siren.mp3");
        assert_eq!(shorten("abcdef", 3), "abc…");
        assert_eq!(shorten("abc", 3), "abc");
        assert_eq!(MediaKey::PlayPause.vk(), 0xB3);
    }
}

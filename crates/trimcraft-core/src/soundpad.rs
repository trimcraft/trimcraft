//! Soundpad (Leppsoft) remote control: the actions, the request strings and the sound list.
//!
//! Soundpad listens on the named pipe `\\.\pipe\sp_remote_control` for plain-text requests
//! such as `DoPlaySound(5, true, false)` and answers with `R-200` style status codes or, for
//! queries, with data (`GetSoundlist()` returns XML). The pipe itself lives in the app; this
//! module is pure. Command set and parameters follow Leppsoft's reference client
//! (`SoundpadRemoteControl.java`, remote control version 1.1.2).
//!
//! Sounds are stored by title and file path, never by index: Soundpad's indices shift
//! whenever the list changes. The index is looked up in the current sound list right before
//! playing (see [`resolve`]).

use serde::{Deserialize, Serialize};

/// Name of Soundpad's remote control pipe.
pub const PIPE: &str = r"\\.\pipe\sp_remote_control";
pub const IS_ALIVE: &str = "IsAlive()";
pub const GET_SOUNDLIST: &str = "GetSoundlist()";

/// A sound in Soundpad's list, identified the way it is stored in actions.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SoundRef {
    pub title: String,
    /// File path as Soundpad reports it.
    pub path: String,
}

impl SoundRef {
    pub fn is_empty(&self) -> bool {
        self.title.trim().is_empty() && self.path.trim().is_empty()
    }
}

/// Where a sound is played: speakers (Soundpad's "render line", so you hear it), the
/// microphone ("capture line", so others hear it), or both.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SoundOutput {
    #[default]
    Both,
    Speakers,
    Mic,
}

impl SoundOutput {
    /// `(render line, capture line)`.
    fn lines(self) -> (bool, bool) {
        match self {
            Self::Both => (true, true),
            Self::Speakers => (true, false),
            Self::Mic => (false, true),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "type"
)]
pub enum SoundpadCommand {
    Play {
        sound: SoundRef,
        #[serde(default)]
        output: SoundOutput,
    },
    /// A random sound from all categories.
    PlayRandom {
        #[serde(default)]
        output: SoundOutput,
    },
    /// Next and previous in the list, on the same output as the last sound.
    PlayNext,
    PlayPrevious,
    Stop,
    TogglePause,
}

impl SoundpadCommand {
    /// The request to send. `index` is the 1-based position of the sound in the current list;
    /// only [`SoundpadCommand::Play`] needs it, and gets `None` back without one.
    pub fn request(&self, index: Option<u32>) -> Option<String> {
        Some(match self {
            Self::Play { output, .. } => {
                let (render, capture) = output.lines();
                format!("DoPlaySound({}, {render}, {capture})", index?)
            }
            Self::PlayRandom { output } => {
                let (render, capture) = output.lines();
                format!("DoPlayRandomSound({render}, {capture})")
            }
            Self::PlayNext => "DoPlayNextSound()".into(),
            Self::PlayPrevious => "DoPlayPreviousSound()".into(),
            Self::Stop => "DoStopSound()".into(),
            Self::TogglePause => "DoTogglePause()".into(),
        })
    }

    pub fn describe(&self) -> String {
        let output = |o: &SoundOutput| match o {
            SoundOutput::Both => "",
            SoundOutput::Speakers => " (speakers)",
            SoundOutput::Mic => " (mic)",
        };
        match self {
            Self::Play { sound, output: o } => format!("play \"{}\"{}", sound.title, output(o)),
            Self::PlayRandom { output: o } => format!("play random{}", output(o)),
            Self::PlayNext => "play next".into(),
            Self::PlayPrevious => "play previous".into(),
            Self::Stop => "stop".into(),
            Self::TogglePause => "pause/resume".into(),
        }
    }
}

/// Soundpad answers `R-200` (sometimes with text after it) when a command succeeded.
pub fn is_success(response: &str) -> bool {
    response.trim_start().starts_with("R-200")
}

/// One entry of Soundpad's sound list ("All sounds").
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Sound {
    /// 1-based, as Soundpad expects it in `DoPlaySound`.
    pub index: u32,
    pub title: String,
    pub path: String,
    /// As Soundpad formats it, e.g. "0:13".
    pub duration: String,
}

/// Parses the XML of `GetSoundlist()`: `<Sound index="1" url="C:\…" title="…" …/>` elements.
/// Entries without an index are skipped; nothing else in the document matters.
pub fn parse_soundlist(xml: &str) -> Vec<Sound> {
    let mut out = Vec::new();
    let mut rest = xml;
    while let Some(start) = rest.find("<Sound ") {
        rest = &rest[start + "<Sound ".len()..];
        let end = rest.find('>').unwrap_or(rest.len());
        let attrs = parse_attributes(&rest[..end]);
        rest = &rest[end..];
        let get = |name: &str| {
            attrs
                .iter()
                .find(|(n, _)| n == name)
                .map(|(_, v)| v.clone())
                .unwrap_or_default()
        };
        let Ok(index) = get("index").parse::<u32>() else {
            continue;
        };
        out.push(Sound {
            index,
            title: get("title"),
            path: get("url"),
            duration: get("duration"),
        });
    }
    out
}

/// `name="value"` pairs (single or double quotes), values unescaped.
fn parse_attributes(s: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut rest = s;
    while let Some(eq) = rest.find('=') {
        let name = rest[..eq].trim().to_owned();
        let after = rest[eq + 1..].trim_start();
        let Some(quote) = after.chars().next().filter(|c| *c == '"' || *c == '\'') else {
            break;
        };
        let value_start = &after[1..];
        let Some(close) = value_start.find(quote) else {
            break;
        };
        out.push((name, unescape(&value_start[..close])));
        rest = &value_start[close + 1..];
    }
    out
}

fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let tail = &rest[amp..];
        let Some(semi) = tail.find(';') else {
            out.push_str(tail);
            return out;
        };
        let entity = &tail[1..semi];
        let decoded = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ => entity
                .strip_prefix("#x")
                .or_else(|| entity.strip_prefix("#X"))
                .and_then(|h| u32::from_str_radix(h, 16).ok())
                .or_else(|| entity.strip_prefix('#').and_then(|d| d.parse().ok()))
                .and_then(char::from_u32),
        };
        match decoded {
            Some(c) => {
                out.push(c);
                rest = &tail[semi + 1..];
            }
            None => {
                out.push('&');
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// Finds the stored sound in the current list and returns its index. The file path decides
/// first (compared without case, as Windows does; the title breaks ties between duplicates),
/// then the title alone, so a renamed file or a re-titled entry are still found.
pub fn resolve(sounds: &[Sound], wanted: &SoundRef) -> Option<u32> {
    let path = wanted.path.trim();
    let title = wanted.title.trim();
    if !path.is_empty() {
        let same_path: Vec<&Sound> = sounds
            .iter()
            .filter(|s| s.path.trim().eq_ignore_ascii_case(path))
            .collect();
        if let Some(s) = same_path
            .iter()
            .find(|s| s.title.trim() == title)
            .or(same_path.first())
        {
            return Some(s.index);
        }
    }
    if title.is_empty() {
        return None;
    }
    sounds
        .iter()
        .find(|s| s.title.trim() == title)
        .or_else(|| {
            sounds
                .iter()
                .find(|s| s.title.trim().eq_ignore_ascii_case(title))
        })
        .map(|s| s.index)
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIST: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<Soundlist>
  <Sound index="1" url="C:\Sounds\siren.mp3" artist="" title="siren" duration="0:50" addedOn="2025-12-21"/>
  <Sound index="2" url="C:\Sounds\dog &amp; cat.mp3" artist="" title="Dog &amp; &quot;Cat&quot;" duration="0:13"/>
  <Sound index="3" url="C:\Sounds\siren.mp3" artist="" title="siren loud" duration="0:50"/>
  <Sound index="4" url='C:\Sounds\caf&#233;.wav' title="Caf&#xE9;" duration="0:01"/>
  <Sound url="C:\Sounds\broken.mp3" title="no index"/>
</Soundlist>"#;

    fn sref(title: &str, path: &str) -> SoundRef {
        SoundRef {
            title: title.into(),
            path: path.into(),
        }
    }

    #[test]
    fn sound_list_is_parsed_and_unescaped() {
        let sounds = parse_soundlist(LIST);
        assert_eq!(sounds.len(), 4);
        assert_eq!(
            sounds[0],
            Sound {
                index: 1,
                title: "siren".into(),
                path: r"C:\Sounds\siren.mp3".into(),
                duration: "0:50".into(),
            }
        );
        assert_eq!(sounds[1].title, r#"Dog & "Cat""#);
        assert_eq!(sounds[1].path, r"C:\Sounds\dog & cat.mp3");
        assert_eq!(sounds[3].title, "Café");
        assert_eq!(sounds[3].path, r"C:\Sounds\café.wav");
        assert!(parse_soundlist("R-404: Command not found.").is_empty());
    }

    #[test]
    fn sounds_resolve_by_path_then_title() {
        let sounds = parse_soundlist(LIST);
        // Path decides, case-insensitively; the title picks among duplicates.
        assert_eq!(
            resolve(&sounds, &sref("siren loud", r"c:\sounds\SIREN.mp3")),
            Some(3)
        );
        assert_eq!(
            resolve(&sounds, &sref("renamed", r"C:\Sounds\siren.mp3")),
            Some(1)
        );
        // File moved: the title still finds it.
        assert_eq!(
            resolve(&sounds, &sref("Café", r"D:\elsewhere\cafe.wav")),
            Some(4)
        );
        assert_eq!(resolve(&sounds, &sref("café", "")), Some(4));
        assert_eq!(resolve(&sounds, &sref("gone", r"C:\gone.mp3")), None);
        assert_eq!(resolve(&sounds, &sref("", "")), None);
        // The index follows the list: after a reorder the same sound has a new index.
        let mut reordered = sounds.clone();
        reordered.reverse();
        for (i, s) in reordered.iter_mut().enumerate() {
            s.index = i as u32 + 1;
        }
        assert_eq!(
            resolve(&reordered, &sref("siren", r"C:\Sounds\siren.mp3")),
            Some(4)
        );
    }

    #[test]
    fn requests_match_the_reference_client() {
        let play = |output| SoundpadCommand::Play {
            sound: sref("x", "y"),
            output,
        };
        assert_eq!(
            play(SoundOutput::Both).request(Some(5)).as_deref(),
            Some("DoPlaySound(5, true, true)")
        );
        assert_eq!(
            play(SoundOutput::Speakers).request(Some(1)).as_deref(),
            Some("DoPlaySound(1, true, false)")
        );
        assert_eq!(
            play(SoundOutput::Mic).request(Some(12)).as_deref(),
            Some("DoPlaySound(12, false, true)")
        );
        assert_eq!(play(SoundOutput::Both).request(None), None);
        assert_eq!(
            SoundpadCommand::PlayRandom {
                output: SoundOutput::Mic
            }
            .request(None)
            .as_deref(),
            Some("DoPlayRandomSound(false, true)")
        );
        assert_eq!(
            SoundpadCommand::Stop.request(None).as_deref(),
            Some("DoStopSound()")
        );
        assert_eq!(
            SoundpadCommand::TogglePause.request(None).as_deref(),
            Some("DoTogglePause()")
        );
        assert_eq!(
            SoundpadCommand::PlayNext.request(None).as_deref(),
            Some("DoPlayNextSound()")
        );
        assert_eq!(
            SoundpadCommand::PlayPrevious.request(None).as_deref(),
            Some("DoPlayPreviousSound()")
        );
    }

    #[test]
    fn responses() {
        assert!(is_success("R-200"));
        assert!(is_success("R-200: OK"));
        assert!(!is_success("R-404: Command not found."));
        assert!(!is_success(""));
    }

    #[test]
    fn json_shape_is_stable() {
        let c = SoundpadCommand::Play {
            sound: sref("siren", r"C:\s.mp3"),
            output: SoundOutput::Speakers,
        };
        assert_eq!(
            serde_json::to_string(&c).unwrap(),
            r#"{"type":"play","sound":{"title":"siren","path":"C:\\s.mp3"},"output":"speakers"}"#
        );
        let r: SoundpadCommand = serde_json::from_str(r#"{"type":"playRandom"}"#).unwrap();
        assert_eq!(
            r,
            SoundpadCommand::PlayRandom {
                output: SoundOutput::Both
            }
        );
        let s: SoundpadCommand = serde_json::from_str(r#"{"type":"togglePause"}"#).unwrap();
        assert_eq!(s.describe(), "pause/resume");
    }
}

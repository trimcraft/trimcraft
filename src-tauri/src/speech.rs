//! "Speak" actions: Windows speech (SAPI) on a thread of its own.
//!
//! The thread owns one SAPI voice. Speaking is asynchronous; a new text purges the one still
//! being spoken (its action is reported as interrupted). While something is spoken the thread
//! polls the voice's status every few milliseconds to report when it finished.
//!
//! Voices: the classic SAPI voices and the newer OneCore voices Windows installs per language.

use crate::executor::Done;
use detent_core::audio::VoiceRef;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::time::Duration;

/// How often a running speech is checked for having finished.
const POLL: Duration = Duration::from_millis(50);

/// What to speak, with the placeholders filled in.
pub struct Utterance {
    pub text: String,
    pub voice: Option<VoiceRef>,
    pub rate: i32,
    pub volume: u8,
}

enum Job {
    Speak(Utterance, Done),
    Voices(Sender<Result<Vec<VoiceRef>, String>>),
}

pub struct Speech {
    queue: Sender<Job>,
}

impl Speech {
    pub fn spawn() -> Self {
        let (queue, jobs) = channel();
        std::thread::Builder::new()
            .name("detent-speech".into())
            .spawn(move || platform::run(jobs))
            .expect("spawn speech thread");
        Self { queue }
    }

    /// Speaks `utterance`, interrupting what is still spoken. `done` gets the outcome.
    pub fn speak(&self, utterance: Utterance, done: Done) {
        if let Err(e) = self.queue.send(Job::Speak(utterance, done)) {
            let Job::Speak(_, done) = e.0 else { return };
            done(Err("the speech thread is gone".into()));
        }
    }

    /// The installed voices, for the voice picker.
    pub fn voices(&self) -> Result<Vec<VoiceRef>, String> {
        let (reply, answer) = channel();
        self.queue
            .send(Job::Voices(reply))
            .map_err(|_| "the speech thread is gone".to_owned())?;
        answer
            .recv_timeout(Duration::from_secs(10))
            .map_err(|_| "Windows speech did not answer".to_owned())?
    }
}

mod platform {
    use super::*;
    use windows::Win32::Media::Speech::{
        IEnumSpObjectTokens, ISpObjectToken, ISpObjectTokenCategory, ISpVoice, SPCAT_VOICES,
        SPF_ASYNC, SPF_IS_NOT_XML, SPF_PURGEBEFORESPEAK, SPRS_DONE, SPVOICESTATUS, SpObjectToken,
        SpObjectTokenCategory, SpVoice,
    };
    use windows::Win32::System::Com::{
        CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree,
    };
    use windows::core::{HSTRING, PCWSTR, PWSTR, w};

    /// The OneCore voices (Windows 10/11 language packs); SAPI can use their tokens.
    const ONECORE_VOICES: PCWSTR =
        w!("HKEY_LOCAL_MACHINE\\SOFTWARE\\Microsoft\\Speech_OneCore\\Voices");

    pub fn run(jobs: Receiver<Job>) {
        // SAFETY: once, on this thread, before any COM call.
        if let Err(e) = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.ok() {
            log::error!("speech: COM is not available: {e}");
        }
        // SAFETY: plain COM object creation.
        let voice: Result<ISpVoice, _> = unsafe { CoCreateInstance(&SpVoice, None, CLSCTX_ALL) };
        let voice = match voice {
            Ok(v) => Some(v),
            Err(e) => {
                log::error!("speech: Windows speech is not available: {e}");
                None
            }
        };
        let mut current: Option<Done> = None;
        loop {
            let job = if current.is_some() {
                match jobs.recv_timeout(POLL) {
                    Ok(job) => Some(job),
                    Err(RecvTimeoutError::Timeout) => None,
                    Err(RecvTimeoutError::Disconnected) => break,
                }
            } else {
                match jobs.recv() {
                    Ok(job) => Some(job),
                    Err(_) => break,
                }
            };
            match job {
                None => {
                    if let Some(voice) = &voice
                        && finished(voice)
                        && let Some(done) = current.take()
                    {
                        done(Ok("Spoken".into()));
                    }
                }
                Some(Job::Voices(reply)) => {
                    let _ = reply.send(list_voices());
                }
                Some(Job::Speak(utterance, done)) => {
                    let Some(voice) = &voice else {
                        done(Err("Windows speech is not available".into()));
                        continue;
                    };
                    if let Some(previous) = current.take() {
                        previous(Ok("Interrupted by the next speech".into()));
                    }
                    match speak(voice, &utterance) {
                        Ok(()) => current = Some(done),
                        Err(e) => done(Err(e)),
                    }
                }
            }
        }
    }

    fn finished(voice: &ISpVoice) -> bool {
        let mut status = SPVOICESTATUS::default();
        // SAFETY: `status` outlives the call; no bookmark wanted.
        let ok = unsafe { voice.GetStatus(&mut status, std::ptr::null_mut()) };
        ok.is_err() || status.dwRunningState == SPRS_DONE.0 as u32
    }

    fn speak(voice: &ISpVoice, u: &Utterance) -> Result<(), String> {
        // SAFETY: COM calls on this thread's voice; strings outlive the calls.
        unsafe {
            let token = match &u.voice {
                Some(v) => match token(&v.id) {
                    Ok(t) => Some(t),
                    Err(e) => {
                        log::warn!(
                            "speech: voice {:?} not found ({e}); using the default",
                            v.name
                        );
                        None
                    }
                },
                None => None,
            };
            if voice.SetVoice(token.as_ref()).is_err() {
                log::warn!("speech: voice cannot be used; using the default");
                let _ = voice.SetVoice(None::<&ISpObjectToken>);
            }
            voice.SetRate(u.rate).map_err(|e| e.to_string())?;
            voice
                .SetVolume(u16::from(u.volume.min(100)))
                .map_err(|e| e.to_string())?;
            let text = HSTRING::from(u.text.as_str());
            let flags = (SPF_ASYNC.0 | SPF_PURGEBEFORESPEAK.0 | SPF_IS_NOT_XML.0) as u32;
            voice
                .Speak(&text, flags, None)
                .map_err(|e| format!("Windows speech failed: {e}"))
        }
    }

    fn token(id: &str) -> windows::core::Result<ISpObjectToken> {
        // SAFETY: plain COM calls.
        unsafe {
            let token: ISpObjectToken = CoCreateInstance(&SpObjectToken, None, CLSCTX_ALL)?;
            token.SetId(PCWSTR::null(), &HSTRING::from(id), false)?;
            Ok(token)
        }
    }

    /// Takes a COM-allocated string.
    fn take(s: PWSTR) -> String {
        // SAFETY: SAPI returns NUL-terminated strings allocated with CoTaskMemAlloc.
        unsafe {
            let text = s.to_string().unwrap_or_default();
            CoTaskMemFree(Some(s.0 as *const _));
            text
        }
    }

    fn list_voices() -> Result<Vec<VoiceRef>, String> {
        let mut voices = Vec::new();
        for category in [SPCAT_VOICES, ONECORE_VOICES] {
            match tokens(category) {
                Ok(list) => {
                    for v in list {
                        if !voices.iter().any(|x: &VoiceRef| x.name == v.name) {
                            voices.push(v);
                        }
                    }
                }
                Err(e) => log::warn!("speech: cannot list voices: {e}"),
            }
        }
        if voices.is_empty() {
            return Err("no Windows voices found".into());
        }
        voices.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(voices)
    }

    fn tokens(category: PCWSTR) -> windows::core::Result<Vec<VoiceRef>> {
        // SAFETY: plain COM calls; returned strings are freed by `take`.
        unsafe {
            let cat: ISpObjectTokenCategory =
                CoCreateInstance(&SpObjectTokenCategory, None, CLSCTX_ALL)?;
            cat.SetId(category, false)?;
            let list: IEnumSpObjectTokens = cat.EnumTokens(PCWSTR::null(), PCWSTR::null())?;
            let mut count = 0u32;
            list.GetCount(&mut count)?;
            let mut out = Vec::new();
            for i in 0..count {
                let token = list.Item(i)?;
                let id = take(token.GetId()?);
                // The token's default value is its display name.
                let name = token
                    .GetStringValue(PCWSTR::null())
                    .map(take)
                    .unwrap_or_else(|_| id.clone());
                out.push(VoiceRef { id, name });
            }
            Ok(out)
        }
    }
}

#[cfg(test)]
mod tests {
    /// Lists the installed voices; speaks nothing.
    #[test]
    #[ignore = "needs Windows speech"]
    fn lists_voices() {
        let voices = super::Speech::spawn().voices().unwrap();
        println!("{voices:#?}");
        assert!(!voices.is_empty());
    }
}

//! Talks to Soundpad over its remote control pipe (see `detent_core::soundpad`).
//!
//! Commands from actions run on a worker thread of their own, so the action queue never
//! waits for Soundpad. Every pipe operation has a short timeout; when Soundpad is not running
//! or a sound is missing, a warning is logged and kept for the UI.
//!
//! The sound list is cached and refreshed when it is older than [`LIST_MAX_AGE`] or when a
//! stored sound is not found in it, so playing a sound normally costs one round trip.

use detent_core::soundpad::{self, Sound, SoundpadCommand};
use serde::Serialize;
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Upper bound for one request (connect, write, read).
const TIMEOUT: Duration = Duration::from_millis(500);
/// A cached sound list older than this is fetched again before resolving a sound, so an index
/// is rarely taken from a list the user has rearranged since.
const LIST_MAX_AGE: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SoundpadStatus {
    /// Soundpad answered `IsAlive()`.
    pub alive: bool,
    /// Why it is not reachable.
    pub error: Option<String>,
    /// The last problem running a Soundpad action, until one succeeds again.
    pub last_warning: Option<String>,
}

#[derive(Default)]
struct Cache {
    sounds: Vec<Sound>,
    fetched: Option<Instant>,
}

#[derive(Default)]
struct State {
    cache: Mutex<Cache>,
    last_warning: Mutex<Option<String>>,
}

pub struct Soundpad {
    queue: Sender<SoundpadCommand>,
    state: Arc<State>,
}

impl Soundpad {
    pub fn spawn() -> Self {
        let (queue, commands) = channel::<SoundpadCommand>();
        let state = Arc::new(State::default());
        let worker = state.clone();
        std::thread::Builder::new()
            .name("detent-soundpad".into())
            .spawn(move || {
                for command in commands {
                    worker.run(&command);
                }
            })
            .expect("spawn soundpad thread");
        Self { queue, state }
    }

    /// Queues a command; returns at once.
    pub fn submit(&self, command: SoundpadCommand) {
        if self.queue.send(command).is_err() {
            log::error!("soundpad thread is gone; command dropped");
        }
    }

    /// Asks Soundpad whether it is there. Blocks for at most [`TIMEOUT`].
    pub fn status(&self) -> SoundpadStatus {
        let (alive, error) = match pipe::request(soundpad::IS_ALIVE, TIMEOUT) {
            Ok(r) if soundpad::is_success(&r) => (true, None),
            Ok(r) => (false, Some(format!("Soundpad answered {r:?}"))),
            Err(e) => (false, Some(e)),
        };
        SoundpadStatus {
            alive,
            error,
            last_warning: lock(&self.state.last_warning).clone(),
        }
    }

    /// The current sound list, fetched from Soundpad (and cached for actions).
    pub fn sounds(&self) -> Result<Vec<Sound>, String> {
        self.state.refresh()
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl State {
    fn refresh(&self) -> Result<Vec<Sound>, String> {
        let xml = pipe::request(soundpad::GET_SOUNDLIST, TIMEOUT)?;
        if !xml.contains("<Soundlist") {
            return Err(format!("unexpected answer to GetSoundlist: {xml:.80}"));
        }
        let sounds = soundpad::parse_soundlist(&xml);
        let mut cache = lock(&self.cache);
        cache.sounds = sounds.clone();
        cache.fetched = Some(Instant::now());
        Ok(sounds)
    }

    /// Index of the stored sound: from the cache if it is fresh and has the sound, otherwise
    /// from a newly fetched list.
    fn index_of(&self, sound: &detent_core::SoundRef) -> Result<Option<u32>, String> {
        {
            let cache = lock(&self.cache);
            let fresh = cache.fetched.is_some_and(|t| t.elapsed() < LIST_MAX_AGE);
            if fresh && let Some(i) = soundpad::resolve(&cache.sounds, sound) {
                return Ok(Some(i));
            }
        }
        Ok(soundpad::resolve(&self.refresh()?, sound))
    }

    fn run(&self, command: &SoundpadCommand) {
        let index = match command {
            SoundpadCommand::Play { sound, .. } => match self.index_of(sound) {
                Ok(Some(i)) => Some(i),
                Ok(None) => {
                    return self.warn(format!(
                        "Soundpad: sound \"{}\" ({}) is not in Soundpad's list",
                        sound.title, sound.path
                    ));
                }
                Err(e) => return self.warn(format!("Soundpad: {e}")),
            },
            _ => None,
        };
        let Some(request) = command.request(index) else {
            return;
        };
        match pipe::request(&request, TIMEOUT) {
            Ok(r) if soundpad::is_success(&r) => {
                log::info!("soundpad: {request}");
                *lock(&self.last_warning) = None;
            }
            Ok(r) => self.warn(format!("Soundpad: {request} failed: {}", r.trim())),
            Err(e) => self.warn(format!("Soundpad: {e}")),
        }
    }

    fn warn(&self, message: String) {
        log::warn!("{message}");
        *lock(&self.last_warning) = Some(message);
    }
}

#[cfg(windows)]
mod pipe {
    //! One request per connection, overlapped I/O so every step can time out.
    use detent_core::soundpad::PIPE;
    use std::time::{Duration, Instant};
    use windows_sys::Win32::Foundation::{
        CloseHandle, ERROR_FILE_NOT_FOUND, ERROR_IO_PENDING, ERROR_MORE_DATA, ERROR_PIPE_BUSY,
        GENERIC_READ, GENERIC_WRITE, GetLastError, HANDLE, INVALID_HANDLE_VALUE, WAIT_OBJECT_0,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, FILE_FLAG_OVERLAPPED, OPEN_EXISTING, ReadFile, WriteFile,
    };
    use windows_sys::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};
    use windows_sys::Win32::System::Pipes::{
        PIPE_READMODE_MESSAGE, SetNamedPipeHandleState, WaitNamedPipeW,
    };
    use windows_sys::Win32::System::Threading::{CreateEventW, WaitForSingleObject};

    struct Handle(HANDLE);

    impl Drop for Handle {
        fn drop(&mut self) {
            // SAFETY: the handle was returned by CreateFileW/CreateEventW and is closed once.
            unsafe { CloseHandle(self.0) };
        }
    }

    fn ms_left(deadline: Instant) -> u32 {
        u32::try_from(
            deadline
                .saturating_duration_since(Instant::now())
                .as_millis(),
        )
        .unwrap_or(u32::MAX)
    }

    fn open(name: &[u16], deadline: Instant) -> Result<Handle, String> {
        loop {
            // SAFETY: `name` is a NUL-terminated UTF-16 string; the other arguments are plain
            // values or null.
            let h = unsafe {
                CreateFileW(
                    name.as_ptr(),
                    GENERIC_READ | GENERIC_WRITE,
                    0,
                    std::ptr::null(),
                    OPEN_EXISTING,
                    FILE_FLAG_OVERLAPPED,
                    std::ptr::null_mut(),
                )
            };
            if h != INVALID_HANDLE_VALUE {
                return Ok(Handle(h));
            }
            // SAFETY: no preconditions.
            match unsafe { GetLastError() } {
                ERROR_FILE_NOT_FOUND => {
                    return Err("Soundpad is not running (or the full version is required)".into());
                }
                // SAFETY: `name` is NUL-terminated.
                ERROR_PIPE_BUSY
                    if ms_left(deadline) > 0
                        && unsafe { WaitNamedPipeW(name.as_ptr(), ms_left(deadline)) } != 0 => {}
                ERROR_PIPE_BUSY => return Err("Soundpad is busy".into()),
                e => return Err(format!("cannot connect to Soundpad (error {e})")),
            }
        }
    }

    /// Runs one overlapped read or write and waits for it until `deadline`. Returns the bytes
    /// transferred and whether the message continues (`ERROR_MORE_DATA`).
    fn finish(
        pipe: &Handle,
        event: &Handle,
        ov: &mut OVERLAPPED,
        started: i32,
        deadline: Instant,
    ) -> Result<(u32, bool), String> {
        // SAFETY: no preconditions.
        let error = unsafe { GetLastError() };
        if started == 0 && error != ERROR_IO_PENDING && error != ERROR_MORE_DATA {
            return Err(format!("Soundpad pipe error {error}"));
        }
        // SAFETY: `event` is a valid event handle owned by the caller.
        if unsafe { WaitForSingleObject(event.0, ms_left(deadline)) } != WAIT_OBJECT_0 {
            // SAFETY: `ov` belongs to the pending operation on `pipe`; waiting for the result
            // afterwards guarantees the kernel no longer uses `ov` or the buffer.
            unsafe {
                CancelIoEx(pipe.0, ov);
                let mut n = 0;
                GetOverlappedResult(pipe.0, ov, &mut n, 1);
            }
            return Err("Soundpad did not answer in time".into());
        }
        let mut n = 0;
        // SAFETY: the operation has completed (event signalled); `ov` is the one it used.
        if unsafe { GetOverlappedResult(pipe.0, ov, &mut n, 0) } != 0 {
            return Ok((n, false));
        }
        // SAFETY: no preconditions.
        match unsafe { GetLastError() } {
            ERROR_MORE_DATA => Ok((n, true)),
            e => Err(format!("Soundpad pipe error {e}")),
        }
    }

    pub fn request(request: &str, timeout: Duration) -> Result<String, String> {
        let deadline = Instant::now() + timeout;
        let name: Vec<u16> = PIPE.encode_utf16().chain(Some(0)).collect();
        let pipe = open(&name, deadline)?;
        let mode = PIPE_READMODE_MESSAGE;
        // SAFETY: valid pipe handle and a pointer to a local mode value.
        if unsafe { SetNamedPipeHandleState(pipe.0, &mode, std::ptr::null(), std::ptr::null()) }
            == 0
        {
            // SAFETY: no preconditions.
            return Err(format!("Soundpad pipe error {}", unsafe { GetLastError() }));
        }
        // SAFETY: plain arguments; a manual-reset event, initially not signalled.
        let event = unsafe { CreateEventW(std::ptr::null(), 1, 0, std::ptr::null()) };
        if event.is_null() {
            return Err("cannot create an event".into());
        }
        let event = Handle(event);

        let bytes = request.as_bytes();
        let mut ov = OVERLAPPED {
            hEvent: event.0,
            ..Default::default()
        };
        // SAFETY: `bytes` and `ov` outlive the operation: `finish` waits for its completion or
        // cancellation before returning.
        let started = unsafe {
            WriteFile(
                pipe.0,
                bytes.as_ptr(),
                u32::try_from(bytes.len()).map_err(|_| "request too long")?,
                std::ptr::null_mut(),
                &mut ov,
            )
        };
        finish(&pipe, &event, &mut ov, started, deadline)?;

        let mut response = Vec::new();
        let mut chunk = vec![0u8; 64 * 1024];
        loop {
            let mut ov = OVERLAPPED {
                hEvent: event.0,
                ..Default::default()
            };
            // SAFETY: as for the write: `chunk` and `ov` outlive the operation.
            let started = unsafe {
                ReadFile(
                    pipe.0,
                    chunk.as_mut_ptr(),
                    chunk.len() as u32,
                    std::ptr::null_mut(),
                    &mut ov,
                )
            };
            let (n, more) = finish(&pipe, &event, &mut ov, started, deadline)?;
            response.extend_from_slice(&chunk[..n as usize]);
            if !more {
                break;
            }
        }
        Ok(String::from_utf8_lossy(&response).into_owned())
    }
}

#[cfg(not(windows))]
mod pipe {
    use std::time::Duration;

    pub fn request(_request: &str, _timeout: Duration) -> Result<String, String> {
        Err("Soundpad is only available on Windows".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Needs a running Soundpad: `cargo test -p detent -- --ignored soundpad`. Plays nothing.
    #[test]
    #[ignore = "needs a running Soundpad"]
    fn live_soundpad_answers() {
        let soundpad = Soundpad::spawn();
        let status = soundpad.status();
        assert!(status.alive, "{status:?}");
        let started = Instant::now();
        let sounds = soundpad.sounds().unwrap();
        println!("{} sounds in {:?}", sounds.len(), started.elapsed());
        assert!(!sounds.is_empty());
        let first = &sounds[0];
        let wanted = detent_core::SoundRef {
            title: first.title.clone(),
            path: first.path.clone(),
        };
        assert_eq!(soundpad.state.index_of(&wanted), Ok(Some(first.index)));
        // An unknown command gets a 404, not a hang.
        let r = pipe::request("NoSuchCommand()", TIMEOUT).unwrap();
        assert!(r.starts_with("R-404"), "{r}");
    }
}

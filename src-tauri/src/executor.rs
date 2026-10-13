//! Runs actions on a worker thread of its own, so a macro with delays never blocks the engine
//! thread. Requests start strictly in the order they arrive.
//!
//! - Keyboard and mouse macros and media keys are synthesised with `SendInput` (keys as scan
//!   codes, so games reading DirectInput or RawInput see them) and run to the end before the
//!   next action starts.
//! - Soundpad, Discord and Voicemeeter commands are handed on to their connector threads, so
//!   a slow or missing program never holds up the queue.
//! - Volume changes are quick Core Audio calls and run in place.
//! - Long-running actions (speech, sound file, launch, HTTP) start in the background and the
//!   queue goes on, unless the action is set to "Wait until finished". Their result goes to
//!   the event log either way.
//!
//! Failures are logged with a reason and never stop the queue.

use crate::discord::Discord;
use crate::overlay;
use crate::sound::{Play, Sound};
use crate::soundpad::Soundpad;
use crate::speech::{Speech, Utterance};
use crate::voicemeeter::Voicemeeter;
use crate::{launch, volume, webhook};
use detent_core::diagnostics::action_record;
use detent_core::{Action, ActionEvent, ActionRequest, Diagnostics, InputOp, Phase};
use std::sync::Arc;
use std::sync::mpsc::{Sender, channel};
use std::time::{Duration, Instant};
use tauri::AppHandle;

/// Called once with the outcome of a long-running action ("Spoken", "HTTP 204", ...).
pub type Done = Box<dyn FnOnce(Result<String, String>) + Send>;

/// "Wait until finished" waits at most this long; then the queue goes on and the action keeps
/// running.
const MAX_WAIT: Duration = Duration::from_secs(10 * 60);

/// The threads actions are handed to.
pub struct Services {
    pub soundpad: Arc<Soundpad>,
    pub discord: Arc<Discord>,
    pub voicemeeter: Arc<Voicemeeter>,
    pub speech: Arc<Speech>,
    pub sound: Arc<Sound>,
}

pub struct Executor {
    queue: Sender<ActionRequest>,
    diagnostics: Diagnostics,
}

impl Executor {
    pub fn spawn(app: AppHandle, services: Services, diagnostics: Diagnostics) -> Self {
        let log = diagnostics.clone();
        let (queue, requests) = channel::<ActionRequest>();
        std::thread::Builder::new()
            .name("detent-actions".into())
            .spawn(move || {
                for request in requests {
                    run(&app, &request, &services, &log);
                }
            })
            .expect("spawn action thread");
        Self { queue, diagnostics }
    }

    pub fn submit(&self, requests: Vec<ActionRequest>) {
        for request in requests {
            self.diagnostics.record(action_record(
                &ActionEvent::Queued,
                &origin(&request),
                &request.action.describe(),
            ));
            if self.queue.send(request).is_err() {
                log::error!("action thread is gone; action dropped");
            }
        }
    }
}

/// Where an action came from, e.g. "Comms / B (activate)".
pub fn origin(request: &ActionRequest) -> String {
    let phase = match request.phase {
        Phase::Activate => "activate",
        Phase::Deactivate => "deactivate",
    };
    format!("{} / {} ({phase})", request.control, request.state)
}

/// Records the events of one action in the event log.
#[derive(Clone)]
struct Recorder {
    diagnostics: Diagnostics,
    origin: String,
    description: String,
}

impl Recorder {
    fn record(&self, event: ActionEvent) {
        if let ActionEvent::Failed { error } = &event {
            log::warn!(
                "action failed: {}: {}: {error}",
                self.origin,
                self.description
            );
        }
        self.diagnostics
            .record(action_record(&event, &self.origin, &self.description));
    }

    fn outcome(&self, started: Instant, result: Result<String, String>) {
        self.record(match result {
            Ok(outcome) => ActionEvent::Finished {
                ms: started.elapsed().as_secs_f64() * 1000.0,
                outcome,
            },
            Err(error) => ActionEvent::Failed { error },
        });
    }
}

fn run(app: &AppHandle, request: &ActionRequest, services: &Services, diagnostics: &Diagnostics) {
    let record = Recorder {
        diagnostics: diagnostics.clone(),
        origin: origin(request),
        description: request.action.describe(),
    };
    log::info!("action: {}: {}", record.origin, record.description);
    let handed = |connector: &str| {
        record.record(ActionEvent::Handed {
            connector: connector.into(),
        });
    };
    match &request.action {
        Action::Soundpad { command } => {
            services.soundpad.submit(command.clone());
            handed("Soundpad");
        }
        Action::Discord { command } => {
            services.discord.submit(*command);
            handed("Discord");
        }
        Action::Voicemeeter { target, command } => {
            services.voicemeeter.submit(*target, *command);
            handed("Voicemeeter");
        }
        // The engine switched already, when the action was emitted; it is here for the log.
        Action::SwitchProfile { .. } => {}
        Action::Volume { target, command } => {
            let started = Instant::now();
            record.outcome(started, volume::apply(target, *command));
        }
        Action::Speak(speech) => {
            let utterance = Utterance {
                text: request.placeholders().fill(speech.text.trim()),
                voice: speech.voice.clone(),
                rate: speech.rate,
                volume: speech.volume,
            };
            let speech_thread = services.speech.clone();
            long_running(&record, speech.wait, move |done| {
                speech_thread.speak(utterance, done);
            });
        }
        Action::PlaySound(sound) => {
            let play = Play {
                path: sound.path.clone(),
                volume: sound.volume,
                device: sound.device.clone(),
                stop_previous: sound.stop_previous,
            };
            let sound_thread = services.sound.clone();
            long_running(&record, sound.wait, move |done| {
                sound_thread.play(play, done);
            });
        }
        Action::Launch(l) => {
            let (target, wait) = (l.target.clone(), l.wait);
            long_running(&record, wait, move |done| {
                in_background("detent-launch", done, move || launch::run(&target, wait));
            });
        }
        Action::Http(r) => {
            let prepared = r.prepare(&request.placeholders());
            long_running(&record, r.wait, move |done| {
                in_background("detent-http", done, move || webhook::send(&prepared));
            });
        }
        Action::KeyboardMacro { .. } | Action::MouseMacro { .. } | Action::MediaKey { .. } => {
            send_input(app, request, &record);
        }
    }
}

/// Starts a long-running action with `start`, which calls `done` when it has finished. With
/// `wait` the queue waits for that (at most [`MAX_WAIT`]); otherwise it goes on at once.
fn long_running(record: &Recorder, wait: bool, start: impl FnOnce(Done)) {
    let started = Instant::now();
    if wait {
        let (tx, rx) = channel();
        start(Box::new(move |result| {
            let _ = tx.send(result);
        }));
        match rx.recv_timeout(MAX_WAIT) {
            Ok(result) => record.outcome(started, result),
            Err(_) => record.record(ActionEvent::Failed {
                error: format!(
                    "not finished after {} minutes; the queue goes on",
                    MAX_WAIT.as_secs() / 60
                ),
            }),
        }
    } else {
        record.record(ActionEvent::Started);
        let record = record.clone();
        start(Box::new(move |result| record.outcome(started, result)));
    }
}

/// Runs `job` on a thread of its own and hands its result to `done`.
fn in_background(
    name: &str,
    done: Done,
    job: impl FnOnce() -> Result<String, String> + Send + 'static,
) {
    let spawned = std::thread::Builder::new()
        .name(name.into())
        .spawn(move || done(job()));
    if let Err(e) = spawned {
        log::error!("cannot start a thread for an action: {e}");
    }
}

fn send_input(app: &AppHandle, request: &ActionRequest, record: &Recorder) {
    // Absolute moves need the current monitor layout; it is queried per macro, so a changed
    // arrangement is picked up.
    let screens = if request.action.needs_screens() {
        overlay::screens(app)
    } else {
        None
    };
    let (ops, warnings) = request.action.expand(screens.as_ref());
    for warning in warnings {
        log::warn!("action: {warning}");
    }
    // Consecutive inputs go out in one call, so nothing else can interleave with them. Waits
    // are measured from the macro's start, so the time spent sending (and the sleep timer's
    // granularity) does not add up over a long recorded path.
    let start = Instant::now();
    let mut due = Duration::ZERO;
    let mut batch = Vec::new();
    let mut failure = None;
    let mut send = |batch: &[InputOp]| {
        if let Err(e) = platform::send(batch) {
            failure.get_or_insert(e);
        }
    };
    for op in ops {
        if let InputOp::Sleep { ms } = op {
            send(&batch);
            batch.clear();
            due += Duration::from_millis(u64::from(ms));
            if let Some(wait) = due.checked_sub(start.elapsed()) {
                std::thread::sleep(wait);
            }
        } else {
            batch.push(op);
        }
    }
    send(&batch);
    match failure {
        None => record.record(ActionEvent::Executed {
            ms: start.elapsed().as_secs_f64() * 1000.0,
        }),
        Some(error) => record.record(ActionEvent::Failed { error }),
    }
}

mod platform {
    use detent_core::{InputOp, MouseButton};
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBDINPUT, KEYEVENTF_EXTENDEDKEY,
        KEYEVENTF_KEYUP, KEYEVENTF_SCANCODE, MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_LEFTDOWN,
        MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP, MOUSEEVENTF_MOVE,
        MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP, MOUSEEVENTF_VIRTUALDESK, MOUSEEVENTF_WHEEL,
        MOUSEEVENTF_XDOWN, MOUSEEVENTF_XUP, MOUSEINPUT, SendInput,
    };

    const XBUTTON1: u32 = 1;
    const XBUTTON2: u32 = 2;
    const WHEEL_DELTA: i32 = 120;
    /// Marks input Detent synthesised ("DTNT"), so it can be told apart later.
    const EXTRA_INFO: usize = 0x4454_4E54;

    fn keyboard(scan: u16, extended: bool, down: bool) -> INPUT {
        let mut flags = KEYEVENTF_SCANCODE;
        if extended {
            flags |= KEYEVENTF_EXTENDEDKEY;
        }
        if !down {
            flags |= KEYEVENTF_KEYUP;
        }
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: 0,
                    wScan: scan,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: EXTRA_INFO,
                },
            },
        }
    }

    fn mouse(dx: i32, dy: i32, data: u32, flags: u32) -> INPUT {
        INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 {
                mi: MOUSEINPUT {
                    dx,
                    dy,
                    mouseData: data,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: EXTRA_INFO,
                },
            },
        }
    }

    fn to_input(op: &InputOp) -> Option<INPUT> {
        Some(match *op {
            InputOp::Key {
                scan,
                extended,
                down,
            } => keyboard(scan, extended, down),
            InputOp::MouseButton { button, down } => {
                let (flags, data) = match (button, down) {
                    (MouseButton::Left, true) => (MOUSEEVENTF_LEFTDOWN, 0),
                    (MouseButton::Left, false) => (MOUSEEVENTF_LEFTUP, 0),
                    (MouseButton::Right, true) => (MOUSEEVENTF_RIGHTDOWN, 0),
                    (MouseButton::Right, false) => (MOUSEEVENTF_RIGHTUP, 0),
                    (MouseButton::Middle, true) => (MOUSEEVENTF_MIDDLEDOWN, 0),
                    (MouseButton::Middle, false) => (MOUSEEVENTF_MIDDLEUP, 0),
                    (MouseButton::X1, true) => (MOUSEEVENTF_XDOWN, XBUTTON1),
                    (MouseButton::X1, false) => (MOUSEEVENTF_XUP, XBUTTON1),
                    (MouseButton::X2, true) => (MOUSEEVENTF_XDOWN, XBUTTON2),
                    (MouseButton::X2, false) => (MOUSEEVENTF_XUP, XBUTTON2),
                };
                mouse(0, 0, data, flags)
            }
            // mouseData is a DWORD holding a signed value.
            InputOp::Wheel { delta } => mouse(
                0,
                0,
                delta.saturating_mul(WHEEL_DELTA) as u32,
                MOUSEEVENTF_WHEEL,
            ),
            InputOp::Move { dx, dy } => mouse(dx, dy, 0, MOUSEEVENTF_MOVE),
            InputOp::MoveAbsolute { x, y } => mouse(
                x,
                y,
                0,
                MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK,
            ),
            // Media keys: by virtual-key code, flagged extended like a keyboard's media keys.
            InputOp::VirtualKey { vk, down } => INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: vk,
                        wScan: 0,
                        dwFlags: KEYEVENTF_EXTENDEDKEY | if down { 0 } else { KEYEVENTF_KEYUP },
                        time: 0,
                        dwExtraInfo: EXTRA_INFO,
                    },
                },
            },
            InputOp::Sleep { .. } => return None,
        })
    }

    pub fn send(ops: &[InputOp]) -> Result<(), String> {
        let inputs: Vec<INPUT> = ops.iter().filter_map(to_input).collect();
        if inputs.is_empty() {
            return Ok(());
        }
        let count = u32::try_from(inputs.len()).unwrap_or(u32::MAX);
        let size = i32::try_from(std::mem::size_of::<INPUT>()).unwrap_or(i32::MAX);
        // SAFETY: `inputs` is a valid, initialised array of `count` INPUT structures that
        // outlives the call, and `size` is the size of one structure.
        let sent = unsafe { SendInput(count, inputs.as_ptr(), size) };
        if sent != count {
            // Typically UIPI: the foreground window belongs to a process with higher integrity
            // (a game running as administrator).
            let error = format!(
                "SendInput delivered {sent} of {count} inputs (error {})",
                std::io::Error::last_os_error()
            );
            log::warn!("{error}");
            return Err(error);
        }
        Ok(())
    }
}

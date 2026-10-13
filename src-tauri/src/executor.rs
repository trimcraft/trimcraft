//! Runs actions on a worker thread of its own, so a macro with delays never blocks the engine
//! thread. Requests start strictly in the order they arrive.
//!
//! - Input macros are played by the macro player (`player.rs`): a macro that runs once
//!   plays to the end before the next action starts; a repeating one runs in the background.
//!   Media keys are synthesised with `SendInput` here.
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
use crate::player::{Outcome, Player};
use crate::sound::{Play, Sound};
use crate::soundpad::Soundpad;
use crate::speech::{Speech, Utterance};
use crate::voicemeeter::Voicemeeter;
use crate::{input, launch, volume, webhook};
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
    pub player: Player,
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
        Action::InputMacro(_) => match services.player.play(request) {
            Outcome::Played(ms) => record.record(ActionEvent::Executed { ms }),
            Outcome::Background => record.record(ActionEvent::Started),
            Outcome::Ignored => record.record(ActionEvent::Skipped {
                reason: "this macro is still running".into(),
            }),
            Outcome::Failed(error) => record.record(ActionEvent::Failed { error }),
        },
        Action::StopMacros { target } => {
            let result = services.player.stop(target);
            record.record(ActionEvent::Finished {
                ms: 0.0,
                outcome: format!("Macros {result}"),
            });
        }
        Action::MediaKey { .. } => send_input(app, request, &record),
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
        if let Err(e) = input::send(batch) {
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

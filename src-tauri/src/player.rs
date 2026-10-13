//! Plays input macros: once in the ordered action queue, or repeated on a thread of their own
//! so the queue goes on (see `detent_core::macro_run` for the decisions).
//!
//! Every run can be stopped: by its control state ending ("while active"), a "Stop macros"
//! action, the panic hotkey, a profile switch or Detent quitting. Stopping releases every key
//! and mouse button the macro pressed and has not released. The panic hotkey, a profile switch
//! and quitting also release what finished macros left pressed (e.g. a "Ctrl down" macro
//! waiting for its "Ctrl up").
//!
//! Waits are measured from the run's start (no drift over long recordings) and are
//! interruptible, so stopping takes effect within a few milliseconds.

use crate::{input, overlay};
use detent_core::action::MacroTarget;
use detent_core::diagnostics::{Category, Level, Record};
use detent_core::macro_run::{
    HeldInputs, Next, Registry, Repetition, Scope, Start, StopReason, stop_text,
};
use detent_core::macros::{InputMacro, Repeat};
use detent_core::{Action, ActionRequest, Diagnostics, InputOp, MacroSignal};
use std::collections::HashMap;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};
use tauri::AppHandle;

/// How long a restart or quit waits for a stopped run to release its inputs.
const STOP_WAIT: Duration = Duration::from_secs(1);

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// What a run is told from outside.
#[derive(Default)]
struct Signals {
    stop: Option<StopReason>,
    /// The control state it belongs to ended (not handled yet).
    state_ended: bool,
    /// The run has ended and released its inputs.
    done: bool,
}

/// One running macro, as seen from outside.
struct Run {
    macro_id: String,
    scope: Option<Scope>,
    while_active: bool,
    signals: Mutex<Signals>,
    changed: Condvar,
}

impl Run {
    fn stop(&self, reason: StopReason) {
        let mut s = lock(&self.signals);
        s.stop.get_or_insert(reason);
        self.changed.notify_all();
    }

    fn end_state(&self) {
        lock(&self.signals).state_ended = true;
        self.changed.notify_all();
    }

    /// Waits until the run has ended (at most `timeout`).
    fn wait_done(&self, timeout: Duration) {
        let deadline = Instant::now() + timeout;
        let mut s = lock(&self.signals);
        while !s.done {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                break;
            }
            s = self
                .changed
                .wait_timeout(s, left)
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
    }

    /// Sleeps until `deadline` or until something asks the run to stop. Returns why it must
    /// stop, if it must. A state that ended is handed to `repetition` (`running`: in the middle
    /// of a run, not in the pause).
    fn sleep_until(
        &self,
        deadline: Instant,
        repetition: &mut Repetition,
        running: bool,
    ) -> Option<StopReason> {
        let mut s = lock(&self.signals);
        loop {
            if let Some(reason) = s.stop {
                return Some(reason);
            }
            if s.state_ended {
                s.state_ended = false;
                if repetition.state_ended(running) {
                    s.stop = Some(StopReason::StateEnded);
                    return s.stop;
                }
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return None;
            }
            s = self
                .changed
                .wait_timeout(s, left)
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
    }
}

struct Inner {
    app: AppHandle,
    diagnostics: Diagnostics,
    registry: Mutex<Registry>,
    runs: Mutex<HashMap<u64, Arc<Run>>>,
    /// Every key and button any macro pressed and nothing released yet.
    held: Mutex<HeldInputs>,
}

/// What came of a start, for the action log.
pub enum Outcome {
    /// Played to the end in the queue, in this many ms.
    Played(f64),
    /// Runs in the background.
    Background,
    /// The same macro runs and ignores new starts.
    Ignored,
    /// Stopped before the end, or sending failed.
    Failed(String),
}

#[derive(Clone)]
pub struct Player {
    inner: Arc<Inner>,
}

impl Player {
    pub fn new(app: AppHandle, diagnostics: Diagnostics) -> Self {
        Self {
            inner: Arc::new(Inner {
                app,
                diagnostics,
                registry: Mutex::new(Registry::default()),
                runs: Mutex::new(HashMap::new()),
                held: Mutex::new(HeldInputs::default()),
            }),
        }
    }

    /// Starts the macro of `request`. A repeating macro runs on a thread of its own and this
    /// returns at once; otherwise it plays here, to the end.
    pub fn play(&self, request: &ActionRequest) -> Outcome {
        let Action::InputMacro(m) = &request.action else {
            return Outcome::Failed("not an input macro".into());
        };
        let start = lock(&self.inner.registry).start(&m.id, m.retrigger);
        let run_id = match start {
            Start::Ignore => return Outcome::Ignored,
            Start::Go { run } => run,
            Start::Restart { old, run } => {
                let previous = lock(&self.inner.runs).get(&old).cloned();
                if let Some(previous) = previous {
                    previous.stop(StopReason::Restarted);
                    previous.wait_done(STOP_WAIT);
                }
                run
            }
        };
        let repetition = Repetition::new(m);
        let run = Arc::new(Run {
            macro_id: m.id.clone(),
            scope: request.scope.clone(),
            while_active: repetition.while_active(),
            signals: Mutex::new(Signals::default()),
            changed: Condvar::new(),
        });
        lock(&self.inner.runs).insert(run_id, run.clone());
        let screens = if request.action.needs_screens() {
            overlay::screens(&self.inner.app)
        } else {
            None
        };
        let (ops, warnings) = request.action.expand(screens.as_ref());
        for warning in warnings {
            log::warn!("macro: {warning}");
        }
        let description = request.action.describe();
        if !m.repeats() {
            let started = Instant::now();
            let (reason, failure) = self.run(&run, run_id, ops, repetition, &description, false);
            return match (reason, failure) {
                (_, Some(error)) => Outcome::Failed(error),
                (StopReason::Finished, None) => {
                    Outcome::Played(started.elapsed().as_secs_f64() * 1000.0)
                }
                (reason, None) => Outcome::Failed(format!("stopped: {}", reason.text())),
            };
        }
        self.log(
            Level::Info,
            format!(
                "Macro started in the background ({}): {description}",
                repeat_text(m)
            ),
        );
        let player = self.clone();
        let spawned = std::thread::Builder::new()
            .name("detent-macro".into())
            .spawn(move || {
                player.run(&run, run_id, ops, repetition, &description, true);
            });
        match spawned {
            Ok(_) => Outcome::Background,
            Err(e) => {
                self.forget(run_id);
                Outcome::Failed(format!("cannot start the macro thread: {e}"))
            }
        }
    }

    /// Plays runs until the repetition ends or the run is stopped. Returns why it ended and
    /// the first sending failure.
    fn run(
        &self,
        run: &Run,
        run_id: u64,
        ops: Vec<InputOp>,
        mut repetition: Repetition,
        description: &str,
        log_end: bool,
    ) -> (StopReason, Option<String>) {
        let mut held = HeldInputs::default();
        let mut failure = None;
        let reason = loop {
            if let Some(reason) =
                self.play_once(run, &ops, &mut held, &mut repetition, &mut failure)
            {
                break reason;
            }
            match repetition.run_finished() {
                Next::Stop(reason) => break reason,
                Next::RunAgain { pause_ms } => {
                    let until = Instant::now() + Duration::from_millis(u64::from(pause_ms));
                    if let Some(reason) = run.sleep_until(until, &mut repetition, false) {
                        break reason;
                    }
                }
            }
        };
        // Stopped before the end: let go of what this run still holds.
        let released = if reason == StopReason::Finished || held.is_empty() {
            String::new()
        } else {
            let names = held.names();
            self.release(&held);
            names
        };
        let stopped_early = reason != StopReason::Finished;
        if log_end || stopped_early {
            let level = if stopped_early && reason != StopReason::StateEnded {
                Level::Warn
            } else {
                Level::Info
            };
            self.log(
                level,
                format!(
                    "{}: {description}",
                    stop_text(reason, repetition.runs(), &released)
                ),
            );
        }
        {
            let mut s = lock(&run.signals);
            s.done = true;
            run.changed.notify_all();
        }
        lock(&self.inner.registry).finished(&run.macro_id, run_id);
        self.forget(run_id);
        (reason, failure)
    }

    /// One run of the macro. Returns why it must stop early, if it must.
    fn play_once(
        &self,
        run: &Run,
        ops: &[InputOp],
        held: &mut HeldInputs,
        repetition: &mut Repetition,
        failure: &mut Option<String>,
    ) -> Option<StopReason> {
        let start = Instant::now();
        let mut due = Duration::ZERO;
        let mut batch: Vec<InputOp> = Vec::new();
        let mut send = |batch: &mut Vec<InputOp>, held: &mut HeldInputs| {
            if batch.is_empty() {
                return;
            }
            if let Err(e) = input::send(batch) {
                failure.get_or_insert(e);
            }
            let mut global = lock(&self.inner.held);
            for op in batch.iter() {
                held.record(op);
                global.record(op);
            }
            batch.clear();
        };
        for op in ops {
            if let InputOp::Sleep { ms } = op {
                send(&mut batch, held);
                due += Duration::from_millis(u64::from(*ms));
                if let Some(reason) = run.sleep_until(start + due, repetition, true) {
                    return Some(reason);
                }
            } else {
                batch.push(*op);
            }
        }
        send(&mut batch, held);
        // A stop that came while sending the last batch.
        run.sleep_until(Instant::now(), repetition, true)
    }

    /// Sends the release of `inputs` and forgets them.
    fn release(&self, inputs: &HeldInputs) {
        let ops = inputs.release_ops();
        if ops.is_empty() {
            return;
        }
        if let Err(e) = input::send(&ops) {
            log::warn!("macro: releasing held inputs failed: {e}");
        }
        let mut global = lock(&self.inner.held);
        for op in &ops {
            global.record(op);
        }
    }

    fn forget(&self, run_id: u64) {
        lock(&self.inner.runs).remove(&run_id);
    }

    fn log(&self, level: Level, text: String) {
        log::info!("macro: {text}");
        self.inner
            .diagnostics
            .record(Record::new(Category::Action, text).level(level));
    }

    /// What the engine reports: control states and profile switches.
    pub fn signal(&self, signals: Vec<MacroSignal>) {
        for signal in signals {
            match signal {
                MacroSignal::State(now) => {
                    for run in lock(&self.inner.runs).values() {
                        if run.while_active
                            && run
                                .scope
                                .as_ref()
                                .is_some_and(|s| s.control == now.control && s.state != now.state)
                        {
                            run.end_state();
                        }
                    }
                }
                // Not on the engine thread that reports it: stopping waits for the runs.
                MacroSignal::ProfileSwitched => {
                    let player = self.clone();
                    let _ = std::thread::Builder::new()
                        .name("detent-macro-stop".into())
                        .spawn(move || player.stop_all(StopReason::ProfileSwitch));
                }
            }
        }
    }

    /// A "Stop macros" action.
    pub fn stop(&self, target: &MacroTarget) -> String {
        match target {
            MacroTarget::All => {
                let n = self.stop_all(StopReason::StopAction);
                format!("{n} running")
            }
            MacroTarget::Macro { id } => {
                let runs: Vec<Arc<Run>> = lock(&self.inner.runs)
                    .values()
                    .filter(|r| r.macro_id == *id)
                    .cloned()
                    .collect();
                for run in &runs {
                    run.stop(StopReason::StopAction);
                }
                if runs.is_empty() {
                    "not running".into()
                } else {
                    "stopped".into()
                }
            }
        }
    }

    /// The panic hotkey: stops every macro and releases everything any macro holds.
    pub fn panic(&self) {
        let n = self.stop_all(StopReason::Panic);
        self.log(
            Level::Warn,
            format!("Panic hotkey: {n} macro(s) stopped, held inputs released"),
        );
        // TODO(vJoy, package G): release vJoy buttons too once the vJoy module offers a
        // release-all (it only releases everything on shutdown so far).
    }

    /// Detent quits: stops everything and waits a moment for the releases.
    pub fn shutdown(&self) {
        self.stop_all(StopReason::AppExit);
    }

    /// Stops all runs, waits briefly for them to let go, then releases whatever is still held
    /// (also by macros that already finished). Returns how many were running.
    fn stop_all(&self, reason: StopReason) -> usize {
        let runs: Vec<Arc<Run>> = lock(&self.inner.runs).values().cloned().collect();
        for run in &runs {
            run.stop(reason);
        }
        for run in &runs {
            run.wait_done(STOP_WAIT);
        }
        let held = std::mem::take(&mut *lock(&self.inner.held));
        if !held.is_empty() {
            let names = held.names();
            let ops = held.release_ops();
            if let Err(e) = input::send(&ops) {
                log::warn!("macro: releasing held inputs failed: {e}");
            }
            self.log(
                Level::Info,
                format!(
                    "Released inputs left pressed by macros ({}): {names}",
                    reason.text()
                ),
            );
        }
        runs.len()
    }
}

fn repeat_text(m: &InputMacro) -> String {
    let base = match m.repeat {
        Repeat::Once => "once".to_owned(),
        Repeat::Times { count } => format!("{count} times"),
        Repeat::WhileActive => "while active".to_owned(),
    };
    if m.pause_ms > 0 {
        format!("{base}, {} ms pause", m.pause_ms)
    } else {
        base
    }
}

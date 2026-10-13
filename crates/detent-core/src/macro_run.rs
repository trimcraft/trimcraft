//! Running input macros: the decisions the app's macro player takes, kept pure so they are
//! tested here.
//!
//! - [`Repetition`]: after each run, run again (after the pause) or stop; what happens when
//!   the control state that started a "while active" macro ends.
//! - [`HeldInputs`]: which keys and mouse buttons a macro pressed and has not released, so
//!   stopping it can release them and nothing stays stuck.
//! - [`Registry`]: the same macro never runs twice at once; a new start is ignored or
//!   restarts it, as the macro says.
//! - [`Scope`]: the control state a macro belongs to.

use crate::action::{InputOp, MouseButton, key_by_scan};
use crate::macros::{InputMacro, OnEnd, Repeat, Retrigger};
use std::collections::HashMap;

/// The control state an action was emitted in: a "while active" macro runs while the control
/// is still in it. `state` is a radio or mode state id, `on`/`off` for toggles and momentary
/// controls, a counter's formatted value.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Scope {
    pub control: String,
    pub state: String,
}

/// Why a macro stopped, for the event log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopReason {
    /// All runs done.
    Finished,
    /// The control state that started it ended.
    StateEnded,
    /// A "Stop macros" action.
    StopAction,
    /// The panic hotkey.
    Panic,
    /// Another profile became active.
    ProfileSwitch,
    /// Started again with "restart".
    Restarted,
    /// Detent quit.
    AppExit,
}

impl StopReason {
    pub fn text(self) -> &'static str {
        match self {
            StopReason::Finished => "finished",
            StopReason::StateEnded => "its state ended",
            StopReason::StopAction => "stopped by a Stop macros action",
            StopReason::Panic => "panic hotkey",
            StopReason::ProfileSwitch => "profile switched",
            StopReason::Restarted => "restarted by its trigger",
            StopReason::AppExit => "Detent quit",
        }
    }
}

/// What to do after a run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Next {
    /// Run again after this pause.
    RunAgain {
        pause_ms: u32,
    },
    Stop(StopReason),
}

/// The repetition of one started macro.
#[derive(Debug, Clone)]
pub struct Repetition {
    repeat: Repeat,
    on_end: OnEnd,
    pause_ms: u32,
    runs: u32,
    state_ended: bool,
}

impl Repetition {
    pub fn new(m: &InputMacro) -> Self {
        Self {
            repeat: m.repeat,
            on_end: m.on_end,
            pause_ms: m.pause_ms,
            runs: 0,
            state_ended: false,
        }
    }

    /// Runs completed so far.
    pub fn runs(&self) -> u32 {
        self.runs
    }

    /// Whether the macro reacts to its state ending.
    pub fn while_active(&self) -> bool {
        self.repeat == Repeat::WhileActive
    }

    /// A run has completed.
    pub fn run_finished(&mut self) -> Next {
        self.runs += 1;
        let again = match self.repeat {
            Repeat::Once => false,
            Repeat::Times { count } => self.runs < count,
            Repeat::WhileActive => !self.state_ended,
        };
        if again {
            Next::RunAgain {
                pause_ms: self.pause_ms,
            }
        } else if self.state_ended {
            Next::Stop(StopReason::StateEnded)
        } else {
            Next::Stop(StopReason::Finished)
        }
    }

    /// The control state that started the macro ended. Returns whether the macro must stop
    /// now; otherwise it finishes the run in progress (`running`) and then stops. Only "while
    /// active" macros react; the others run their count.
    pub fn state_ended(&mut self, running: bool) -> bool {
        if !self.while_active() {
            return false;
        }
        self.state_ended = true;
        !running || self.on_end == OnEnd::StopImmediately
    }
}

/// An input a macro holds down.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Held {
    Key { scan: u16, extended: bool },
    Button(MouseButton),
    VirtualKey(u16),
}

/// The keys and buttons a macro pressed and has not released, in the order pressed.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct HeldInputs {
    held: Vec<Held>,
}

impl HeldInputs {
    /// Notes an operation that was sent.
    pub fn record(&mut self, op: &InputOp) {
        let (input, down) = match *op {
            InputOp::Key {
                scan,
                extended,
                down,
            } => (Held::Key { scan, extended }, down),
            InputOp::MouseButton { button, down } => (Held::Button(button), down),
            InputOp::VirtualKey { vk, down } => (Held::VirtualKey(vk), down),
            _ => return,
        };
        self.held.retain(|h| *h != input);
        if down {
            self.held.push(input);
        }
    }

    pub fn is_empty(&self) -> bool {
        self.held.is_empty()
    }

    /// Operations that release everything held, most recent first (modifiers pressed first
    /// go last, like a person letting go).
    pub fn release_ops(&self) -> Vec<InputOp> {
        self.held
            .iter()
            .rev()
            .map(|h| match *h {
                Held::Key { scan, extended } => InputOp::Key {
                    scan,
                    extended,
                    down: false,
                },
                Held::Button(button) => InputOp::MouseButton {
                    button,
                    down: false,
                },
                Held::VirtualKey(vk) => InputOp::VirtualKey { vk, down: false },
            })
            .collect()
    }

    /// Names of what is held, for the event log, e.g. "LeftCtrl, mouse Left".
    pub fn names(&self) -> String {
        self.held
            .iter()
            .map(|h| match *h {
                Held::Key { scan, extended } => key_by_scan(scan, extended)
                    .map_or_else(|| format!("scan {scan:#04x}"), |k| k.name.to_owned()),
                Held::Button(b) => format!("mouse {b:?}"),
                Held::VirtualKey(vk) => format!("key {vk:#04x}"),
            })
            .collect::<Vec<_>>()
            .join(", ")
    }

    pub fn clear(&mut self) {
        self.held.clear();
    }
}

/// What to do with a macro start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Start {
    /// Start it as run `run`.
    Go { run: u64 },
    /// It runs already and ignores new starts.
    Ignore,
    /// Stop run `old` (it runs already), then start it as run `run`.
    Restart { old: u64, run: u64 },
}

/// The macros running now, by macro id.
#[derive(Debug, Default)]
pub struct Registry {
    running: HashMap<String, u64>,
    next_run: u64,
}

impl Registry {
    /// A macro is to start. Macros without an id (test play) are never guarded.
    pub fn start(&mut self, id: &str, retrigger: Retrigger) -> Start {
        self.next_run += 1;
        let run = self.next_run;
        if id.is_empty() {
            return Start::Go { run };
        }
        match (self.running.get(id).copied(), retrigger) {
            (None, _) => {
                self.running.insert(id.to_owned(), run);
                Start::Go { run }
            }
            (Some(_), Retrigger::Ignore) => Start::Ignore,
            (Some(old), Retrigger::Restart) => {
                self.running.insert(id.to_owned(), run);
                Start::Restart { old, run }
            }
        }
    }

    /// Run `run` of macro `id` has ended (a restarted run's end does not clear its successor).
    pub fn finished(&mut self, id: &str, run: u64) {
        if self.running.get(id) == Some(&run) {
            self.running.remove(id);
        }
    }

    pub fn is_running(&self, id: &str) -> bool {
        self.running.contains_key(id)
    }
}

/// Event log text when a macro stops, e.g. "Macro stopped (its state ended) after 4 runs;
/// released LeftCtrl".
pub fn stop_text(reason: StopReason, runs: u32, released: &str) -> String {
    let runs = match runs {
        1 => "1 run".to_owned(),
        n => format!("{n} runs"),
    };
    let released = if released.is_empty() {
        String::new()
    } else {
        format!("; released {released}")
    };
    format!("Macro stopped ({}) after {runs}{released}", reason.text())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::macros::InputMacro;

    fn mac(repeat: Repeat, on_end: OnEnd, pause_ms: u32) -> InputMacro {
        InputMacro {
            repeat,
            on_end,
            pause_ms,
            ..InputMacro::new(vec![])
        }
    }

    #[test]
    fn once_and_times() {
        let mut once = Repetition::new(&mac(Repeat::Once, OnEnd::default(), 0));
        assert_eq!(once.run_finished(), Next::Stop(StopReason::Finished));
        let mut three = Repetition::new(&mac(Repeat::Times { count: 3 }, OnEnd::default(), 250));
        assert_eq!(three.run_finished(), Next::RunAgain { pause_ms: 250 });
        assert_eq!(three.run_finished(), Next::RunAgain { pause_ms: 250 });
        assert_eq!(three.run_finished(), Next::Stop(StopReason::Finished));
        assert_eq!(three.runs(), 3);
        // Counted repetitions ignore the state.
        let mut two = Repetition::new(&mac(Repeat::Times { count: 2 }, OnEnd::default(), 0));
        assert!(!two.state_ended(true));
        assert_eq!(two.run_finished(), Next::RunAgain { pause_ms: 0 });
    }

    #[test]
    fn while_active_runs_until_the_state_ends() {
        let mut r = Repetition::new(&mac(Repeat::WhileActive, OnEnd::StopImmediately, 100));
        for _ in 0..5 {
            assert_eq!(r.run_finished(), Next::RunAgain { pause_ms: 100 });
        }
        // Default: stop at once, also in the middle of a run.
        assert!(r.state_ended(true));
        assert_eq!(r.run_finished(), Next::Stop(StopReason::StateEnded));
    }

    #[test]
    fn while_active_can_finish_the_current_run() {
        let mut r = Repetition::new(&mac(Repeat::WhileActive, OnEnd::FinishRun, 0));
        assert_eq!(r.run_finished(), Next::RunAgain { pause_ms: 0 });
        // During a run: it goes on to the end of the run, then stops.
        assert!(!r.state_ended(true));
        assert_eq!(r.run_finished(), Next::Stop(StopReason::StateEnded));
        // During the pause between runs there is nothing to finish.
        let mut p = Repetition::new(&mac(Repeat::WhileActive, OnEnd::FinishRun, 500));
        assert_eq!(p.run_finished(), Next::RunAgain { pause_ms: 500 });
        assert!(p.state_ended(false));
    }

    #[test]
    fn held_inputs_are_released_most_recent_first() {
        let ctrl = InputOp::Key {
            scan: 0x1D,
            extended: false,
            down: true,
        };
        let a = InputOp::Key {
            scan: 0x1E,
            extended: false,
            down: true,
        };
        let mut held = HeldInputs::default();
        held.record(&ctrl);
        held.record(&InputOp::MouseButton {
            button: MouseButton::Left,
            down: true,
        });
        held.record(&a);
        held.record(&InputOp::Key {
            scan: 0x1E,
            extended: false,
            down: false,
        });
        // Repeated downs (auto-repeat style) count once.
        held.record(&ctrl);
        held.record(&InputOp::Sleep { ms: 5 });
        assert_eq!(held.names(), "mouse Left, LeftCtrl");
        assert_eq!(
            held.release_ops(),
            [
                InputOp::Key {
                    scan: 0x1D,
                    extended: false,
                    down: false
                },
                InputOp::MouseButton {
                    button: MouseButton::Left,
                    down: false
                },
            ]
        );
        held.record(&InputOp::MouseButton {
            button: MouseButton::Left,
            down: false,
        });
        held.record(&InputOp::Key {
            scan: 0x1D,
            extended: false,
            down: false,
        });
        assert!(held.is_empty());
        assert!(held.release_ops().is_empty());
    }

    #[test]
    fn stop_texts() {
        assert_eq!(
            stop_text(StopReason::StateEnded, 4, "LeftCtrl, mouse Left"),
            "Macro stopped (its state ended) after 4 runs; released LeftCtrl, mouse Left"
        );
        assert_eq!(
            stop_text(StopReason::Finished, 1, ""),
            "Macro stopped (finished) after 1 run"
        );
    }

    #[test]
    fn retrigger_ignores_or_restarts() {
        let mut reg = Registry::default();
        let Start::Go { run: first } = reg.start("mac-1", Retrigger::Ignore) else {
            panic!()
        };
        assert_eq!(reg.start("mac-1", Retrigger::Ignore), Start::Ignore);
        let Start::Restart { old, run: second } = reg.start("mac-1", Retrigger::Restart) else {
            panic!()
        };
        assert_eq!(old, first);
        // The old run ending does not free the new one.
        reg.finished("mac-1", first);
        assert!(reg.is_running("mac-1"));
        reg.finished("mac-1", second);
        assert!(!reg.is_running("mac-1"));
        assert!(matches!(
            reg.start("mac-1", Retrigger::Ignore),
            Start::Go { .. }
        ));
        // Other macros and test plays are independent.
        assert!(matches!(
            reg.start("mac-2", Retrigger::Ignore),
            Start::Go { .. }
        ));
        assert!(matches!(reg.start("", Retrigger::Ignore), Start::Go { .. }));
        assert!(matches!(reg.start("", Retrigger::Ignore), Start::Go { .. }));
    }
}

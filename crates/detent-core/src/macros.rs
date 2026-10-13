//! Input macros: one macro type whose steps can be keyboard and mouse steps in any order (e.g.
//! Ctrl down, click, Ctrl up), with repetition and a concurrency policy.
//!
//! Files from before (schema 3 and older) have separate `keyboardMacro` and `mouseMacro`
//! actions; both load as an input macro with all their settings (see [`crate::Action`]).
//!
//! The runtime side (which macros run, when a repetition stops, which inputs a macro still
//! holds) is in [`crate::macro_run`].

use crate::action::{KeyStep, MouseStep, Movement, Playback};
use crate::overlay::MonitorRef;
use serde::{Deserialize, Serialize};

/// Upper bound for "repeat n times".
pub const MAX_TIMES: u32 = 10_000;
/// Upper bound for the pause between runs.
pub const MAX_PAUSE_MS: u32 = 600_000;

/// One step: a keyboard or a mouse step. A wait is stored as a keyboard step (`delay`); both
/// kinds have the same shape.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MacroStep {
    Key(KeyStep),
    Mouse(MouseStep),
}

impl From<KeyStep> for MacroStep {
    fn from(s: KeyStep) -> Self {
        MacroStep::Key(s)
    }
}

impl From<MouseStep> for MacroStep {
    fn from(s: MouseStep) -> Self {
        match s {
            MouseStep::Delay { ms } => MacroStep::Key(KeyStep::Delay { ms }),
            other => MacroStep::Mouse(other),
        }
    }
}

impl MacroStep {
    pub fn delay(ms: u32) -> Self {
        MacroStep::Key(KeyStep::Delay { ms })
    }

    /// A wait, if this is one.
    pub fn delay_ms(&self) -> Option<u32> {
        match self {
            MacroStep::Key(KeyStep::Delay { ms }) | MacroStep::Mouse(MouseStep::Delay { ms }) => {
                Some(*ms)
            }
            _ => None,
        }
    }

    pub fn is_mouse(&self) -> bool {
        matches!(self, MacroStep::Mouse(s) if !matches!(s, MouseStep::Delay { .. }))
    }

    pub fn is_key(&self) -> bool {
        matches!(self, MacroStep::Key(s) if !matches!(s, KeyStep::Delay { .. }))
    }
}

/// How often a macro runs when it starts.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "type"
)]
pub enum Repeat {
    #[default]
    Once,
    Times {
        count: u32,
    },
    /// As long as the control state that started the macro stays active (a momentary held, a
    /// toggle on, a radio or mode state active).
    WhileActive,
}

/// What happens to a "while active" macro when its state ends.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OnEnd {
    /// Stops at once and releases what it holds.
    #[default]
    StopImmediately,
    /// Finishes the run in progress, then stops.
    FinishRun,
}

/// What a trigger does while the same macro still runs.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Retrigger {
    /// The running macro goes on; the new start is ignored.
    #[default]
    Ignore,
    /// The running macro stops (releasing what it holds) and starts over.
    Restart,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InputMacro {
    /// Identifies the macro for "Stop macros" and the concurrency guard; the engine assigns
    /// one when the control is saved.
    #[serde(default)]
    pub id: String,
    pub steps: Vec<MacroStep>,
    #[serde(default)]
    pub playback: Playback,
    /// How the recorder records mouse movement. Steps keep their own kind either way.
    #[serde(default)]
    pub movement: Movement,
    /// The monitor absolute positions refer to; `None` is the primary monitor.
    #[serde(default)]
    pub monitor: Option<MonitorRef>,
    #[serde(default)]
    pub repeat: Repeat,
    /// For [`Repeat::WhileActive`].
    #[serde(default)]
    pub on_end: OnEnd,
    /// Pause between two runs.
    #[serde(default)]
    pub pause_ms: u32,
    #[serde(default)]
    pub retrigger: Retrigger,
}

impl InputMacro {
    pub fn new(steps: Vec<MacroStep>) -> Self {
        Self {
            id: String::new(),
            steps,
            playback: Playback::default(),
            movement: Movement::default(),
            monitor: None,
            repeat: Repeat::Once,
            on_end: OnEnd::default(),
            pause_ms: 0,
            retrigger: Retrigger::default(),
        }
    }

    /// Repeating macros run in the background; the action queue goes on.
    pub fn repeats(&self) -> bool {
        !matches!(self.repeat, Repeat::Once | Repeat::Times { count: 0 | 1 })
    }

    pub fn has_mouse(&self) -> bool {
        self.steps.iter().any(MacroStep::is_mouse)
    }

    pub fn has_keys(&self) -> bool {
        self.steps.iter().any(MacroStep::is_key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action::MouseButton;

    #[test]
    fn steps_of_both_kinds_round_trip() {
        let json = r#"[{"type":"keyDown","key":"LeftCtrl"},{"type":"click","button":"left","holdMs":null},
            {"type":"delay","ms":20},{"type":"keyUp","key":"LeftCtrl"},{"type":"wheel","delta":-1}]"#;
        let steps: Vec<MacroStep> = serde_json::from_str(json).unwrap();
        assert_eq!(
            steps[1],
            MacroStep::Mouse(MouseStep::Click {
                button: MouseButton::Left,
                hold_ms: None
            })
        );
        assert_eq!(steps[2], MacroStep::delay(20));
        assert!(steps[0].is_key() && steps[1].is_mouse() && !steps[2].is_mouse());
        let back: Vec<MacroStep> =
            serde_json::from_str(&serde_json::to_string(&steps).unwrap()).unwrap();
        assert_eq!(back, steps);
        assert_eq!(
            MacroStep::from(MouseStep::Delay { ms: 5 }),
            MacroStep::delay(5)
        );
    }

    #[test]
    fn options_default_and_serialise() {
        let m: InputMacro = serde_json::from_str(r#"{"steps":[]}"#).unwrap();
        assert_eq!(m, InputMacro::new(vec![]));
        assert!(!m.repeats());
        let json = serde_json::to_value(InputMacro {
            repeat: Repeat::Times { count: 3 },
            on_end: OnEnd::FinishRun,
            retrigger: Retrigger::Restart,
            ..InputMacro::new(vec![])
        })
        .unwrap();
        assert_eq!(json["repeat"]["type"], "times");
        assert_eq!(json["repeat"]["count"], 3);
        assert_eq!(json["onEnd"], "finishRun");
        assert_eq!(json["retrigger"], "restart");
        let w: Repeat = serde_json::from_str(r#"{"type":"whileActive"}"#).unwrap();
        assert_eq!(w, Repeat::WhileActive);
        assert!(
            InputMacro {
                repeat: w,
                ..InputMacro::new(vec![])
            }
            .repeats()
        );
    }
}

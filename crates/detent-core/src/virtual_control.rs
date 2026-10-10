//! Virtual controls: named controls built from physical inputs, with actions attached.
//!
//! The first kind is the [`Radio`]: a set of states of which at most one is active, like the
//! preset buttons of a car radio. Triggers (button presses, hat directions, encoder detents)
//! switch states; entering and leaving a state emits its actions.
//!
//! Everything here is pure. A transition returns the actions to run; executing them is the
//! app's job.

use crate::action::{Action, ActionError};
use crate::device::DeviceId;
use crate::function::EndBehavior;
use crate::state::HatState;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum HatDirection {
    Up,
    UpRight,
    Right,
    DownRight,
    Down,
    DownLeft,
    Left,
    UpLeft,
}

impl HatDirection {
    pub fn from_state(state: HatState) -> Option<Self> {
        const U: u8 = HatState::UP;
        const R: u8 = HatState::RIGHT;
        const D: u8 = HatState::DOWN;
        const L: u8 = HatState::LEFT;
        Some(match state.0 {
            x if x == U => Self::Up,
            x if x == U | R => Self::UpRight,
            x if x == R => Self::Right,
            x if x == D | R => Self::DownRight,
            x if x == D => Self::Down,
            x if x == D | L => Self::DownLeft,
            x if x == L => Self::Left,
            x if x == U | L => Self::UpLeft,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StepDirection {
    Decrement,
    Increment,
}

/// Something on a controller that fires once: a press, a hat direction entered, a detent.
///
/// The same type describes what happened (an input edge) and what a control listens to, so
/// matching is equality. More kinds (axis zones, modifier combos) can be added as variants.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum Trigger {
    /// Fires when the button is pressed, not when released.
    Button { device: DeviceId, index: u16 },
    /// Fires when the hat enters the direction.
    Hat {
        device: DeviceId,
        index: u16,
        direction: HatDirection,
    },
    /// Fires once per detent of a configured encoder.
    EncoderStep {
        encoder: String,
        direction: StepDirection,
    },
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Startup {
    /// Restore the state that was active when Detent last ran.
    #[default]
    RestoreLast,
    /// Start with no state active.
    Empty,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RadioState {
    pub id: String,
    pub name: String,
    /// Makes this state active.
    pub select: Vec<Trigger>,
    pub on_activate: Vec<Action>,
    pub on_deactivate: Vec<Action>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Radio {
    pub states: Vec<RadioState>,
    pub increment: Vec<Trigger>,
    pub decrement: Vec<Trigger>,
    /// Deactivates whatever state is active.
    pub clear: Vec<Trigger>,
    pub end: EndBehavior,
    /// Selecting the active state again deactivates it.
    pub toggle_on_reselect: bool,
    pub startup: Startup,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum VirtualControlKind {
    Radio(Radio),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VirtualControl {
    pub id: String,
    /// Shown in the overlay, e.g. "Comms".
    pub name: String,
    pub kind: VirtualControlKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RadioCommand {
    Increment,
    Decrement,
    /// Index into [`Radio::states`].
    Select(usize),
    Clear,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Phase {
    Deactivate,
    Activate,
}

/// An action to run, with where it came from (for the log).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionRequest {
    pub control: String,
    pub state: String,
    pub phase: Phase,
    pub action: Action,
}

#[derive(Debug, Error, PartialEq)]
pub enum ControlError {
    #[error("a radio needs at least one state")]
    NoStates,
    #[error("state id \"{0}\" is used twice")]
    DuplicateState(String),
    #[error(transparent)]
    Action(#[from] ActionError),
}

impl Radio {
    pub fn index_of(&self, state_id: &str) -> Option<usize> {
        self.states.iter().position(|s| s.id == state_id)
    }

    /// The active state after `command`, given the active state before.
    pub fn next(&self, active: Option<usize>, command: RadioCommand) -> Option<usize> {
        let len = self.states.len();
        if len == 0 {
            return None;
        }
        let last = len - 1;
        match (command, active) {
            (RadioCommand::Increment, None) => Some(0),
            (RadioCommand::Decrement, None) => Some(last),
            (RadioCommand::Increment, Some(i)) if i >= last => match self.end {
                EndBehavior::Wrap => Some(0),
                EndBehavior::Clamp => Some(last),
            },
            (RadioCommand::Increment, Some(i)) => Some(i + 1),
            (RadioCommand::Decrement, Some(0)) => match self.end {
                EndBehavior::Wrap => Some(last),
                EndBehavior::Clamp => Some(0),
            },
            (RadioCommand::Decrement, Some(i)) => Some(i.min(len) - 1),
            (RadioCommand::Select(i), _) if i > last => active,
            (RadioCommand::Select(i), Some(a)) if a == i => {
                if self.toggle_on_reselect {
                    None
                } else {
                    active
                }
            }
            (RadioCommand::Select(i), _) => Some(i),
            (RadioCommand::Clear, _) => None,
        }
    }

    /// Actions for switching from `from` to `to`: the old state's `on_deactivate`, then the
    /// new state's `on_activate`. Nothing if the state does not change.
    pub fn transition_actions(
        &self,
        control: &str,
        from: Option<usize>,
        to: Option<usize>,
    ) -> Vec<ActionRequest> {
        if from == to {
            return Vec::new();
        }
        let mut out = Vec::new();
        if let Some(state) = from.and_then(|i| self.states.get(i)) {
            out.extend(state.on_deactivate.iter().map(|a| ActionRequest {
                control: control.into(),
                state: state.name.clone(),
                phase: Phase::Deactivate,
                action: a.clone(),
            }));
        }
        if let Some(state) = to.and_then(|i| self.states.get(i)) {
            out.extend(state.on_activate.iter().map(|a| ActionRequest {
                control: control.into(),
                state: state.name.clone(),
                phase: Phase::Activate,
                action: a.clone(),
            }));
        }
        out
    }

    /// Commands `fired` triggers, in a fixed order: increment, decrement, per-state select
    /// (in state order), clear.
    pub fn commands_for(&self, fired: &Trigger) -> Vec<RadioCommand> {
        let mut out = Vec::new();
        if self.increment.contains(fired) {
            out.push(RadioCommand::Increment);
        }
        if self.decrement.contains(fired) {
            out.push(RadioCommand::Decrement);
        }
        for (i, state) in self.states.iter().enumerate() {
            if state.select.contains(fired) {
                out.push(RadioCommand::Select(i));
            }
        }
        if self.clear.contains(fired) {
            out.push(RadioCommand::Clear);
        }
        out
    }

    pub fn triggers_mut(&mut self) -> impl Iterator<Item = &mut Vec<Trigger>> {
        [&mut self.increment, &mut self.decrement, &mut self.clear]
            .into_iter()
            .chain(self.states.iter_mut().map(|s| &mut s.select))
    }

    pub fn triggers(&self) -> impl Iterator<Item = &Trigger> {
        self.increment
            .iter()
            .chain(&self.decrement)
            .chain(&self.clear)
            .chain(self.states.iter().flat_map(|s| &s.select))
    }

    /// Validates the radio and fills in missing state ids and names.
    pub fn normalize(&mut self) -> Result<(), ControlError> {
        if self.states.is_empty() {
            return Err(ControlError::NoStates);
        }
        let mut next = 1 + self
            .states
            .iter()
            .filter_map(|s| s.id.strip_prefix("st-")?.parse::<u32>().ok())
            .max()
            .unwrap_or(0);
        let mut seen = std::collections::HashSet::new();
        for (i, state) in self.states.iter_mut().enumerate() {
            if state.id.is_empty() {
                state.id = format!("st-{next}");
                next += 1;
            }
            if !seen.insert(state.id.clone()) {
                return Err(ControlError::DuplicateState(state.id.clone()));
            }
            state.name = state.name.trim().to_owned();
            if state.name.is_empty() {
                state.name = format!("State {}", i + 1);
            }
            for action in state.on_activate.iter_mut().chain(&mut state.on_deactivate) {
                action.normalize()?;
            }
        }
        for slot in self.triggers_mut() {
            let mut unique = Vec::with_capacity(slot.len());
            for t in slot.drain(..) {
                if !unique.contains(&t) {
                    unique.push(t);
                }
            }
            *slot = unique;
        }
        Ok(())
    }
}

impl VirtualControl {
    pub fn radio(&self) -> &Radio {
        let VirtualControlKind::Radio(r) = &self.kind;
        r
    }

    pub fn radio_mut(&mut self) -> &mut Radio {
        let VirtualControlKind::Radio(r) = &mut self.kind;
        r
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action::KeyStep;

    fn key(name: &str) -> Action {
        Action::KeyboardMacro {
            steps: vec![KeyStep::KeyTap {
                key: name.into(),
                hold_ms: None,
            }],
        }
    }

    fn state(id: &str) -> RadioState {
        RadioState {
            id: id.into(),
            name: id.to_uppercase(),
            select: vec![],
            on_activate: vec![key(&format!("{id}-on"))],
            on_deactivate: vec![key(&format!("{id}-off"))],
        }
    }

    fn radio(end: EndBehavior) -> Radio {
        Radio {
            states: vec![state("a"), state("b"), state("c")],
            end,
            ..Default::default()
        }
    }

    use RadioCommand::*;

    #[test]
    fn increment_and_decrement_from_nothing() {
        let r = radio(EndBehavior::Wrap);
        assert_eq!(r.next(None, Increment), Some(0));
        assert_eq!(r.next(None, Decrement), Some(2));
    }

    #[test]
    fn wrap_goes_round() {
        let r = radio(EndBehavior::Wrap);
        assert_eq!(r.next(Some(0), Increment), Some(1));
        assert_eq!(r.next(Some(2), Increment), Some(0));
        assert_eq!(r.next(Some(0), Decrement), Some(2));
        assert_eq!(r.next(Some(1), Decrement), Some(0));
    }

    #[test]
    fn clamp_stops_at_ends() {
        let r = radio(EndBehavior::Clamp);
        assert_eq!(r.next(Some(2), Increment), Some(2));
        assert_eq!(r.next(Some(0), Decrement), Some(0));
    }

    #[test]
    fn select_and_reselect() {
        let mut r = radio(EndBehavior::Wrap);
        assert_eq!(r.next(None, Select(1)), Some(1));
        assert_eq!(r.next(Some(0), Select(1)), Some(1));
        assert_eq!(r.next(Some(1), Select(1)), Some(1));
        r.toggle_on_reselect = true;
        assert_eq!(r.next(Some(1), Select(1)), None);
        assert_eq!(r.next(Some(0), Select(1)), Some(1));
        assert_eq!(r.next(Some(0), Select(7)), Some(0));
    }

    #[test]
    fn clear() {
        let r = radio(EndBehavior::Wrap);
        assert_eq!(r.next(Some(1), Clear), None);
        assert_eq!(r.next(None, Clear), None);
    }

    #[test]
    fn no_states_means_nothing_active() {
        let r = Radio::default();
        assert_eq!(r.next(None, Increment), None);
    }

    #[test]
    fn switching_deactivates_then_activates() {
        let r = radio(EndBehavior::Wrap);
        let actions: Vec<(String, Phase, Action)> = r
            .transition_actions("radio", Some(0), Some(1))
            .into_iter()
            .map(|a| (a.state, a.phase, a.action))
            .collect();
        assert_eq!(
            actions,
            [
                ("A".to_string(), Phase::Deactivate, key("a-off")),
                ("B".to_string(), Phase::Activate, key("b-on")),
            ]
        );
        assert_eq!(r.transition_actions("radio", None, Some(2)).len(), 1);
        assert_eq!(r.transition_actions("radio", Some(2), None).len(), 1);
        assert!(r.transition_actions("radio", Some(1), Some(1)).is_empty());
        assert!(r.transition_actions("radio", None, None).is_empty());
    }

    #[test]
    fn commands_follow_fixed_order() {
        let t = Trigger::Button {
            device: DeviceId("d".into()),
            index: 1,
        };
        let mut r = radio(EndBehavior::Wrap);
        r.clear.push(t.clone());
        r.states[2].select.push(t.clone());
        r.increment.push(t.clone());
        assert_eq!(r.commands_for(&t), [Increment, Select(2), Clear]);
        let other = Trigger::Button {
            device: DeviceId("d".into()),
            index: 2,
        };
        assert!(r.commands_for(&other).is_empty());
    }

    #[test]
    fn normalize_fills_ids_names_and_dedupes() {
        let t = Trigger::EncoderStep {
            encoder: "enc-1".into(),
            direction: StepDirection::Increment,
        };
        let mut r = Radio {
            states: vec![
                RadioState {
                    id: "st-4".into(),
                    ..Default::default()
                },
                RadioState::default(),
            ],
            increment: vec![t.clone(), t.clone()],
            ..Default::default()
        };
        r.normalize().unwrap();
        assert_eq!(r.states[1].id, "st-5");
        assert_eq!(r.states[1].name, "State 2");
        assert_eq!(r.increment, [t]);

        r.states[1].id = "st-4".into();
        assert_eq!(
            r.normalize(),
            Err(ControlError::DuplicateState("st-4".into()))
        );
        assert_eq!(Radio::default().normalize(), Err(ControlError::NoStates));
    }

    #[test]
    fn hat_directions_from_mask() {
        assert_eq!(
            HatDirection::from_state(HatState(HatState::UP | HatState::RIGHT)),
            Some(HatDirection::UpRight)
        );
        assert_eq!(HatDirection::from_state(HatState::CENTERED), None);
        assert_eq!(
            HatDirection::from_state(HatState(HatState::UP | HatState::DOWN)),
            None
        );
    }

    #[test]
    fn json_shape_is_stable() {
        let vc = VirtualControl {
            id: "vc-1".into(),
            name: "Comms".into(),
            kind: VirtualControlKind::Radio(Radio {
                states: vec![RadioState {
                    id: "st-1".into(),
                    name: "Guard".into(),
                    select: vec![Trigger::Hat {
                        device: DeviceId("f00d:0001#1".into()),
                        index: 0,
                        direction: HatDirection::UpLeft,
                    }],
                    on_activate: vec![],
                    on_deactivate: vec![],
                }],
                increment: vec![Trigger::EncoderStep {
                    encoder: "enc-1".into(),
                    direction: StepDirection::Increment,
                }],
                ..Default::default()
            }),
        };
        let json = serde_json::to_value(&vc).unwrap();
        assert_eq!(json["kind"]["type"], "radio");
        assert_eq!(json["kind"]["end"], "wrap");
        assert_eq!(json["kind"]["startup"], "restoreLast");
        assert_eq!(json["kind"]["toggleOnReselect"], false);
        assert_eq!(json["kind"]["increment"][0]["type"], "encoderStep");
        assert_eq!(json["kind"]["increment"][0]["direction"], "increment");
        assert_eq!(
            json["kind"]["states"][0]["select"][0]["direction"],
            "upLeft"
        );
        assert_eq!(serde_json::from_value::<VirtualControl>(json).unwrap(), vc);

        // Optional settings may be left out.
        let minimal: VirtualControl = serde_json::from_str(
            r#"{"id":"vc-2","name":"R","kind":{"type":"radio","states":[{"id":"s","name":"S"}]}}"#,
        )
        .unwrap();
        assert_eq!(minimal.radio().startup, Startup::RestoreLast);
        assert!(!minimal.radio().toggle_on_reselect);
    }
}

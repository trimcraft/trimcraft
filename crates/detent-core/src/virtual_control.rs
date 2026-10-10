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

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RadioState {
    pub id: String,
    pub name: String,
    /// Makes this state active.
    pub select: Vec<Trigger>,
    pub on_activate: Vec<Action>,
    pub on_deactivate: Vec<Action>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Radio {
    pub states: Vec<RadioState>,
    pub increment: Vec<Trigger>,
    pub decrement: Vec<Trigger>,
    /// Deactivates whatever state is active.
    pub clear: Vec<Trigger>,
    /// Deactivates the active state, or reactivates the most recently active one if none is.
    pub toggle_off: Vec<Trigger>,
    pub end: EndBehavior,
    /// Selecting the active state again deactivates it.
    pub toggle_on_reselect: bool,
    pub startup: Startup,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum VirtualControlKind {
    Radio(Radio),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
    /// Off if a state is active, otherwise back to the most recently active state.
    ToggleOff,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Phase {
    Deactivate,
    Activate,
}

/// An action to run, with where it came from (for the log).
#[derive(Debug, Clone, PartialEq)]
pub struct ActionRequest {
    pub control: String,
    pub state: String,
    pub phase: Phase,
    pub action: Action,
}

/// What is remembered per radio between transitions and across restarts (control-states.json).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", from = "SavedMemory")]
pub struct RadioMemory {
    /// Id of the active state.
    pub active: Option<String>,
    /// Id of the most recently active state, for [`RadioCommand::ToggleOff`].
    pub last: Option<String>,
}

/// control-states.json entries: a plain state id in files from before toggle off.
#[derive(Deserialize)]
#[serde(untagged)]
enum SavedMemory {
    Active(String),
    Full {
        #[serde(default)]
        active: Option<String>,
        #[serde(default)]
        last: Option<String>,
    },
}

impl From<SavedMemory> for RadioMemory {
    fn from(saved: SavedMemory) -> Self {
        match saved {
            SavedMemory::Active(active) => Self {
                active: Some(active),
                last: None,
            },
            SavedMemory::Full { active, last } => Self { active, last },
        }
    }
}

impl RadioMemory {
    pub fn is_empty(&self) -> bool {
        self.active.is_none() && self.last.is_none()
    }

    /// Forgets states the radio no longer has.
    pub fn retain_existing(&mut self, radio: &Radio) {
        for slot in [&mut self.active, &mut self.last] {
            if slot
                .as_deref()
                .is_some_and(|id| radio.index_of(id).is_none())
            {
                *slot = None;
            }
        }
    }
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

    /// The active state after `command`, given the active state before and the most recently
    /// active one (`recent`, for [`RadioCommand::ToggleOff`]).
    pub fn next(
        &self,
        active: Option<usize>,
        recent: Option<usize>,
        command: RadioCommand,
    ) -> Option<usize> {
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
            (RadioCommand::Clear | RadioCommand::ToggleOff, Some(_)) => None,
            (RadioCommand::Clear, None) => None,
            // Nothing remembered (or it no longer exists): the first state.
            (RadioCommand::ToggleOff, None) => Some(recent.filter(|&r| r <= last).unwrap_or(0)),
        }
    }

    /// Runs `command`: updates `memory` and returns the actions to run, or `None` if the
    /// active state does not change. A state that is left becomes the remembered one.
    pub fn apply(
        &self,
        control: &str,
        memory: &mut RadioMemory,
        command: RadioCommand,
    ) -> Option<Vec<ActionRequest>> {
        let from = memory.active.as_deref().and_then(|id| self.index_of(id));
        let recent = memory.last.as_deref().and_then(|id| self.index_of(id));
        let to = self.next(from, recent, command);
        if from == to {
            return None;
        }
        let actions = self.transition_actions(control, from, to);
        self.set(memory, to);
        Some(actions)
    }

    /// Sets the active state without actions; the state left (if any) is remembered.
    pub fn set(&self, memory: &mut RadioMemory, to: Option<usize>) {
        let from = memory.active.as_deref().and_then(|id| self.index_of(id));
        if from == to {
            return;
        }
        if let Some(f) = from {
            memory.last = Some(self.states[f].id.clone());
        }
        memory.active = to.map(|i| self.states[i].id.clone());
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
    /// (in state order), clear, toggle off.
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
        if self.toggle_off.contains(fired) {
            out.push(RadioCommand::ToggleOff);
        }
        out
    }

    pub fn triggers_mut(&mut self) -> impl Iterator<Item = &mut Vec<Trigger>> {
        [
            &mut self.increment,
            &mut self.decrement,
            &mut self.clear,
            &mut self.toggle_off,
        ]
        .into_iter()
        .chain(self.states.iter_mut().map(|s| &mut s.select))
    }

    pub fn triggers(&self) -> impl Iterator<Item = &Trigger> {
        self.increment
            .iter()
            .chain(&self.decrement)
            .chain(&self.clear)
            .chain(&self.toggle_off)
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
        Action::keyboard(vec![KeyStep::KeyTap {
            key: name.into(),
            hold_ms: None,
        }])
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
        assert_eq!(r.next(None, None, Increment), Some(0));
        assert_eq!(r.next(None, None, Decrement), Some(2));
    }

    #[test]
    fn wrap_goes_round() {
        let r = radio(EndBehavior::Wrap);
        assert_eq!(r.next(Some(0), None, Increment), Some(1));
        assert_eq!(r.next(Some(2), None, Increment), Some(0));
        assert_eq!(r.next(Some(0), None, Decrement), Some(2));
        assert_eq!(r.next(Some(1), None, Decrement), Some(0));
    }

    #[test]
    fn clamp_stops_at_ends() {
        let r = radio(EndBehavior::Clamp);
        assert_eq!(r.next(Some(2), None, Increment), Some(2));
        assert_eq!(r.next(Some(0), None, Decrement), Some(0));
    }

    #[test]
    fn select_and_reselect() {
        let mut r = radio(EndBehavior::Wrap);
        assert_eq!(r.next(None, None, Select(1)), Some(1));
        assert_eq!(r.next(Some(0), None, Select(1)), Some(1));
        assert_eq!(r.next(Some(1), None, Select(1)), Some(1));
        r.toggle_on_reselect = true;
        assert_eq!(r.next(Some(1), None, Select(1)), None);
        assert_eq!(r.next(Some(0), None, Select(1)), Some(1));
        assert_eq!(r.next(Some(0), None, Select(7)), Some(0));
    }

    #[test]
    fn clear() {
        let r = radio(EndBehavior::Wrap);
        assert_eq!(r.next(Some(1), None, Clear), None);
        assert_eq!(r.next(None, None, Clear), None);
    }

    #[test]
    fn no_states_means_nothing_active() {
        let r = Radio::default();
        assert_eq!(r.next(None, None, Increment), None);
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

    fn memory(active: Option<&str>, last: Option<&str>) -> RadioMemory {
        RadioMemory {
            active: active.map(Into::into),
            last: last.map(Into::into),
        }
    }

    /// Phases and states of the actions, e.g. ["-A", "+B"].
    fn trace(actions: Option<Vec<ActionRequest>>) -> Vec<String> {
        actions
            .unwrap_or_default()
            .into_iter()
            .map(|a| {
                let sign = if a.phase == Phase::Activate { '+' } else { '-' };
                format!("{sign}{}", a.state)
            })
            .collect()
    }

    #[test]
    fn toggle_off_deactivates_and_reactivates_the_last_state() {
        let r = radio(EndBehavior::Wrap);
        let mut m = memory(Some("b"), None);
        assert_eq!(trace(r.apply("radio", &mut m, ToggleOff)), ["-B"]);
        assert_eq!(m, memory(None, Some("b")));
        assert_eq!(trace(r.apply("radio", &mut m, ToggleOff)), ["+B"]);
        assert_eq!(m, memory(Some("b"), Some("b")));
    }

    #[test]
    fn toggle_off_without_memory_activates_the_first_state() {
        let r = radio(EndBehavior::Wrap);
        let mut m = RadioMemory::default();
        assert_eq!(trace(r.apply("radio", &mut m, ToggleOff)), ["+A"]);
        // A remembered state that no longer exists counts as nothing remembered.
        let mut m = memory(None, Some("gone"));
        assert_eq!(trace(r.apply("radio", &mut m, ToggleOff)), ["+A"]);
        assert_eq!(r.next(None, Some(9), ToggleOff), Some(0));
    }

    #[test]
    fn every_deactivation_is_remembered() {
        let mut r = radio(EndBehavior::Wrap);
        // Switching from A to B remembers A.
        let mut m = memory(Some("a"), None);
        assert_eq!(trace(r.apply("radio", &mut m, Select(1))), ["-A", "+B"]);
        assert_eq!(m, memory(Some("b"), Some("a")));
        // All off.
        assert_eq!(trace(r.apply("radio", &mut m, Clear)), ["-B"]);
        assert_eq!(m, memory(None, Some("b")));
        assert_eq!(trace(r.apply("radio", &mut m, ToggleOff)), ["+B"]);
        // Reselect with toggle_on_reselect.
        r.toggle_on_reselect = true;
        let mut m = memory(Some("c"), Some("a"));
        assert_eq!(trace(r.apply("radio", &mut m, Select(2))), ["-C"]);
        assert_eq!(m, memory(None, Some("c")));
        // Increment and decrement too.
        let mut m = memory(Some("c"), None);
        r.apply("radio", &mut m, Increment);
        assert_eq!(m, memory(Some("a"), Some("c")));
        // A silent set as well.
        r.set(&mut m, None);
        assert_eq!(m, memory(None, Some("a")));
        // Nothing changes, nothing is remembered.
        assert!(r.apply("radio", &mut m, Clear).is_none());
        assert_eq!(m, memory(None, Some("a")));
    }

    #[test]
    fn memory_forgets_deleted_states() {
        let r = radio(EndBehavior::Wrap);
        let mut m = memory(Some("x"), Some("b"));
        m.retain_existing(&r);
        assert_eq!(m, memory(None, Some("b")));
        let mut m = memory(Some("a"), Some("y"));
        m.retain_existing(&r);
        assert_eq!(m, memory(Some("a"), None));
    }

    #[test]
    fn memory_file_shape_and_old_entries() {
        let m = memory(None, Some("st-2"));
        let json = serde_json::to_string(&m).unwrap();
        assert_eq!(json, r#"{"active":null,"last":"st-2"}"#);
        assert_eq!(serde_json::from_str::<RadioMemory>(&json).unwrap(), m);
        // Files written before toggle off hold just the active state id.
        assert_eq!(
            serde_json::from_str::<RadioMemory>(r#""st-1""#).unwrap(),
            memory(Some("st-1"), None)
        );
        assert_eq!(
            serde_json::from_str::<RadioMemory>(r#"{"active":"st-3"}"#).unwrap(),
            memory(Some("st-3"), None)
        );
    }

    #[test]
    fn toggle_off_trigger_comes_last() {
        let t = Trigger::Button {
            device: DeviceId("d".into()),
            index: 1,
        };
        let mut r = radio(EndBehavior::Wrap);
        r.toggle_off.push(t.clone());
        r.clear.push(t.clone());
        assert_eq!(r.commands_for(&t), [Clear, ToggleOff]);
        assert!(r.triggers().any(|x| *x == t));
    }
}

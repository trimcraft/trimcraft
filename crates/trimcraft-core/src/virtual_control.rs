//! Virtual controls: named controls built from physical inputs, with actions attached.
//!
//! - [`Radio`]: states of which at most one is active, like the preset buttons of a car
//!   radio.
//! - [`Toggle`]: on or off.
//! - [`Momentary`]: on only while a trigger is held.
//! - [`CounterControl`]: a value with range and step, like the Hardware Test counters.
//! - [`Mode`]: states of which exactly one is active (a layer); any trigger can be limited to
//!   a mode state with a [`Condition`](crate::trigger::Condition).
//!
//! Triggers are matched by the engine (see [`crate::trigger`]); each control lists its
//! triggers with the command they run ([`VirtualControl::bindings`]). Everything here is
//! pure: a transition returns the actions to run; executing them is the app's job.

use crate::action::{Action, ActionError};
use crate::function::{CounterSpec, EndBehavior, SpecError};
use crate::macro_run::Scope;
use crate::placeholder::Placeholders;
use crate::vjoy::{ButtonOutput, VjoyAxisRef, VjoyError};
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub use crate::trigger::{
    ButtonRef, Condition, HatDirection, PressKind, StepDirection, Trigger, TriggerError,
    TriggerInput,
};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Startup {
    /// Restore the state (or value) that was active when Trimcraft last ran.
    #[default]
    RestoreLast,
    /// Start empty: no radio state, toggle off, counter at its initial value, mode in its
    /// first state.
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
    /// vJoy button held while this state is active (or pulsed on entering it).
    pub output: Option<ButtonOutput>,
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

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Toggle {
    /// Switches between on and off.
    pub toggle: Vec<Trigger>,
    pub on: Vec<Trigger>,
    pub off: Vec<Trigger>,
    pub on_on: Vec<Action>,
    pub on_off: Vec<Action>,
    /// `Empty`: off at startup.
    pub startup: Startup,
    /// vJoy button held while on (or pulsed on switching on).
    pub output: Option<ButtonOutput>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Momentary {
    /// Active while any of them is held.
    pub triggers: Vec<Trigger>,
    pub on_press: Vec<Action>,
    pub on_release: Vec<Action>,
    /// Active while nothing is held instead.
    pub invert: bool,
    /// vJoy button held while active (or pulsed on activating).
    pub output: Option<ButtonOutput>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CounterControl {
    pub spec: CounterSpec,
    pub increment: Vec<Trigger>,
    pub decrement: Vec<Trigger>,
    /// Back to the initial value.
    pub reset: Vec<Trigger>,
    /// Runs on every change; actions can use the new value as `{value}`.
    pub on_change: Vec<Action>,
    /// `Empty`: the initial value at startup.
    pub startup: Startup,
    /// vJoy axis set in proportion to the value's position between minimum and maximum.
    pub output: Option<VjoyAxisRef>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ModeState {
    pub id: String,
    pub name: String,
    /// Makes this state active (latching).
    pub select: Vec<Trigger>,
    /// Makes this state active while held; on release the mode goes back.
    pub shift: Vec<Trigger>,
    pub on_activate: Vec<Action>,
    pub on_deactivate: Vec<Action>,
    /// vJoy button held while this state is active (or pulsed on entering it).
    pub output: Option<ButtonOutput>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Mode {
    /// Exactly one is active; the first one by default.
    pub states: Vec<ModeState>,
    pub next: Vec<Trigger>,
    pub previous: Vec<Trigger>,
    pub end: EndBehavior,
    /// `Empty`: the first state at startup.
    pub startup: Startup,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum VirtualControlKind {
    Radio(Radio),
    Toggle(Toggle),
    Momentary(Momentary),
    Counter(CounterControl),
    Mode(Mode),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VirtualControl {
    pub id: String,
    /// Shown in the overlay, e.g. "Comms".
    pub name: String,
    pub kind: VirtualControlKind,
    /// Show an overlay toast when its state or value changes.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub show_toast: bool,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToggleCommand {
    Toggle,
    On,
    Off,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CounterCommand {
    Increment,
    Decrement,
    Reset,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModeCommand {
    Next,
    Previous,
    /// Index into [`Mode::states`].
    Select(usize),
    /// Index into [`Mode::states`]: active while the trigger is held.
    Shift(usize),
}

/// What a trigger does to its control.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    Radio(RadioCommand),
    Toggle(ToggleCommand),
    /// Held: the momentary control is pressed until the trigger's input is released.
    Momentary,
    Counter(CounterCommand),
    Mode(ModeCommand),
}

impl Command {
    /// The command lasts while the input is held (it needs a release).
    pub fn is_hold(self) -> bool {
        matches!(
            self,
            Command::Momentary | Command::Mode(ModeCommand::Shift(_))
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Phase {
    Deactivate,
    Activate,
}

/// An action to run, with where it came from (for the log and the placeholders).
#[derive(Debug, Clone, PartialEq)]
pub struct ActionRequest {
    pub control: String,
    pub state: String,
    pub phase: Phase,
    pub action: Action,
    /// The value an action may insert as `{value}` (counters: the new value, formatted).
    pub value: Option<String>,
    /// Name of the profile the emitting control belongs to, for `{profile}`; the engine
    /// fills it in.
    pub profile: String,
    /// The control state the action was emitted in, for "while active" macros; the engine
    /// fills it in.
    pub scope: Option<Scope>,
}

impl ActionRequest {
    pub fn new(control: &str, state: &str, phase: Phase, action: &Action) -> Self {
        Self {
            control: control.into(),
            state: state.into(),
            phase,
            action: action.clone(),
            value: None,
            profile: String::new(),
            scope: None,
        }
    }

    /// The values of `{control}`, `{state}`, `{profile}` and `{value}`.
    pub fn placeholders(&self) -> Placeholders {
        Placeholders {
            control: self.control.clone(),
            state: self.state.clone(),
            profile: self.profile.clone(),
            value: self.value.clone(),
        }
    }

    /// `text` with all placeholders filled in (`{value}` empty without a value).
    pub fn fill_placeholders(&self, text: &str) -> String {
        self.placeholders().fill(text)
    }
}

/// What is remembered per control between transitions and across restarts
/// (control-states.json): the active state (radio, mode; `"on"` for a toggle), the most
/// recently active one, and a counter's value.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", from = "SavedMemory")]
pub struct RadioMemory {
    /// Id of the active state.
    pub active: Option<String>,
    /// Id of the most recently active state, for [`RadioCommand::ToggleOff`].
    pub last: Option<String>,
    /// A counter's value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<f64>,
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
        #[serde(default)]
        value: Option<f64>,
    },
}

impl From<SavedMemory> for RadioMemory {
    fn from(saved: SavedMemory) -> Self {
        match saved {
            SavedMemory::Active(active) => Self {
                active: Some(active),
                last: None,
                value: None,
            },
            SavedMemory::Full {
                active,
                last,
                value,
            } => Self {
                active,
                last,
                value,
            },
        }
    }
}

/// The state id a toggle remembers while on.
pub const TOGGLE_ON: &str = "on";

impl RadioMemory {
    pub fn is_empty(&self) -> bool {
        self.active.is_none() && self.last.is_none() && self.value.is_none()
    }

    /// Forgets what the control no longer has.
    pub fn retain_existing(&mut self, control: &VirtualControl) {
        let exists = |id: &str| match &control.kind {
            VirtualControlKind::Radio(r) => r.index_of(id).is_some(),
            VirtualControlKind::Mode(m) => m.index_of(id).is_some(),
            VirtualControlKind::Toggle(_) => id == TOGGLE_ON,
            VirtualControlKind::Momentary(_) | VirtualControlKind::Counter(_) => false,
        };
        for slot in [&mut self.active, &mut self.last] {
            if slot.as_deref().is_some_and(|id| !exists(id)) {
                *slot = None;
            }
        }
        match &control.kind {
            VirtualControlKind::Counter(c) => {
                if let Some(v) = self.value {
                    self.value = Some(c.spec.value_at(c.spec.position_of(v)));
                }
            }
            _ => self.value = None,
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
    #[error(transparent)]
    Trigger(#[from] TriggerError),
    #[error(transparent)]
    Spec(#[from] SpecError),
    #[error(transparent)]
    Vjoy(#[from] VjoyError),
}

fn requests(
    control: &str,
    state: &str,
    phase: Phase,
    actions: &[Action],
    value: Option<&str>,
) -> impl Iterator<Item = ActionRequest> {
    actions.iter().map(move |a| ActionRequest {
        value: value.map(str::to_owned),
        ..ActionRequest::new(control, state, phase, a)
    })
}

/// Fills in missing state ids (`st-<n>`) and names; refuses duplicate ids.
fn normalize_states<'a>(
    states: impl Iterator<Item = (&'a mut String, &'a mut String)>,
) -> Result<(), ControlError> {
    let states: Vec<_> = states.collect();
    let mut next = 1 + states
        .iter()
        .filter_map(|(id, _)| id.strip_prefix("st-")?.parse::<u32>().ok())
        .max()
        .unwrap_or(0);
    let mut seen = std::collections::HashSet::new();
    for (i, (id, name)) in states.into_iter().enumerate() {
        if id.is_empty() {
            *id = format!("st-{next}");
            next += 1;
        }
        if !seen.insert(id.clone()) {
            return Err(ControlError::DuplicateState(id.clone()));
        }
        *name = name.trim().to_owned();
        if name.is_empty() {
            *name = format!("State {}", i + 1);
        }
    }
    Ok(())
}

/// Index of the state after stepping, for radios and modes. `None` (nothing active) counts
/// as before the first state going up and after the last going down.
fn step(len: usize, active: Option<usize>, up: bool, end: EndBehavior) -> Option<usize> {
    if len == 0 {
        return None;
    }
    let last = len - 1;
    Some(match (up, active) {
        (true, None) => 0,
        (false, None) => last,
        (true, Some(i)) if i >= last => match end {
            EndBehavior::Wrap => 0,
            EndBehavior::Clamp => last,
        },
        (true, Some(i)) => i + 1,
        (false, Some(0)) => match end {
            EndBehavior::Wrap => last,
            EndBehavior::Clamp => 0,
        },
        (false, Some(i)) => i.min(len) - 1,
    })
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
            (RadioCommand::Increment, _) => step(len, active, true, self.end),
            (RadioCommand::Decrement, _) => step(len, active, false, self.end),
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
        if let Some(s) = from.and_then(|i| self.states.get(i)) {
            out.extend(requests(
                control,
                &s.name,
                Phase::Deactivate,
                &s.on_deactivate,
                None,
            ));
        }
        if let Some(s) = to.and_then(|i| self.states.get(i)) {
            out.extend(requests(
                control,
                &s.name,
                Phase::Activate,
                &s.on_activate,
                None,
            ));
        }
        out
    }
}

impl Toggle {
    pub fn is_on(memory: &RadioMemory) -> bool {
        memory.active.is_some()
    }

    pub fn apply(
        &self,
        control: &str,
        memory: &mut RadioMemory,
        command: ToggleCommand,
    ) -> Option<Vec<ActionRequest>> {
        let on = Self::is_on(memory);
        let to = match command {
            ToggleCommand::Toggle => !on,
            ToggleCommand::On => true,
            ToggleCommand::Off => false,
        };
        if to == on {
            return None;
        }
        Self::set(memory, to);
        Some(if to {
            requests(control, "on", Phase::Activate, &self.on_on, None).collect()
        } else {
            requests(control, "off", Phase::Deactivate, &self.on_off, None).collect()
        })
    }

    pub fn set(memory: &mut RadioMemory, on: bool) {
        memory.active = on.then(|| TOGGLE_ON.to_owned());
    }
}

impl Momentary {
    /// Whether the control is active with `held` triggers held.
    pub fn active(&self, held: bool) -> bool {
        held != self.invert
    }

    /// Actions for going from active `from` to `to`.
    pub fn transition_actions(&self, control: &str, from: bool, to: bool) -> Vec<ActionRequest> {
        match (from, to) {
            (false, true) => {
                requests(control, "pressed", Phase::Activate, &self.on_press, None).collect()
            }
            (true, false) => requests(
                control,
                "released",
                Phase::Deactivate,
                &self.on_release,
                None,
            )
            .collect(),
            _ => Vec::new(),
        }
    }
}

impl CounterControl {
    pub fn value(&self, memory: &RadioMemory) -> f64 {
        let pos = memory.value.map_or_else(
            || self.spec.position_of(self.spec.initial),
            |v| self.spec.position_of(v),
        );
        self.spec.value_at(pos)
    }

    pub fn apply(
        &self,
        control: &str,
        memory: &mut RadioMemory,
        command: CounterCommand,
    ) -> Option<Vec<ActionRequest>> {
        let before = self.value(memory);
        let pos = self.spec.position_of(before);
        let to = match command {
            CounterCommand::Increment => self.spec.advance(pos, 1),
            CounterCommand::Decrement => self.spec.advance(pos, -1),
            CounterCommand::Reset => self.spec.position_of(self.spec.initial),
        };
        let value = self.spec.value_at(to);
        if to == pos {
            return None;
        }
        memory.value = Some(value);
        let shown = self.spec.format(value);
        Some(
            requests(
                control,
                &shown,
                Phase::Activate,
                &self.on_change,
                Some(&shown),
            )
            .collect(),
        )
    }

    /// Sets the value (clamped to the range, on a step) without actions.
    pub fn set(&self, memory: &mut RadioMemory, value: f64) {
        memory.value = Some(self.spec.value_at(self.spec.position_of(value)));
    }
}

impl Mode {
    pub fn index_of(&self, state_id: &str) -> Option<usize> {
        self.states.iter().position(|s| s.id == state_id)
    }

    /// The active state: the remembered one, or the first.
    pub fn active(&self, memory: &RadioMemory) -> Option<usize> {
        if self.states.is_empty() {
            return None;
        }
        Some(
            memory
                .active
                .as_deref()
                .and_then(|id| self.index_of(id))
                .unwrap_or(0),
        )
    }

    /// The state after a latching command (shifts are handled by the engine).
    pub fn next(&self, active: usize, command: ModeCommand) -> usize {
        let len = self.states.len();
        match command {
            ModeCommand::Next => step(len, Some(active), true, self.end).unwrap_or(0),
            ModeCommand::Previous => step(len, Some(active), false, self.end).unwrap_or(0),
            ModeCommand::Select(i) | ModeCommand::Shift(i) if i < len => i,
            _ => active,
        }
    }

    /// Switches to state `to`: updates `memory` and returns the actions, `None` if it is
    /// already active.
    pub fn go(
        &self,
        control: &str,
        memory: &mut RadioMemory,
        to: usize,
    ) -> Option<Vec<ActionRequest>> {
        let from = self.active(memory)?;
        if from == to || to >= self.states.len() {
            return None;
        }
        let (a, b) = (&self.states[from], &self.states[to]);
        let actions = requests(control, &a.name, Phase::Deactivate, &a.on_deactivate, None)
            .chain(requests(
                control,
                &b.name,
                Phase::Activate,
                &b.on_activate,
                None,
            ))
            .collect();
        self.set(memory, to);
        Some(actions)
    }

    pub fn set(&self, memory: &mut RadioMemory, to: usize) {
        if let Some(s) = self.states.get(to) {
            memory.last = memory.active.take();
            memory.active = Some(s.id.clone());
        }
    }
}

impl VirtualControl {
    pub fn radio(&self) -> Option<&Radio> {
        match &self.kind {
            VirtualControlKind::Radio(r) => Some(r),
            _ => None,
        }
    }

    pub fn radio_mut(&mut self) -> Option<&mut Radio> {
        match &mut self.kind {
            VirtualControlKind::Radio(r) => Some(r),
            _ => None,
        }
    }

    pub fn mode(&self) -> Option<&Mode> {
        match &self.kind {
            VirtualControlKind::Mode(m) => Some(m),
            _ => None,
        }
    }

    pub fn startup(&self) -> Startup {
        match &self.kind {
            VirtualControlKind::Radio(r) => r.startup,
            VirtualControlKind::Toggle(t) => t.startup,
            VirtualControlKind::Counter(c) => c.startup,
            VirtualControlKind::Mode(m) => m.startup,
            VirtualControlKind::Momentary(_) => Startup::Empty,
        }
    }

    /// Every trigger with the command it runs, in a fixed order per kind (radio: increment,
    /// decrement, per-state select, clear, toggle off).
    pub fn bindings<'a>(&'a self) -> Vec<(Command, &'a Trigger)> {
        fn push<'a>(out: &mut Vec<(Command, &'a Trigger)>, cmd: Command, list: &'a [Trigger]) {
            out.extend(list.iter().map(|t| (cmd, t)));
        }
        let mut out = Vec::new();
        let mut add = |cmd: Command, list: &'a [Trigger]| push(&mut out, cmd, list);
        match &self.kind {
            VirtualControlKind::Radio(r) => {
                add(Command::Radio(RadioCommand::Increment), &r.increment);
                add(Command::Radio(RadioCommand::Decrement), &r.decrement);
                for (i, s) in r.states.iter().enumerate() {
                    add(Command::Radio(RadioCommand::Select(i)), &s.select);
                }
                add(Command::Radio(RadioCommand::Clear), &r.clear);
                add(Command::Radio(RadioCommand::ToggleOff), &r.toggle_off);
            }
            VirtualControlKind::Toggle(t) => {
                add(Command::Toggle(ToggleCommand::Toggle), &t.toggle);
                add(Command::Toggle(ToggleCommand::On), &t.on);
                add(Command::Toggle(ToggleCommand::Off), &t.off);
            }
            VirtualControlKind::Momentary(m) => add(Command::Momentary, &m.triggers),
            VirtualControlKind::Counter(c) => {
                add(Command::Counter(CounterCommand::Increment), &c.increment);
                add(Command::Counter(CounterCommand::Decrement), &c.decrement);
                add(Command::Counter(CounterCommand::Reset), &c.reset);
            }
            VirtualControlKind::Mode(m) => {
                add(Command::Mode(ModeCommand::Next), &m.next);
                add(Command::Mode(ModeCommand::Previous), &m.previous);
                for (i, s) in m.states.iter().enumerate() {
                    add(Command::Mode(ModeCommand::Select(i)), &s.select);
                    add(Command::Mode(ModeCommand::Shift(i)), &s.shift);
                }
            }
        }
        out
    }

    /// Every trigger list, for editing (e.g. removing a deleted encoder's triggers).
    pub fn triggers_mut(&mut self) -> Vec<&mut Vec<Trigger>> {
        match &mut self.kind {
            VirtualControlKind::Radio(r) => {
                let mut v = vec![
                    &mut r.increment,
                    &mut r.decrement,
                    &mut r.clear,
                    &mut r.toggle_off,
                ];
                v.extend(r.states.iter_mut().map(|s| &mut s.select));
                v
            }
            VirtualControlKind::Toggle(t) => vec![&mut t.toggle, &mut t.on, &mut t.off],
            VirtualControlKind::Momentary(m) => vec![&mut m.triggers],
            VirtualControlKind::Counter(c) => {
                vec![&mut c.increment, &mut c.decrement, &mut c.reset]
            }
            VirtualControlKind::Mode(m) => {
                let mut v = vec![&mut m.next, &mut m.previous];
                for s in &mut m.states {
                    v.push(&mut s.select);
                    v.push(&mut s.shift);
                }
                v
            }
        }
    }

    pub fn triggers(&self) -> impl Iterator<Item = &Trigger> {
        self.bindings().into_iter().map(|(_, t)| t)
    }

    /// Every action list.
    pub fn actions(&self) -> Vec<&Action> {
        let lists: Vec<&Vec<Action>> = match &self.kind {
            VirtualControlKind::Radio(r) => r
                .states
                .iter()
                .flat_map(|s| [&s.on_activate, &s.on_deactivate])
                .collect(),
            VirtualControlKind::Toggle(t) => vec![&t.on_on, &t.on_off],
            VirtualControlKind::Momentary(m) => vec![&m.on_press, &m.on_release],
            VirtualControlKind::Counter(c) => vec![&c.on_change],
            VirtualControlKind::Mode(m) => m
                .states
                .iter()
                .flat_map(|s| [&s.on_activate, &s.on_deactivate])
                .collect(),
        };
        lists.into_iter().flatten().collect()
    }

    /// Every vJoy button output of the control (per state or for the control).
    pub fn button_outputs(&self) -> Vec<&ButtonOutput> {
        match &self.kind {
            VirtualControlKind::Radio(r) => {
                r.states.iter().filter_map(|s| s.output.as_ref()).collect()
            }
            VirtualControlKind::Mode(m) => {
                m.states.iter().filter_map(|s| s.output.as_ref()).collect()
            }
            VirtualControlKind::Toggle(t) => t.output.iter().collect(),
            VirtualControlKind::Momentary(m) => m.output.iter().collect(),
            VirtualControlKind::Counter(_) => Vec::new(),
        }
    }

    /// vJoy devices the control writes to (outputs and vJoy actions).
    pub fn vjoy_devices(&self) -> Vec<u8> {
        let mut out: Vec<u8> = self
            .button_outputs()
            .into_iter()
            .map(|o| o.button.device)
            .collect();
        if let VirtualControlKind::Counter(CounterControl {
            output: Some(axis), ..
        }) = &self.kind
        {
            out.push(axis.device);
        }
        for action in self.actions() {
            if let Action::Vjoy { command } = action {
                out.push(command.device());
            }
        }
        out
    }

    /// Every action list, to change.
    pub fn actions_mut(&mut self) -> Vec<&mut Action> {
        let lists: Vec<&mut Vec<Action>> = match &mut self.kind {
            VirtualControlKind::Radio(r) => r
                .states
                .iter_mut()
                .flat_map(|s| [&mut s.on_activate, &mut s.on_deactivate])
                .collect(),
            VirtualControlKind::Toggle(t) => vec![&mut t.on_on, &mut t.on_off],
            VirtualControlKind::Momentary(m) => vec![&mut m.on_press, &mut m.on_release],
            VirtualControlKind::Counter(c) => vec![&mut c.on_change],
            VirtualControlKind::Mode(m) => m
                .states
                .iter_mut()
                .flat_map(|s| [&mut s.on_activate, &mut s.on_deactivate])
                .collect(),
        };
        lists.into_iter().flatten().collect()
    }

    /// Validates the control: states (filling in missing ids and names), counter range,
    /// actions and triggers (held commands need inputs with a release). Duplicate triggers
    /// in one list are dropped.
    pub fn normalize(&mut self) -> Result<(), ControlError> {
        match &mut self.kind {
            VirtualControlKind::Radio(r) => {
                if r.states.is_empty() {
                    return Err(ControlError::NoStates);
                }
                normalize_states(r.states.iter_mut().map(|s| (&mut s.id, &mut s.name)))?;
            }
            VirtualControlKind::Mode(m) => {
                if m.states.is_empty() {
                    return Err(ControlError::NoStates);
                }
                normalize_states(m.states.iter_mut().map(|s| (&mut s.id, &mut s.name)))?;
            }
            VirtualControlKind::Counter(c) => c.spec.validate()?,
            VirtualControlKind::Toggle(_) | VirtualControlKind::Momentary(_) => {}
        }
        for action in self.actions_mut() {
            action.normalize()?;
        }
        for output in self.button_outputs() {
            output.validate()?;
        }
        if let VirtualControlKind::Counter(CounterControl {
            output: Some(axis), ..
        }) = &self.kind
        {
            crate::vjoy::validate_axis(axis)?;
        }
        let holds: Vec<bool> = self.bindings_hold_flags();
        for (slot, hold) in self.triggers_mut().into_iter().zip(holds) {
            for t in slot.iter_mut() {
                t.validate(hold)?;
            }
            let mut unique: Vec<Trigger> = Vec::with_capacity(slot.len());
            for t in slot.drain(..) {
                if !unique.contains(&t) {
                    unique.push(t);
                }
            }
            *slot = unique;
        }
        Ok(())
    }

    /// For each list of [`Self::triggers_mut`], whether its command is held.
    fn bindings_hold_flags(&self) -> Vec<bool> {
        match &self.kind {
            VirtualControlKind::Radio(r) => vec![false; 4 + r.states.len()],
            VirtualControlKind::Toggle(_) => vec![false; 3],
            VirtualControlKind::Momentary(_) => vec![true],
            VirtualControlKind::Counter(_) => vec![false; 3],
            VirtualControlKind::Mode(m) => {
                let mut v = vec![false, false];
                for _ in &m.states {
                    v.extend([false, true]);
                }
                v
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action::KeyStep;
    use crate::device::DeviceId;
    use crate::state::HatState;

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
            output: None,
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

    fn button(index: u16) -> Trigger {
        TriggerInput::Button {
            device: DeviceId("d".into()),
            index,
        }
        .into()
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

    fn commands(c: &VirtualControl, t: &Trigger) -> Vec<Command> {
        c.bindings()
            .into_iter()
            .filter(|(_, b)| *b == t)
            .map(|(c, _)| c)
            .collect()
    }

    fn control(kind: VirtualControlKind) -> VirtualControl {
        VirtualControl {
            show_toast: false,
            id: "vc-1".into(),
            name: "C".into(),
            kind,
        }
    }

    #[test]
    fn bindings_follow_fixed_order() {
        let t = button(1);
        let mut r = radio(EndBehavior::Wrap);
        r.clear.push(t.clone());
        r.states[2].select.push(t.clone());
        r.increment.push(t.clone());
        r.toggle_off.push(t.clone());
        let c = control(VirtualControlKind::Radio(r));
        assert_eq!(
            commands(&c, &t),
            [
                Command::Radio(Increment),
                Command::Radio(Select(2)),
                Command::Radio(Clear),
                Command::Radio(ToggleOff)
            ]
        );
        assert!(commands(&c, &button(2)).is_empty());
    }

    #[test]
    fn normalize_fills_ids_names_and_dedupes() {
        let t: Trigger = TriggerInput::EncoderStep {
            encoder: "enc-1".into(),
            direction: StepDirection::Increment,
        }
        .into();
        let mut c = control(VirtualControlKind::Radio(Radio {
            states: vec![
                RadioState {
                    id: "st-4".into(),
                    ..Default::default()
                },
                RadioState::default(),
            ],
            increment: vec![t.clone(), t.clone()],
            ..Default::default()
        }));
        c.normalize().unwrap();
        let r = c.radio().unwrap();
        assert_eq!(r.states[1].id, "st-5");
        assert_eq!(r.states[1].name, "State 2");
        assert_eq!(r.increment, [t]);

        let VirtualControlKind::Radio(r) = &mut c.kind else {
            unreachable!()
        };
        r.states[1].id = "st-4".into();
        assert_eq!(
            c.normalize(),
            Err(ControlError::DuplicateState("st-4".into()))
        );
        assert_eq!(
            control(VirtualControlKind::Radio(Radio::default())).normalize(),
            Err(ControlError::NoStates)
        );
        assert_eq!(
            control(VirtualControlKind::Mode(Mode::default())).normalize(),
            Err(ControlError::NoStates)
        );
    }

    #[test]
    fn held_commands_need_a_release() {
        let enc: Trigger = TriggerInput::EncoderStep {
            encoder: "enc-1".into(),
            direction: StepDirection::Increment,
        }
        .into();
        let mut m = control(VirtualControlKind::Momentary(Momentary {
            triggers: vec![enc.clone()],
            ..Default::default()
        }));
        assert_eq!(
            m.normalize(),
            Err(ControlError::Trigger(TriggerError::NeedsRelease))
        );
        // Encoders are fine for latching mode commands, not for shifts.
        let mut mode = control(VirtualControlKind::Mode(Mode {
            states: vec![ModeState {
                select: vec![enc.clone()],
                ..Default::default()
            }],
            next: vec![enc.clone()],
            ..Default::default()
        }));
        mode.normalize().unwrap();
        let VirtualControlKind::Mode(md) = &mut mode.kind else {
            unreachable!()
        };
        md.states[0].shift.push(enc);
        assert_eq!(
            mode.normalize(),
            Err(ControlError::Trigger(TriggerError::NeedsRelease))
        );
        let mut counter = control(VirtualControlKind::Counter(CounterControl {
            spec: CounterSpec {
                step: 0.0,
                ..Default::default()
            },
            ..Default::default()
        }));
        assert_eq!(
            counter.normalize(),
            Err(ControlError::Spec(SpecError::Step))
        );
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
            show_toast: false,
            id: "vc-1".into(),
            name: "Comms".into(),
            kind: VirtualControlKind::Radio(Radio {
                states: vec![RadioState {
                    id: "st-1".into(),
                    name: "Guard".into(),
                    select: vec![
                        TriggerInput::Hat {
                            device: DeviceId("f00d:0001#1".into()),
                            index: 0,
                            direction: HatDirection::UpLeft,
                        }
                        .into(),
                    ],
                    on_activate: vec![],
                    on_deactivate: vec![],
                    output: None,
                }],
                increment: vec![
                    TriggerInput::EncoderStep {
                        encoder: "enc-1".into(),
                        direction: StepDirection::Increment,
                    }
                    .into(),
                ],
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
        assert_eq!(minimal.radio().unwrap().startup, Startup::RestoreLast);
        assert!(!minimal.radio().unwrap().toggle_on_reselect);

        // The new kinds.
        for (kind, json) in [
            (
                "toggle",
                r#"{"type":"toggle","toggle":[],"onOn":[],"onOff":[],"startup":"empty"}"#,
            ),
            (
                "momentary",
                r#"{"type":"momentary","triggers":[],"invert":true}"#,
            ),
            (
                "counter",
                r#"{"type":"counter","spec":{"min":1,"max":5,"step":1,"initial":1,"end":"clamp","decimals":0}}"#,
            ),
            (
                "mode",
                r#"{"type":"mode","states":[{"id":"st-1","name":"Air","shift":[]}],"next":[]}"#,
            ),
        ] {
            let c: VirtualControlKind = serde_json::from_str(json).unwrap();
            assert_eq!(serde_json::to_value(&c).unwrap()["type"], kind);
        }
    }

    fn memory(active: Option<&str>, last: Option<&str>) -> RadioMemory {
        RadioMemory {
            active: active.map(Into::into),
            last: last.map(Into::into),
            value: None,
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
        let r = control(VirtualControlKind::Radio(radio(EndBehavior::Wrap)));
        let mut m = memory(Some("x"), Some("b"));
        m.retain_existing(&r);
        assert_eq!(m, memory(None, Some("b")));
        let mut m = memory(Some("a"), Some("y"));
        m.retain_existing(&r);
        assert_eq!(m, memory(Some("a"), None));
        // A counter's value is clamped into a changed range.
        let c = control(VirtualControlKind::Counter(CounterControl {
            spec: CounterSpec {
                min: 1.0,
                max: 5.0,
                ..Default::default()
            },
            ..Default::default()
        }));
        let mut m = RadioMemory {
            value: Some(9.0),
            ..Default::default()
        };
        m.retain_existing(&c);
        assert_eq!(m.value, Some(5.0));
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
        let v: RadioMemory = serde_json::from_str(r#"{"value":7.5}"#).unwrap();
        assert_eq!(v.value, Some(7.5));
    }

    #[test]
    fn toggle_commands() {
        let t = Toggle {
            on_on: vec![key("on")],
            on_off: vec![key("off")],
            ..Default::default()
        };
        let mut m = RadioMemory::default();
        assert_eq!(trace(t.apply("t", &mut m, ToggleCommand::Toggle)), ["+on"]);
        assert!(Toggle::is_on(&m));
        assert!(t.apply("t", &mut m, ToggleCommand::On).is_none());
        assert_eq!(trace(t.apply("t", &mut m, ToggleCommand::Off)), ["-off"]);
        assert!(t.apply("t", &mut m, ToggleCommand::Off).is_none());
        assert_eq!(trace(t.apply("t", &mut m, ToggleCommand::On)), ["+on"]);
    }

    #[test]
    fn momentary_transitions_and_invert() {
        let mut m = Momentary {
            on_press: vec![key("p")],
            on_release: vec![key("r")],
            ..Default::default()
        };
        assert!(m.active(true) && !m.active(false));
        assert_eq!(
            trace(Some(m.transition_actions("m", false, true))),
            ["+pressed"]
        );
        assert_eq!(
            trace(Some(m.transition_actions("m", true, false))),
            ["-released"]
        );
        assert!(m.transition_actions("m", true, true).is_empty());
        m.invert = true;
        assert!(!m.active(true) && m.active(false));
    }

    #[test]
    fn counter_steps_wraps_resets_and_carries_its_value() {
        let c = CounterControl {
            spec: CounterSpec {
                min: 118.0,
                max: 118.1,
                step: 0.025,
                initial: 118.05,
                end: EndBehavior::Wrap,
                decimals: 3,
            },
            on_change: vec![key("x")],
            ..Default::default()
        };
        let mut m = RadioMemory::default();
        assert_eq!(c.value(&m), 118.05);
        let a = c.apply("Freq", &mut m, CounterCommand::Increment).unwrap();
        assert_eq!(a[0].value.as_deref(), Some("118.075"));
        assert_eq!(a[0].fill_placeholders("set {value} MHz"), "set 118.075 MHz");
        c.apply("Freq", &mut m, CounterCommand::Increment);
        c.apply("Freq", &mut m, CounterCommand::Increment);
        assert_eq!(c.spec.format(c.value(&m)), "118.000");
        c.apply("Freq", &mut m, CounterCommand::Decrement);
        assert_eq!(c.spec.format(c.value(&m)), "118.100");
        c.apply("Freq", &mut m, CounterCommand::Reset);
        assert_eq!(c.value(&m), 118.05);
        assert!(c.apply("Freq", &mut m, CounterCommand::Reset).is_none());
        c.set(&mut m, 500.0);
        assert_eq!(c.spec.format(c.value(&m)), "118.100");
    }

    #[test]
    fn mode_has_exactly_one_state_active() {
        let s = |id: &str| ModeState {
            id: id.into(),
            name: id.to_uppercase(),
            on_activate: vec![key(id)],
            on_deactivate: vec![key(id)],
            ..Default::default()
        };
        let mode = Mode {
            states: vec![s("air"), s("ground"), s("nav")],
            end: EndBehavior::Clamp,
            ..Default::default()
        };
        let mut m = RadioMemory::default();
        assert_eq!(mode.active(&m), Some(0));
        assert_eq!(mode.next(0, ModeCommand::Previous), 0);
        assert_eq!(mode.next(0, ModeCommand::Next), 1);
        assert_eq!(mode.next(2, ModeCommand::Next), 2);
        assert_eq!(mode.next(0, ModeCommand::Select(9)), 0);
        assert_eq!(trace(mode.go("M", &mut m, 2)), ["-AIR", "+NAV"]);
        assert_eq!(mode.active(&m), Some(2));
        assert!(mode.go("M", &mut m, 2).is_none());
        // A remembered state that is gone: the first one.
        let gone = memory(Some("x"), None);
        assert_eq!(mode.active(&gone), Some(0));
    }
}

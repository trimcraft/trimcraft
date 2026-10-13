//! Ties everything together: live input, encoder detection, stateful functions and virtual
//! controls.
//!
//! The engine is driven by backend events one at a time, so every encoder pulse is counted
//! when it happens, independent of how often the UI or overlay redraws.

use crate::action::{Action, Movement};
use crate::connection::{ConnectionError, Connections, ConnectorId, ConnectorState, ConnectorView};
use crate::device::DeviceId;
use crate::device::DeviceKind;
use crate::encoder::{
    DetectorConfig, EncoderBinding, EncoderDetector, EncoderSuggestion, PulseStats,
};
use crate::event::BackendEvent;
use crate::function::{CounterInput, Function, FunctionKind, SpecError};
use crate::overlay::{ControlRef, MonitorRef, OverlayConfig, OverlayItem, WidgetPosition};
use crate::profile::{AutoSwitch, Profile, ProfileSwitching, ProfileTarget, normalize_executable};
use crate::recording::{
    RecordKind, RecorderConfig, RecorderError, RecorderPhase, RecorderView, RecordingResult,
};
use crate::registry::DeviceRegistry;
use crate::setup::{ButtonPair, Setup, next_id};
use crate::state::{ButtonEdge, HatEdge};
use crate::state::{LiveState, Snapshot};
use crate::virtual_control::{
    ActionRequest, ControlError, HatDirection, RadioCommand, RadioMemory, Startup, StepDirection,
    Trigger, VirtualControl,
};
use serde::Serialize;
use std::collections::{BTreeMap, HashMap, HashSet};
use thiserror::Error;

/// Which persisted parts changed since the last [`Engine::take_dirty`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Dirty {
    pub registry: bool,
    pub setup: bool,
    pub values: bool,
    /// Active states of virtual controls.
    pub controls: bool,
}

#[derive(Debug, Error, PartialEq)]
pub enum EngineError {
    #[error("unknown device {0}")]
    UnknownDevice(String),
    #[error("unknown encoder {0}")]
    UnknownEncoder(String),
    #[error("unknown function {0}")]
    UnknownFunction(String),
    #[error("both directions need different buttons")]
    SameButton,
    #[error("button {0} already belongs to another encoder")]
    ButtonInUse(u16),
    #[error("unknown virtual control {0}")]
    UnknownControl(String),
    #[error("unknown state {0}")]
    UnknownState(String),
    #[error("overlay widget {0} does not exist")]
    UnknownWidget(usize),
    #[error("expected {expected} widget positions, got {got}")]
    WidgetCount { expected: usize, got: usize },
    #[error("a widget cannot change its type or what it shows")]
    WidgetKind,
    #[error(transparent)]
    Spec(#[from] SpecError),
    #[error(transparent)]
    Control(#[from] ControlError),
    #[error(transparent)]
    Recorder(#[from] RecorderError),
    #[error(transparent)]
    Connection(#[from] ConnectionError),
    #[error("a recording is already running")]
    Recording,
    #[error("unknown profile {0}")]
    UnknownProfile(String),
    #[error("the last profile cannot be deleted")]
    LastProfile,
    #[error("\"{0}\" is not a program name; enter e.g. DCS.exe")]
    BadExecutable(String),
}

/// A profile without its controls and widgets (the snapshot carries the active profile's).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileView {
    pub id: String,
    pub name: String,
    pub executables: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ButtonStats {
    pub device: DeviceId,
    pub button: u16,
    pub stats: PulseStats,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EncoderView {
    #[serde(flatten)]
    pub binding: EncoderBinding,
    pub decrement_stats: Option<PulseStats>,
    pub increment_stats: Option<PulseStats>,
    /// Detents counted since start, per direction.
    pub steps_down: u64,
    pub steps_up: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FunctionView {
    #[serde(flatten)]
    pub function: Function,
    pub value: f64,
    /// Value formatted for display, e.g. "3" or "121.500".
    pub display: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VirtualControlView {
    #[serde(flatten)]
    pub control: VirtualControl,
    /// Id of the active state, if any.
    pub active: Option<String>,
    /// Name of the active state, for display.
    pub active_name: Option<String>,
}

/// "Learn" mode: the next input edge is captured instead of firing triggers.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LearnView {
    /// `None` while still waiting for an input.
    pub captured: Option<Trigger>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineSnapshot {
    #[serde(flatten)]
    pub live: Snapshot,
    pub encoders: Vec<EncoderView>,
    pub suggestions: Vec<EncoderSuggestion>,
    /// Buttons that pulse like an encoder but whose partner was not seen yet.
    pub half_detected: Vec<ButtonPair>,
    pub button_stats: Vec<ButtonStats>,
    pub functions: Vec<FunctionView>,
    /// All profiles in order; the first is the default profile.
    pub profiles: Vec<ProfileView>,
    pub active_profile: String,
    pub profile_switching: ProfileSwitching,
    /// The program in the foreground (not Detent), as auto-switching sees it.
    pub foreground: Option<String>,
    /// The active profile's virtual controls.
    pub virtual_controls: Vec<VirtualControlView>,
    /// `None` unless learn mode is on or just captured something.
    pub learn: Option<LearnView>,
    /// The overlay is interactive so widgets can be dragged.
    pub overlay_editing: bool,
    /// The macro recorder: countdown, recording, or the last result until the UI takes it.
    pub recorder: RecorderView,
    pub recorder_config: RecorderConfig,
    pub connections: Connections,
    /// Every connector with its live status.
    pub connectors: Vec<ConnectorView>,
    /// Global overlay settings with the active profile's widgets.
    pub overlay: OverlayConfig,
    pub detector: DetectorConfig,
}

#[derive(Debug, Clone, PartialEq)]
enum Learn {
    Off,
    Waiting,
    Captured(Trigger),
}

#[derive(Debug)]
pub struct Engine {
    registry: DeviceRegistry,
    setup: Setup,
    live: LiveState,
    detector: EncoderDetector,
    positions: HashMap<String, i64>,
    steps: HashMap<(String, bool), u64>,
    /// Active and most recently active state per virtual control, of all profiles (control
    /// ids are unique across profiles).
    memory: HashMap<String, RadioMemory>,
    auto_switch: AutoSwitch,
    /// Actions emitted by triggers, oldest first, until [`Engine::take_actions`].
    actions: Vec<ActionRequest>,
    learn: Learn,
    overlay_editing: bool,
    recorder: RecorderView,
    connector_states: HashMap<ConnectorId, ConnectorState>,
    dirty: Dirty,
}

impl Engine {
    /// `values` are the function values saved at the last run. Virtual controls start without
    /// an active state; see [`Engine::restore_active_states`].
    pub fn new(registry: DeviceRegistry, setup: Setup, values: &BTreeMap<String, f64>) -> Self {
        let positions = setup
            .functions
            .iter()
            .map(|f| {
                let pos = match (&f.kind, values.get(&f.id)) {
                    (FunctionKind::Counter { spec, .. }, Some(v)) => spec.position_of(*v),
                    _ => f.initial_position(),
                };
                (f.id.clone(), pos)
            })
            .collect();
        Self {
            registry,
            detector: EncoderDetector::new(setup.detector),
            setup,
            live: LiveState::default(),
            positions,
            steps: HashMap::new(),
            memory: HashMap::new(),
            auto_switch: AutoSwitch::default(),
            actions: Vec::new(),
            learn: Learn::Off,
            overlay_editing: false,
            recorder: RecorderView::default(),
            connector_states: HashMap::new(),
            dirty: Dirty::default(),
        }
    }

    /// Restores what was saved at the last run (active and most recently active state per
    /// control), without running any actions. States that no longer exist are dropped. A
    /// control set to start empty starts with nothing active but remembers the state that
    /// was active, so toggle off brings it back.
    pub fn restore_active_states(&mut self, saved: &BTreeMap<String, RadioMemory>) {
        for control in self.setup.profiles.iter().flat_map(|p| &p.virtual_controls) {
            let radio = control.radio();
            let Some(mut memory) = saved.get(&control.id).cloned() else {
                continue;
            };
            memory.retain_existing(radio);
            if radio.startup == Startup::Empty {
                memory.last = memory.active.take().or(memory.last);
            }
            if !memory.is_empty() {
                self.memory.insert(control.id.clone(), memory);
            }
        }
    }

    pub fn registry(&self) -> &DeviceRegistry {
        &self.registry
    }

    pub fn setup(&self) -> &Setup {
        &self.setup
    }

    pub fn take_dirty(&mut self) -> Dirty {
        std::mem::take(&mut self.dirty)
    }

    /// Applies one backend event. Returns whether anything visible changed.
    ///
    /// Triggers of virtual controls are evaluated right here, edge by edge; the actions they
    /// emit are queued in order and collected with [`Engine::take_actions`].
    pub fn apply(&mut self, event: BackendEvent) -> bool {
        let applied = self.live.apply(event, &mut self.registry);
        self.dirty.registry |= applied.devices_changed;
        let mut fired = Vec::new();
        if let Some(edge) = &applied.button {
            self.detector
                .observe(&edge.device, edge.index, edge.pressed, edge.time_ns);
            if edge.pressed {
                fired = self.step_encoders(&edge.device, edge.index);
                fired.extend(self.button_trigger(edge));
            }
        }
        if let Some(edge) = &applied.hat {
            fired.extend(self.hat_trigger(edge));
        }
        if !fired.is_empty() {
            if self.learn == Learn::Waiting {
                // The first trigger wins: an encoder detent before the plain button press.
                self.learn = Learn::Captured(fired.swap_remove(0));
            } else {
                let queued = self.actions.len();
                for trigger in &fired {
                    self.fire(trigger);
                }
                self.run_profile_switches(queued);
            }
        }
        applied.state_changed
    }

    /// Buttons and hats of virtual devices (vJoy) are Detent's own output, never triggers.
    fn is_input_device(&self, device: &DeviceId) -> bool {
        self.live
            .device(device)
            .is_some_and(|d| d.kind == DeviceKind::Physical)
    }

    fn button_trigger(&self, edge: &ButtonEdge) -> Option<Trigger> {
        self.is_input_device(&edge.device).then(|| Trigger::Button {
            device: edge.device.clone(),
            index: edge.index,
        })
    }

    fn hat_trigger(&self, edge: &HatEdge) -> Option<Trigger> {
        let direction = HatDirection::from_state(edge.state)?;
        if HatDirection::from_state(edge.previous) == Some(direction)
            || !self.is_input_device(&edge.device)
        {
            return None;
        }
        Some(Trigger::Hat {
            device: edge.device.clone(),
            index: edge.index,
            direction,
        })
    }

    /// Runs `trigger` through every virtual control of the active profile, in setup order.
    /// Profile switches it causes take effect afterwards (see [`Self::run_profile_switches`]),
    /// so the triggers of one input edge all reach the same profile.
    fn fire(&mut self, trigger: &Trigger) {
        let active = self.setup.active_index();
        for control in &self.setup.profiles[active].virtual_controls {
            for command in control.radio().commands_for(trigger) {
                self.dirty.controls |=
                    run_radio_command(control, command, &mut self.memory, &mut self.actions);
            }
        }
    }

    /// Carries out the "Switch profile" actions queued since index `from`, in order. They
    /// stay in the queue, so the app logs them like every action.
    fn run_profile_switches(&mut self, from: usize) {
        let targets: Vec<ProfileTarget> = self.actions[from..]
            .iter()
            .filter_map(|r| match &r.action {
                Action::SwitchProfile { target } => Some(target.clone()),
                _ => None,
            })
            .collect();
        for target in targets {
            if let Some(index) = target.resolve(&self.setup.profiles, self.setup.active_index()) {
                self.auto_switch.manual_switch();
                self.activate_profile(index);
            }
        }
    }

    /// Actions emitted since the last call, in the order they must run.
    pub fn take_actions(&mut self) -> Vec<ActionRequest> {
        std::mem::take(&mut self.actions)
    }

    /// Returns a trigger per encoder detent caused by this press.
    fn step_encoders(&mut self, device: &DeviceId, button: u16) -> Vec<Trigger> {
        let mut fired = Vec::new();
        for encoder in &self.setup.encoders {
            let direction = encoder.direction(device, button);
            if direction == 0 {
                continue;
            }
            fired.push(Trigger::EncoderStep {
                encoder: encoder.id.clone(),
                direction: if direction > 0 {
                    StepDirection::Increment
                } else {
                    StepDirection::Decrement
                },
            });
            *self
                .steps
                .entry((encoder.id.clone(), direction > 0))
                .or_default() += 1;
            for function in &self.setup.functions {
                if function.encoder() != Some(encoder.id.as_str()) {
                    continue;
                }
                let FunctionKind::Counter { spec, .. } = &function.kind;
                let pos = self.positions.entry(function.id.clone()).or_insert(0);
                *pos = spec.advance(*pos, direction);
                self.dirty.values = true;
            }
        }
        fired
    }

    /// Current function values, for persisting.
    pub fn values(&self) -> BTreeMap<String, f64> {
        self.setup
            .functions
            .iter()
            .map(|f| (f.id.clone(), self.value_of(f)))
            .collect()
    }

    /// Active and most recently active state per virtual control, for persisting.
    pub fn active_states(&self) -> BTreeMap<String, RadioMemory> {
        self.memory
            .iter()
            .filter(|(_, m)| !m.is_empty())
            .map(|(c, m)| (c.clone(), m.clone()))
            .collect()
    }

    fn control_view(&self, control: &VirtualControl) -> VirtualControlView {
        let radio = control.radio();
        let active = self
            .memory
            .get(&control.id)
            .and_then(|m| m.active.as_deref())
            .and_then(|id| radio.index_of(id));
        VirtualControlView {
            control: control.clone(),
            active: active.map(|i| radio.states[i].id.clone()),
            active_name: active.map(|i| radio.states[i].name.clone()),
        }
    }

    fn value_of(&self, f: &Function) -> f64 {
        let FunctionKind::Counter { spec, .. } = &f.kind;
        let pos = self
            .positions
            .get(&f.id)
            .copied()
            .unwrap_or_else(|| f.initial_position());
        spec.value_at(pos.clamp(0, spec.last_position()))
    }

    pub fn snapshot(&self) -> EngineSnapshot {
        let bound = &self.setup.encoders;
        let dismissed: HashSet<(DeviceId, u16, u16)> = self
            .setup
            .dismissed_suggestions
            .iter()
            .map(|p| (p.device.clone(), p.decrement, p.increment))
            .collect();
        EngineSnapshot {
            live: self.live.snapshot(),
            encoders: bound
                .iter()
                .map(|e| EncoderView {
                    binding: e.clone(),
                    decrement_stats: self.detector.stats(&e.device, e.decrement).cloned(),
                    increment_stats: self.detector.stats(&e.device, e.increment).cloned(),
                    steps_down: self.steps.get(&(e.id.clone(), false)).copied().unwrap_or(0),
                    steps_up: self.steps.get(&(e.id.clone(), true)).copied().unwrap_or(0),
                })
                .collect(),
            suggestions: self.detector.suggestions(bound, &dismissed),
            half_detected: self
                .detector
                .half_detected(bound)
                .into_iter()
                .map(|(device, b)| ButtonPair {
                    device,
                    decrement: b,
                    increment: b,
                })
                .collect(),
            button_stats: self
                .detector
                .all_stats()
                .map(|(d, b, s)| ButtonStats {
                    device: d.clone(),
                    button: b,
                    stats: s.clone(),
                })
                .collect(),
            functions: self
                .setup
                .functions
                .iter()
                .map(|f| {
                    let value = self.value_of(f);
                    let FunctionKind::Counter { spec, .. } = &f.kind;
                    FunctionView {
                        function: f.clone(),
                        value,
                        display: spec.format(value),
                    }
                })
                .collect(),
            profiles: self
                .setup
                .profiles
                .iter()
                .map(|p| ProfileView {
                    id: p.id.clone(),
                    name: p.name.clone(),
                    executables: p.executables.clone(),
                })
                .collect(),
            active_profile: self.setup.active_profile.clone(),
            profile_switching: self.setup.profile_switching,
            foreground: self.auto_switch.foreground().map(str::to_owned),
            virtual_controls: self
                .setup
                .profile()
                .virtual_controls
                .iter()
                .map(|c| self.control_view(c))
                .collect(),
            learn: match &self.learn {
                Learn::Off => None,
                Learn::Waiting => Some(LearnView { captured: None }),
                Learn::Captured(t) => Some(LearnView {
                    captured: Some(t.clone()),
                }),
            },
            overlay_editing: self.overlay_editing,
            recorder: self.recorder.clone(),
            recorder_config: self.setup.recorder.clone(),
            connections: self.setup.connections.clone(),
            connectors: ConnectorId::ALL
                .iter()
                .map(|&id| {
                    ConnectorView::new(
                        id,
                        self.setup.connections.enabled(id),
                        self.connector_states.get(&id),
                    )
                })
                .collect(),
            overlay: self.overlay(),
            detector: self.setup.detector,
        }
    }

    // --- Commands from the UI -------------------------------------------------------------

    pub fn set_device_label(
        &mut self,
        id: &DeviceId,
        label: Option<String>,
    ) -> Result<(), EngineError> {
        if !self.registry.set_label(id, label) {
            return Err(EngineError::UnknownDevice(id.0.clone()));
        }
        self.live.sync_labels(&self.registry);
        self.dirty.registry = true;
        Ok(())
    }

    /// Marks two buttons as one encoder (from a suggestion or by hand). Returns its id.
    pub fn add_encoder(
        &mut self,
        name: String,
        device: DeviceId,
        decrement: u16,
        increment: u16,
    ) -> Result<String, EngineError> {
        if decrement == increment {
            return Err(EngineError::SameButton);
        }
        if self.live.device(&device).is_none() && self.registry.get(&device).is_none() {
            return Err(EngineError::UnknownDevice(device.0));
        }
        for b in [decrement, increment] {
            if self.setup.encoders.iter().any(|e| e.uses(&device, b)) {
                return Err(EngineError::ButtonInUse(b));
            }
        }
        let id = next_id("enc", self.setup.encoders.iter().map(|e| e.id.as_str()));
        let name = if name.trim().is_empty() {
            format!("Encoder {}", self.setup.encoders.len() + 1)
        } else {
            name.trim().to_owned()
        };
        self.setup.encoders.push(EncoderBinding {
            id: id.clone(),
            name,
            device,
            decrement,
            increment,
        });
        self.dirty.setup = true;
        Ok(id)
    }

    pub fn rename_encoder(&mut self, id: &str, name: String) -> Result<(), EngineError> {
        let e = self.encoder_mut(id)?;
        if !name.trim().is_empty() {
            e.name = name.trim().to_owned();
        }
        self.dirty.setup = true;
        Ok(())
    }

    /// Swaps which button counts up.
    pub fn swap_encoder(&mut self, id: &str) -> Result<(), EngineError> {
        let e = self.encoder_mut(id)?;
        std::mem::swap(&mut e.decrement, &mut e.increment);
        self.dirty.setup = true;
        Ok(())
    }

    /// Removes an encoder; counters bound to it keep their value but lose their input.
    pub fn remove_encoder(&mut self, id: &str) -> Result<(), EngineError> {
        let before = self.setup.encoders.len();
        self.setup.encoders.retain(|e| e.id != id);
        if self.setup.encoders.len() == before {
            return Err(EngineError::UnknownEncoder(id.into()));
        }
        for f in &mut self.setup.functions {
            let FunctionKind::Counter { input, .. } = &mut f.kind;
            if matches!(input, CounterInput::Encoder { encoder } if encoder == id) {
                *input = CounterInput::None;
            }
        }
        for control in self
            .setup
            .profiles
            .iter_mut()
            .flat_map(|p| &mut p.virtual_controls)
        {
            for slot in control.radio_mut().triggers_mut() {
                slot.retain(
                    |t| !matches!(t, Trigger::EncoderStep { encoder, .. } if encoder == id),
                );
            }
        }
        self.dirty.setup = true;
        Ok(())
    }

    pub fn dismiss_suggestion(&mut self, pair: ButtonPair) {
        if !self.setup.dismissed_suggestions.contains(&pair) {
            self.setup.dismissed_suggestions.push(pair);
            self.dirty.setup = true;
        }
    }

    /// Creates the function if `function.id` is empty, updates it otherwise. Returns its id.
    pub fn save_function(&mut self, mut function: Function) -> Result<String, EngineError> {
        function.validate()?;
        if let Some(encoder) = function.encoder()
            && !self.setup.encoders.iter().any(|e| e.id == encoder)
        {
            return Err(EngineError::UnknownEncoder(encoder.into()));
        }
        if function.name.trim().is_empty() {
            function.name = "Counter".into();
        }
        if function.id.is_empty() {
            function.id = next_id("fn", self.setup.functions.iter().map(|f| f.id.as_str()));
            self.positions
                .insert(function.id.clone(), function.initial_position());
            self.setup.functions.push(function.clone());
        } else {
            let id = function.id.clone();
            let current = self.value_of(self.function(&id)?);
            let FunctionKind::Counter { spec, .. } = &function.kind;
            // Keep the current value where the new range allows it.
            self.positions.insert(id.clone(), spec.position_of(current));
            let slot = self
                .setup
                .functions
                .iter_mut()
                .find(|f| f.id == id)
                .ok_or(EngineError::UnknownFunction(id))?;
            *slot = function.clone();
        }
        self.dirty.setup = true;
        self.dirty.values = true;
        Ok(function.id)
    }

    pub fn remove_function(&mut self, id: &str) -> Result<(), EngineError> {
        self.function(id)?;
        self.setup.functions.retain(|f| f.id != id);
        for profile in &mut self.setup.profiles {
            profile.overlay_widgets.remove_function(id);
        }
        self.positions.remove(id);
        self.dirty.setup = true;
        self.dirty.values = true;
        Ok(())
    }

    /// Sets a counter to a value, e.g. to match the game after a restart.
    pub fn set_function_value(&mut self, id: &str, value: f64) -> Result<(), EngineError> {
        let FunctionKind::Counter { spec, .. } = &self.function(id)?.kind;
        let pos = spec.position_of(value);
        self.positions.insert(id.into(), pos);
        self.dirty.values = true;
        Ok(())
    }

    pub fn toggle_overlay_control(&mut self, device: &DeviceId, control: ControlRef) -> bool {
        self.dirty.setup = true;
        self.setup
            .profile_mut()
            .overlay_widgets
            .toggle_control(device, control)
    }

    pub fn toggle_overlay_function(&mut self, id: &str) -> Result<bool, EngineError> {
        self.function(id)?;
        self.dirty.setup = true;
        Ok(self.setup.profile_mut().overlay_widgets.toggle_function(id))
    }

    /// The overlay as the windows show it: global settings with the active profile's widgets.
    pub fn overlay(&self) -> OverlayConfig {
        OverlayConfig {
            widgets: self.setup.profile().overlay_widgets.clone(),
            ..self.setup.overlay.clone()
        }
    }

    /// Replaces layout settings (anchor, margin, scale, hotkey, visibility) and the active
    /// profile's widgets (their order); takes what [`Self::overlay`] returns.
    pub fn set_overlay(&mut self, mut overlay: OverlayConfig) {
        self.setup.profile_mut().overlay_widgets = std::mem::take(&mut overlay.widgets);
        self.setup.overlay = overlay;
        self.dirty.setup = true;
    }

    pub fn set_overlay_visible(&mut self, visible: bool) {
        self.setup.overlay.visible = visible;
        self.dirty.setup = true;
    }

    /// Replaces one widget's settings (position, radio layout, label). What it shows stays.
    pub fn set_overlay_widget(
        &mut self,
        index: usize,
        mut item: OverlayItem,
    ) -> Result<(), EngineError> {
        let slot = self
            .setup
            .profile_mut()
            .overlay_widgets
            .get_mut(index)
            .ok_or(EngineError::UnknownWidget(index))?;
        if !slot.widget.same_target(&item.widget) {
            return Err(EngineError::WidgetKind);
        }
        item.position = item.position.map(WidgetPosition::clamped);
        *slot = item;
        self.dirty.setup = true;
        Ok(())
    }

    /// Sets the positions of all widgets at once, in widget order.
    pub fn set_overlay_positions(
        &mut self,
        positions: Vec<Option<WidgetPosition>>,
    ) -> Result<(), EngineError> {
        let widgets = &mut self.setup.profile_mut().overlay_widgets;
        if positions.len() != widgets.len() {
            return Err(EngineError::WidgetCount {
                expected: widgets.len(),
                got: positions.len(),
            });
        }
        for (w, p) in widgets.iter_mut().zip(positions) {
            w.position = p.map(WidgetPosition::clamped);
        }
        self.dirty.setup = true;
        Ok(())
    }

    /// `None` is the primary monitor.
    pub fn set_overlay_monitor(&mut self, monitor: Option<MonitorRef>) {
        self.setup.overlay.monitor = monitor;
        self.dirty.setup = true;
    }

    /// Shows or hides the active profile's name on the overlay (for the active profile).
    pub fn toggle_overlay_profile_name(&mut self) -> bool {
        self.dirty.setup = true;
        self.setup
            .profile_mut()
            .overlay_widgets
            .toggle_profile_name()
    }

    /// Layout edit mode: the overlay window takes the mouse so widgets can be dragged.
    pub fn set_overlay_editing(&mut self, editing: bool) {
        self.overlay_editing = editing;
    }

    pub fn overlay_editing(&self) -> bool {
        self.overlay_editing
    }

    pub fn set_detector_config(&mut self, config: DetectorConfig) {
        self.setup.detector = config;
        self.detector = EncoderDetector::new(config);
        self.dirty.setup = true;
    }

    /// Creates the control (in the active profile) if `control.id` is empty, updates it
    /// otherwise, in whichever profile it belongs to. Returns its id.
    ///
    /// Editing never runs actions. The active state is kept if it still exists; a new control
    /// starts with no state active and is put on the overlay.
    pub fn save_virtual_control(
        &mut self,
        mut control: VirtualControl,
    ) -> Result<String, EngineError> {
        let radio = control.radio_mut();
        radio.normalize()?;
        for trigger in radio.triggers() {
            if let Trigger::EncoderStep { encoder, .. } = trigger
                && !self.setup.encoders.iter().any(|e| &e.id == encoder)
            {
                return Err(EngineError::UnknownEncoder(encoder.clone()));
            }
        }
        for state in &radio.states {
            for action in state.on_activate.iter().chain(&state.on_deactivate) {
                if let Action::SwitchProfile {
                    target: ProfileTarget::Profile { profile },
                } = action
                {
                    self.profile_index(profile)?;
                }
            }
        }
        control.name = control.name.trim().to_owned();
        if control.name.is_empty() {
            control.name = "Radio".into();
        }
        if control.id.is_empty() {
            control.id = next_id("vc", self.all_controls().map(|c| c.id.as_str()));
            let profile = self.setup.profile_mut();
            profile.overlay_widgets.toggle_virtual_control(&control.id);
            profile.virtual_controls.push(control.clone());
        } else {
            let (p, c) = self.control_position(&control.id)?;
            self.setup.profiles[p].virtual_controls[c] = control.clone();
            if let Some(memory) = self.memory.get_mut(&control.id) {
                let before = memory.clone();
                memory.retain_existing(control.radio());
                self.dirty.controls |= *memory != before;
            }
        }
        self.dirty.setup = true;
        Ok(control.id)
    }

    pub fn remove_virtual_control(&mut self, id: &str) -> Result<(), EngineError> {
        let (p, c) = self.control_position(id)?;
        let profile = &mut self.setup.profiles[p];
        profile.virtual_controls.remove(c);
        profile.overlay_widgets.remove_virtual_control(id);
        self.memory.remove(id);
        self.dirty.setup = true;
        self.dirty.controls = true;
        Ok(())
    }

    /// Sets the active state (or none) without running actions, e.g. to match the game.
    pub fn set_radio_active(&mut self, id: &str, state: Option<String>) -> Result<(), EngineError> {
        let (p, c) = self.control_position(id)?;
        let radio = self.setup.profiles[p].virtual_controls[c].radio();
        let to = match state {
            Some(state) => Some(
                radio
                    .index_of(&state)
                    .ok_or(EngineError::UnknownState(state))?,
            ),
            None => None,
        };
        radio.set(self.memory.entry(id.into()).or_default(), to);
        self.dirty.controls = true;
        Ok(())
    }

    /// Runs a radio command as if a trigger had fired, including its actions.
    pub fn command_radio(&mut self, id: &str, command: RadioCommand) -> Result<(), EngineError> {
        let (p, c) = self.control_position(id)?;
        let queued = self.actions.len();
        self.dirty.controls |= run_radio_command(
            &self.setup.profiles[p].virtual_controls[c],
            command,
            &mut self.memory,
            &mut self.actions,
        );
        self.run_profile_switches(queued);
        Ok(())
    }

    pub fn toggle_overlay_virtual_control(&mut self, id: &str) -> Result<bool, EngineError> {
        let (p, _) = self.control_position(id)?;
        self.dirty.setup = true;
        Ok(self.setup.profiles[p]
            .overlay_widgets
            .toggle_virtual_control(id))
    }

    /// Starts learn mode: the next button press, hat direction or encoder detent is captured
    /// (see [`EngineSnapshot::learn`]) instead of firing triggers.
    pub fn start_learn(&mut self) {
        self.learn = Learn::Waiting;
    }

    pub fn stop_learn(&mut self) {
        self.learn = Learn::Off;
    }

    // --- Connections ----------------------------------------------------------------------

    pub fn connections(&self) -> &Connections {
        &self.setup.connections
    }

    pub fn set_connections(&mut self, connections: Connections) -> Result<(), EngineError> {
        connections.validate()?;
        self.setup.connections = connections;
        self.dirty.setup = true;
        Ok(())
    }

    /// Stores what the app found out about a connector. Returns whether anything changed.
    pub fn set_connector_state(&mut self, id: ConnectorId, state: ConnectorState) -> bool {
        if self.connector_states.get(&id) == Some(&state) {
            return false;
        }
        self.connector_states.insert(id, state);
        true
    }

    pub fn connector_state(&self, id: ConnectorId) -> ConnectorState {
        self.connector_states.get(&id).cloned().unwrap_or_default()
    }

    /// Whether an action may run now: built-in ones always, others while their connector is
    /// enabled.
    pub fn action_enabled(&self, action: &Action) -> bool {
        self.setup.connections.action_enabled(action)
    }

    // --- Macro recorder -------------------------------------------------------------------
    //
    // The app runs the hooks and the countdown; the engine only holds the state the windows
    // show, and the result until the editor has taken it.

    pub fn recorder(&self) -> &RecorderView {
        &self.recorder
    }

    /// Whether a recording is counting down or running.
    pub fn recording_active(&self) -> bool {
        self.recorder.phase != RecorderPhase::Idle
    }

    /// Starts a recording session (in its countdown). Returns the session number.
    pub fn begin_recording(
        &mut self,
        kind: RecordKind,
        movement: Movement,
    ) -> Result<u64, EngineError> {
        if self.recording_active() {
            return Err(EngineError::Recording);
        }
        let session = self.recorder.session + 1;
        self.recorder = RecorderView {
            session,
            phase: RecorderPhase::Countdown,
            kind: Some(kind),
            movement,
            result: None,
            error: None,
        };
        Ok(session)
    }

    /// Moves `session` to another phase; ignored if that session is over.
    pub fn set_recording_phase(&mut self, session: u64, phase: RecorderPhase) {
        if self.recorder.session == session && self.recording_active() {
            self.recorder.phase = phase;
        }
    }

    /// Ends `session` with the recorded macro, or without one (cancelled or failed).
    pub fn finish_recording(
        &mut self,
        session: u64,
        result: Result<Option<(Action, Vec<String>)>, String>,
    ) {
        if self.recorder.session != session {
            return;
        }
        self.recorder.phase = RecorderPhase::Idle;
        match result {
            Ok(recorded) => {
                self.recorder.result = recorded.map(|(action, notes)| RecordingResult {
                    session,
                    action,
                    notes,
                });
            }
            Err(e) => self.recorder.error = Some(e),
        }
    }

    /// The editor took the result (or dismissed the error).
    pub fn clear_recording(&mut self) {
        self.recorder.result = None;
        self.recorder.error = None;
    }

    pub fn set_recorder_config(&mut self, config: RecorderConfig) -> Result<(), EngineError> {
        config.validate()?;
        self.setup.recorder = config;
        self.dirty.setup = true;
        Ok(())
    }

    // --- Profiles -------------------------------------------------------------------------
    //
    // Switching profiles never runs actions: each profile's radios keep their states and are
    // simply shown again.

    /// Makes the profile at `index` active. Returns whether it changed.
    fn activate_profile(&mut self, index: usize) -> bool {
        let id = &self.setup.profiles[index].id;
        if *id == self.setup.active_profile {
            return false;
        }
        self.setup.active_profile.clone_from(id);
        self.dirty.setup = true;
        true
    }

    /// The user picks a profile (selector in the main window). It stays until the next
    /// foreground change to a program that a profile lists.
    pub fn select_profile(&mut self, id: &str) -> Result<(), EngineError> {
        let index = self.profile_index(id)?;
        self.auto_switch.manual_switch();
        self.activate_profile(index);
        Ok(())
    }

    /// The foreground window changed to the program `exe` (`None`: Detent itself, or a
    /// program whose name cannot be read). Switches profiles as set; returns whether anything
    /// visible changed.
    pub fn foreground_changed(&mut self, exe: Option<&str>) -> bool {
        let before = self.auto_switch.foreground().map(str::to_owned);
        let switch = self.auto_switch.foreground_changed(
            exe,
            &self.setup.profiles,
            self.setup.active_index(),
            self.setup.profile_switching,
        );
        let switched = switch.is_some_and(|i| self.activate_profile(i));
        switched || before.as_deref() != self.auto_switch.foreground()
    }

    pub fn set_profile_switching(&mut self, switching: ProfileSwitching) {
        self.setup.profile_switching = switching;
        self.dirty.setup = true;
    }

    /// Creates an empty profile and makes it active, so it can be set up. Returns its id.
    pub fn create_profile(&mut self, name: &str) -> String {
        let id = next_id("profile", self.setup.profiles.iter().map(|p| p.id.as_str()));
        let name = match name.trim() {
            "" => format!("Profile {}", self.setup.profiles.len() + 1),
            name => name.to_owned(),
        };
        self.setup.profiles.push(Profile::new(id.clone(), name));
        self.auto_switch.manual_switch();
        self.activate_profile(self.setup.profiles.len() - 1);
        id
    }

    /// Copies a profile with all its controls, widgets and radio states, and makes the copy
    /// active. The copy lists no executables, so it never takes over from the original
    /// automatically. Its controls get new ids. Returns the new profile's id.
    pub fn duplicate_profile(&mut self, id: &str, name: &str) -> Result<String, EngineError> {
        let source = self.profile_index(id)?;
        let mut copy = self.setup.profiles[source].clone();
        copy.id = next_id("profile", self.setup.profiles.iter().map(|p| p.id.as_str()));
        copy.name = match name.trim() {
            "" => format!("{} copy", copy.name),
            name => name.to_owned(),
        };
        copy.executables.clear();
        let mut taken: Vec<String> = self.all_controls().map(|c| c.id.clone()).collect();
        let mut renamed = HashMap::new();
        for control in &mut copy.virtual_controls {
            let new = next_id("vc", taken.iter().map(String::as_str));
            taken.push(new.clone());
            if let Some(memory) = self.memory.get(&control.id).cloned() {
                self.memory.insert(new.clone(), memory);
                self.dirty.controls = true;
            }
            renamed.insert(std::mem::replace(&mut control.id, new.clone()), new);
        }
        copy.overlay_widgets.rename_virtual_controls(&renamed);
        let new_id = copy.id.clone();
        self.setup.profiles.push(copy);
        self.auto_switch.manual_switch();
        self.activate_profile(self.setup.profiles.len() - 1);
        Ok(new_id)
    }

    pub fn rename_profile(&mut self, id: &str, name: &str) -> Result<(), EngineError> {
        let index = self.profile_index(id)?;
        if !name.trim().is_empty() {
            self.setup.profiles[index].name = name.trim().to_owned();
            self.dirty.setup = true;
        }
        Ok(())
    }

    /// Deletes a profile with its controls; not the last one. If it was active, the default
    /// profile (the first remaining) takes over.
    pub fn delete_profile(&mut self, id: &str) -> Result<(), EngineError> {
        let index = self.profile_index(id)?;
        if self.setup.profiles.len() == 1 {
            return Err(EngineError::LastProfile);
        }
        let removed = self.setup.profiles.remove(index);
        for control in &removed.virtual_controls {
            self.memory.remove(&control.id);
        }
        if removed.id == self.setup.active_profile {
            self.setup.active_profile = self.setup.profiles[0].id.clone();
        }
        self.dirty.setup = true;
        self.dirty.controls = true;
        Ok(())
    }

    /// Makes a profile the default one by moving it to the front of the list.
    pub fn make_default_profile(&mut self, id: &str) -> Result<(), EngineError> {
        let index = self.profile_index(id)?;
        let profile = self.setup.profiles.remove(index);
        self.setup.profiles.insert(0, profile);
        self.dirty.setup = true;
        Ok(())
    }

    /// Replaces the programs that make a profile active. Names are cleaned up (a full path
    /// keeps only the file name, `.exe` is added when there is no extension) and duplicates
    /// dropped.
    pub fn set_profile_executables(
        &mut self,
        id: &str,
        executables: Vec<String>,
    ) -> Result<(), EngineError> {
        let index = self.profile_index(id)?;
        let mut clean: Vec<String> = Vec::new();
        for name in executables {
            let exe = normalize_executable(&name).ok_or(EngineError::BadExecutable(name))?;
            if !clean
                .iter()
                .any(|e| crate::profile::same_executable(e, &exe))
            {
                clean.push(exe);
            }
        }
        self.setup.profiles[index].executables = clean;
        self.dirty.setup = true;
        Ok(())
    }

    fn profile_index(&self, id: &str) -> Result<usize, EngineError> {
        self.setup
            .profiles
            .iter()
            .position(|p| p.id == id)
            .ok_or_else(|| EngineError::UnknownProfile(id.into()))
    }

    /// The controls of all profiles.
    fn all_controls(&self) -> impl Iterator<Item = &VirtualControl> {
        self.setup.profiles.iter().flat_map(|p| &p.virtual_controls)
    }

    /// Profile index and index within it of a control, in any profile.
    fn control_position(&self, id: &str) -> Result<(usize, usize), EngineError> {
        self.setup
            .profiles
            .iter()
            .enumerate()
            .find_map(|(p, profile)| {
                let c = profile.virtual_controls.iter().position(|c| c.id == id)?;
                Some((p, c))
            })
            .ok_or_else(|| EngineError::UnknownControl(id.into()))
    }

    fn function(&self, id: &str) -> Result<&Function, EngineError> {
        self.setup
            .functions
            .iter()
            .find(|f| f.id == id)
            .ok_or_else(|| EngineError::UnknownFunction(id.into()))
    }

    fn encoder_mut(&mut self, id: &str) -> Result<&mut EncoderBinding, EngineError> {
        self.setup
            .encoders
            .iter_mut()
            .find(|e| e.id == id)
            .ok_or_else(|| EngineError::UnknownEncoder(id.into()))
    }
}

/// Applies `command` to a radio: updates its memory and queues the transition's actions.
/// Returns whether the active state changed.
fn run_radio_command(
    control: &VirtualControl,
    command: RadioCommand,
    memory: &mut HashMap<String, RadioMemory>,
    actions: &mut Vec<ActionRequest>,
) -> bool {
    let memory = memory.entry(control.id.clone()).or_default();
    match control.radio().apply(&control.name, memory, command) {
        Some(emitted) => {
            actions.extend(emitted);
            true
        }
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action::{Action, KeyStep};
    use crate::device::Fingerprint;
    use crate::event::RawDevice;
    use crate::function::{CounterSpec, EndBehavior};
    use crate::state::HatState;
    use crate::virtual_control::{Radio, RadioState, VirtualControlKind};

    const MS: u64 = 1_000_000;

    fn raw() -> RawDevice {
        RawDevice {
            instance: 1,
            fingerprint: Fingerprint {
                vendor_id: 0x3344,
                product_id: 0x0001,
                serial: Some("S1".into()),
                path: None,
                name: "Panel".into(),
            },
            backend_guid: "g".into(),
            axes: vec![],
            buttons: vec![false; 32],
            hats: vec![HatState::CENTERED],
        }
    }

    fn pulse(engine: &mut Engine, button: u16, at_ms: u64) {
        engine.apply(BackendEvent::Button {
            instance: 1,
            index: button,
            pressed: true,
            time_ns: at_ms * MS,
        });
        engine.apply(BackendEvent::Button {
            instance: 1,
            index: button,
            pressed: false,
            time_ns: (at_ms + 20) * MS,
        });
    }

    fn engine_with_channel(end: EndBehavior) -> (Engine, String) {
        let mut engine = Engine::new(
            DeviceRegistry::default(),
            Setup::default(),
            &BTreeMap::new(),
        );
        engine.apply(BackendEvent::Initial(vec![raw()]));
        let dev = DeviceId("3344:0001#1".into());
        let enc = engine
            .add_encoder("Radio knob".into(), dev, 10, 11)
            .unwrap();
        let f = engine
            .save_function(Function {
                id: String::new(),
                name: "Channel".into(),
                kind: FunctionKind::Counter {
                    spec: CounterSpec {
                        min: 1.0,
                        max: 3.0,
                        step: 1.0,
                        initial: 1.0,
                        end,
                        decimals: 0,
                    },
                    input: CounterInput::Encoder { encoder: enc },
                },
            })
            .unwrap();
        (engine, f)
    }

    fn display(engine: &Engine, id: &str) -> String {
        engine
            .snapshot()
            .functions
            .into_iter()
            .find(|f| f.function.id == id)
            .unwrap()
            .display
    }

    #[test]
    fn every_pulse_steps_the_counter_and_wraps() {
        let (mut engine, f) = engine_with_channel(EndBehavior::Wrap);
        for i in 0..4 {
            pulse(&mut engine, 11, i * 50);
        }
        assert_eq!(display(&engine, &f), "2"); // 1 -> 2 -> 3 -> 1 -> 2
        pulse(&mut engine, 10, 1_000);
        pulse(&mut engine, 10, 1_050);
        assert_eq!(display(&engine, &f), "3");
        let enc = &engine.snapshot().encoders[0];
        assert_eq!((enc.steps_up, enc.steps_down), (4, 2));
        assert!(engine.take_dirty().values);
    }

    #[test]
    fn clamp_stops_at_the_end() {
        let (mut engine, f) = engine_with_channel(EndBehavior::Clamp);
        for i in 0..10 {
            pulse(&mut engine, 11, i * 50);
        }
        assert_eq!(display(&engine, &f), "3");
    }

    #[test]
    fn values_survive_a_restart() {
        let (mut engine, f) = engine_with_channel(EndBehavior::Wrap);
        pulse(&mut engine, 11, 0);
        let values = engine.values();
        let restarted = Engine::new(engine.registry().clone(), engine.setup().clone(), &values);
        assert_eq!(display(&restarted, &f), "2");
    }

    #[test]
    fn swapping_reverses_direction() {
        let (mut engine, f) = engine_with_channel(EndBehavior::Wrap);
        engine.swap_encoder("enc-1").unwrap();
        pulse(&mut engine, 11, 0);
        assert_eq!(display(&engine, &f), "3");
    }

    #[test]
    fn button_cannot_belong_to_two_encoders() {
        let (mut engine, _) = engine_with_channel(EndBehavior::Wrap);
        let dev = DeviceId("3344:0001#1".into());
        assert_eq!(
            engine.add_encoder(String::new(), dev, 11, 12),
            Err(EngineError::ButtonInUse(11))
        );
    }

    #[test]
    fn removing_encoder_unbinds_counter_but_keeps_value() {
        let (mut engine, f) = engine_with_channel(EndBehavior::Wrap);
        pulse(&mut engine, 11, 0);
        engine.remove_encoder("enc-1").unwrap();
        pulse(&mut engine, 11, 100);
        assert_eq!(display(&engine, &f), "2");
        let snap = engine.snapshot();
        assert!(matches!(
            snap.functions[0].function.kind,
            FunctionKind::Counter {
                input: CounterInput::None,
                ..
            }
        ));
    }

    #[test]
    fn removing_function_removes_its_overlay_widget() {
        let (mut engine, f) = engine_with_channel(EndBehavior::Wrap);
        assert!(engine.toggle_overlay_function(&f).unwrap());
        engine.remove_function(&f).unwrap();
        assert!(engine.snapshot().overlay.widgets.is_empty());
    }

    #[test]
    fn ids_continue_after_highest() {
        assert_eq!(next_id("fn", ["fn-1", "fn-7", "x-9"].into_iter()), "fn-8");
        assert_eq!(next_id("fn", std::iter::empty()), "fn-1");
    }

    // --- Virtual controls ----------------------------------------------------------------

    fn dev() -> DeviceId {
        DeviceId("3344:0001#1".into())
    }

    fn tap(key: &str) -> Action {
        Action::keyboard(vec![KeyStep::KeyTap {
            key: key.into(),
            hold_ms: None,
        }])
    }

    fn button(index: u16) -> Trigger {
        Trigger::Button {
            device: dev(),
            index,
        }
    }

    /// Radio with states A, B, C (A: keys F1/F2 on/off, B: F3/F4, C: F5/F6), driven by
    /// encoder `enc-1` (buttons 10/11), button 0 selects B, hat up clears.
    fn engine_with_radio(radio: impl FnOnce(&mut Radio)) -> (Engine, String) {
        let (mut engine, _) = engine_with_channel(EndBehavior::Wrap);
        let state = |name: &str, on: &str, off: &str| RadioState {
            id: String::new(),
            name: name.into(),
            select: vec![],
            on_activate: vec![tap(on)],
            on_deactivate: vec![tap(off)],
        };
        let mut r = Radio {
            states: vec![
                state("A", "F1", "F2"),
                state("B", "F3", "F4"),
                state("C", "F5", "F6"),
            ],
            increment: vec![Trigger::EncoderStep {
                encoder: "enc-1".into(),
                direction: StepDirection::Increment,
            }],
            decrement: vec![Trigger::EncoderStep {
                encoder: "enc-1".into(),
                direction: StepDirection::Decrement,
            }],
            clear: vec![Trigger::Hat {
                device: dev(),
                index: 0,
                direction: HatDirection::Up,
            }],
            ..Default::default()
        };
        r.states[1].select.push(button(0));
        radio(&mut r);
        let id = engine
            .save_virtual_control(VirtualControl {
                id: String::new(),
                name: "Comms".into(),
                kind: VirtualControlKind::Radio(r),
            })
            .unwrap();
        (engine, id)
    }

    fn active(engine: &Engine, id: &str) -> Option<String> {
        engine
            .snapshot()
            .virtual_controls
            .into_iter()
            .find(|c| c.control.id == id)
            .unwrap()
            .active_name
    }

    /// The keys tapped by the queued actions, in order.
    fn keys(engine: &mut Engine) -> Vec<String> {
        engine
            .take_actions()
            .into_iter()
            .flat_map(|a| match a.action {
                Action::KeyboardMacro { steps, .. } => steps,
                _ => vec![],
            })
            .filter_map(|s| match s {
                KeyStep::KeyTap { key, .. } => Some(key),
                _ => None,
            })
            .collect()
    }

    fn press(engine: &mut Engine, index: u16, at_ms: u64) {
        pulse(engine, index, at_ms);
    }

    fn hat(engine: &mut Engine, mask: u8, at_ms: u64) {
        engine.apply(BackendEvent::Hat {
            instance: 1,
            index: 0,
            state: HatState(mask),
            time_ns: at_ms * MS,
        });
    }

    #[test]
    fn encoder_steps_switch_states_with_ordered_actions() {
        let (mut engine, id) = engine_with_radio(|_| {});
        assert_eq!(active(&engine, &id), None);
        press(&mut engine, 11, 0);
        assert_eq!(active(&engine, &id).as_deref(), Some("A"));
        assert_eq!(keys(&mut engine), ["F1"]);
        press(&mut engine, 11, 100);
        assert_eq!(active(&engine, &id).as_deref(), Some("B"));
        assert_eq!(keys(&mut engine), ["F2", "F3"]);
        press(&mut engine, 10, 200);
        press(&mut engine, 10, 300);
        assert_eq!(active(&engine, &id).as_deref(), Some("C")); // wraps from A to C
        assert_eq!(keys(&mut engine), ["F4", "F1", "F2", "F5"]);
        assert!(engine.take_dirty().controls);
    }

    #[test]
    fn decrement_from_nothing_selects_last() {
        let (mut engine, id) = engine_with_radio(|_| {});
        press(&mut engine, 10, 0);
        assert_eq!(active(&engine, &id).as_deref(), Some("C"));
        assert_eq!(keys(&mut engine), ["F5"]);
    }

    #[test]
    fn button_selects_and_reselect_is_noop_or_toggle() {
        let (mut engine, id) = engine_with_radio(|_| {});
        press(&mut engine, 0, 0);
        assert_eq!(active(&engine, &id).as_deref(), Some("B"));
        press(&mut engine, 0, 1_000);
        assert_eq!(keys(&mut engine), ["F3"]);
        assert_eq!(active(&engine, &id).as_deref(), Some("B"));

        let (mut engine, id) = engine_with_radio(|r| r.toggle_on_reselect = true);
        press(&mut engine, 0, 0);
        press(&mut engine, 0, 1_000);
        assert_eq!(active(&engine, &id), None);
        assert_eq!(keys(&mut engine), ["F3", "F4"]);
    }

    #[test]
    fn hat_clears_when_direction_is_entered() {
        let (mut engine, id) = engine_with_radio(|_| {});
        press(&mut engine, 0, 0);
        engine.take_actions();
        hat(&mut engine, HatState::UP, 100);
        assert_eq!(active(&engine, &id), None);
        assert_eq!(keys(&mut engine), ["F4"]);
        // Clearing again does nothing, and staying in the direction does not fire again.
        hat(&mut engine, HatState::UP, 150);
        press(&mut engine, 0, 200);
        engine.take_actions();
        hat(&mut engine, HatState::UP, 300);
        assert_eq!(active(&engine, &id).as_deref(), Some("B"));
        hat(&mut engine, 0, 400);
        hat(&mut engine, HatState::UP, 500);
        assert_eq!(active(&engine, &id), None);
    }

    #[test]
    fn clamp_does_not_fire_at_the_end() {
        let (mut engine, id) = engine_with_radio(|r| r.end = EndBehavior::Clamp);
        press(&mut engine, 10, 0); // none -> C
        press(&mut engine, 11, 100); // stays C
        assert_eq!(active(&engine, &id).as_deref(), Some("C"));
        assert_eq!(keys(&mut engine), ["F5"]);
    }

    #[test]
    fn button_release_does_not_fire() {
        let (mut engine, id) = engine_with_radio(|_| {});
        engine.apply(BackendEvent::Button {
            instance: 1,
            index: 0,
            pressed: false,
            time_ns: 0,
        });
        assert_eq!(active(&engine, &id), None);
    }

    #[test]
    fn restart_restores_state_without_actions() {
        let (mut engine, id) = engine_with_radio(|_| {});
        press(&mut engine, 0, 0);
        engine.take_actions();
        let saved = engine.active_states();
        assert_eq!(saved[&id].active.as_deref(), Some("st-2"));

        let mut restarted = Engine::new(
            engine.registry().clone(),
            engine.setup().clone(),
            &BTreeMap::new(),
        );
        restarted.restore_active_states(&saved);
        assert_eq!(active(&restarted, &id).as_deref(), Some("B"));
        assert!(restarted.take_actions().is_empty());

        let mut setup = engine.setup().clone();
        setup.profile_mut().virtual_controls[0].radio_mut().startup = Startup::Empty;
        let mut empty = Engine::new(engine.registry().clone(), setup, &BTreeMap::new());
        empty.restore_active_states(&saved);
        assert_eq!(active(&empty, &id), None);

        // A state that no longer exists is not restored.
        let mut gone = Engine::new(
            engine.registry().clone(),
            engine.setup().clone(),
            &BTreeMap::new(),
        );
        let stale = RadioMemory {
            active: Some("st-99".into()),
            last: None,
        };
        gone.restore_active_states(&BTreeMap::from([(id.clone(), stale)]));
        assert!(gone.active_states().is_empty());
        assert_eq!(active(&gone, &id), None);
    }

    #[test]
    fn learn_captures_first_edge_instead_of_firing() {
        let (mut engine, id) = engine_with_radio(|_| {});
        engine.start_learn();
        assert_eq!(engine.snapshot().learn, Some(LearnView { captured: None }));
        press(&mut engine, 11, 0); // encoder button: the detent wins over the button
        press(&mut engine, 0, 100); // learning is over: this fires normally
        let learned = engine.snapshot().learn.unwrap().captured;
        assert_eq!(
            learned,
            Some(Trigger::EncoderStep {
                encoder: "enc-1".into(),
                direction: StepDirection::Increment
            })
        );
        assert_eq!(active(&engine, &id).as_deref(), Some("B"));
        assert_eq!(keys(&mut engine), ["F3"]);
        engine.stop_learn();
        assert_eq!(engine.snapshot().learn, None);

        engine.start_learn();
        hat(&mut engine, HatState::LEFT | HatState::DOWN, 200);
        assert_eq!(
            engine.snapshot().learn.unwrap().captured,
            Some(Trigger::Hat {
                device: dev(),
                index: 0,
                direction: HatDirection::DownLeft
            })
        );
    }

    #[test]
    fn virtual_devices_never_trigger() {
        let (mut engine, id) = engine_with_radio(|_| {});
        let mut vjoy = raw();
        vjoy.instance = 2;
        vjoy.fingerprint.vendor_id = 0x1234;
        vjoy.fingerprint.product_id = 0xBEAD;
        vjoy.fingerprint.serial = None;
        engine.apply(BackendEvent::Added(vjoy));
        let vjoy_id = DeviceId("1234:bead#1".into());
        let mut control = engine.setup().profile().virtual_controls[0].clone();
        control.radio_mut().states[0].select.push(Trigger::Button {
            device: vjoy_id,
            index: 3,
        });
        engine.save_virtual_control(control).unwrap();
        engine.start_learn();
        engine.apply(BackendEvent::Button {
            instance: 2,
            index: 3,
            pressed: true,
            time_ns: 0,
        });
        assert_eq!(engine.snapshot().learn.unwrap().captured, None);
        engine.stop_learn();
        engine.apply(BackendEvent::Button {
            instance: 2,
            index: 3,
            pressed: false,
            time_ns: 1,
        });
        engine.apply(BackendEvent::Button {
            instance: 2,
            index: 3,
            pressed: true,
            time_ns: 2,
        });
        assert_eq!(active(&engine, &id), None);
    }

    #[test]
    fn editing_keeps_active_state_and_never_runs_actions() {
        let (mut engine, id) = engine_with_radio(|_| {});
        press(&mut engine, 0, 0);
        engine.take_actions();
        let mut control = engine.setup().profile().virtual_controls[0].clone();
        control.radio_mut().states.swap(0, 1);
        control.radio_mut().states[0].name = "Bravo".into();
        engine.save_virtual_control(control.clone()).unwrap();
        assert_eq!(active(&engine, &id).as_deref(), Some("Bravo"));

        control.radio_mut().states.remove(0);
        engine.save_virtual_control(control).unwrap();
        assert_eq!(active(&engine, &id), None);
        assert!(engine.take_actions().is_empty());
    }

    #[test]
    fn new_control_goes_on_the_overlay_and_removal_cleans_up() {
        let (mut engine, id) = engine_with_radio(|_| {});
        assert!(engine.overlay().widgets.has_virtual_control(&id));
        assert!(!engine.toggle_overlay_virtual_control(&id).unwrap());
        assert!(engine.toggle_overlay_virtual_control(&id).unwrap());
        press(&mut engine, 0, 0);
        engine.remove_virtual_control(&id).unwrap();
        assert!(engine.snapshot().virtual_controls.is_empty());
        assert!(engine.snapshot().overlay.widgets.is_empty());
        assert!(engine.active_states().is_empty());
    }

    #[test]
    fn validation() {
        let (mut engine, _) = engine_with_channel(EndBehavior::Wrap);
        let control = |radio: Radio| VirtualControl {
            id: String::new(),
            name: "  ".into(),
            kind: VirtualControlKind::Radio(radio),
        };
        assert!(matches!(
            engine.save_virtual_control(control(Radio::default())),
            Err(EngineError::Control(ControlError::NoStates))
        ));
        let mut r = Radio {
            states: vec![RadioState::default()],
            ..Default::default()
        };
        r.increment.push(Trigger::EncoderStep {
            encoder: "enc-9".into(),
            direction: StepDirection::Increment,
        });
        assert_eq!(
            engine.save_virtual_control(control(r.clone())),
            Err(EngineError::UnknownEncoder("enc-9".into()))
        );
        r.increment.clear();
        r.states[0].on_activate.push(tap("NoSuchKey"));
        assert!(matches!(
            engine.save_virtual_control(control(r.clone())),
            Err(EngineError::Control(ControlError::Action(_)))
        ));
        r.states[0].on_activate = vec![tap("f1")];
        let id = engine.save_virtual_control(control(r)).unwrap();
        let saved = &engine.setup().profile().virtual_controls[0];
        assert_eq!(
            (saved.id.as_str(), saved.name.as_str()),
            (id.as_str(), "Radio")
        );
        assert_eq!(saved.radio().states[0].on_activate, [tap("F1")]);
        assert_eq!(saved.radio().states[0].name, "State 1");
    }

    #[test]
    fn removing_encoder_removes_its_triggers() {
        let (mut engine, id) = engine_with_radio(|_| {});
        engine.remove_encoder("enc-1").unwrap();
        let radio = engine.setup().profile().virtual_controls[0].radio().clone();
        assert!(radio.increment.is_empty() && radio.decrement.is_empty());
        assert_eq!(radio.clear.len(), 1);
        press(&mut engine, 11, 0);
        assert_eq!(active(&engine, &id), None);
    }

    #[test]
    fn manual_set_is_silent_and_command_runs_actions() {
        let (mut engine, id) = engine_with_radio(|_| {});
        engine.set_radio_active(&id, Some("st-3".into())).unwrap();
        assert_eq!(active(&engine, &id).as_deref(), Some("C"));
        assert!(engine.take_actions().is_empty());
        assert_eq!(
            engine.set_radio_active(&id, Some("nope".into())),
            Err(EngineError::UnknownState("nope".into()))
        );
        engine.command_radio(&id, RadioCommand::Select(0)).unwrap();
        assert_eq!(keys(&mut engine), ["F6", "F1"]);
        engine.set_radio_active(&id, None).unwrap();
        assert_eq!(active(&engine, &id), None);
    }

    #[test]
    fn snapshot_json_has_virtual_controls() {
        let (mut engine, _) = engine_with_radio(|_| {});
        engine.start_learn();
        let json = serde_json::to_value(engine.snapshot()).unwrap();
        let vc = &json["virtualControls"][0];
        assert_eq!(vc["id"], "vc-1");
        assert_eq!(vc["kind"]["type"], "radio");
        assert_eq!(vc["active"], serde_json::Value::Null);
        assert_eq!(vc["activeName"], serde_json::Value::Null);
        assert_eq!(json["learn"]["captured"], serde_json::Value::Null);
        assert_eq!(json["overlay"]["widgets"][0]["type"], "radio");
        assert_eq!(json["overlayEditing"], false);
        assert_eq!(json["recorder"]["phase"], "idle");
        assert_eq!(json["recorderConfig"]["stopHotkey"], "Ctrl+Shift+F9");
    }

    #[test]
    fn connectors_gate_actions_and_show_their_status() {
        use crate::connection::ConnectorStatus;
        use crate::soundpad::SoundpadCommand;
        let (mut engine, _) = engine_with_channel(EndBehavior::Wrap);
        let sound = Action::Soundpad {
            command: SoundpadCommand::Stop,
        };
        let view = |e: &Engine| e.snapshot().connectors[0].clone();
        assert_eq!(view(&engine).status, ConnectorStatus::NotConfigured);
        assert!(!engine.action_enabled(&sound));
        assert!(engine.action_enabled(&tap("A")));
        let mut c = engine.connections().clone();
        c.soundpad.enabled = true;
        engine.take_dirty();
        engine.set_connections(c.clone()).unwrap();
        assert!(engine.take_dirty().setup);
        assert!(engine.action_enabled(&sound));
        assert_eq!(view(&engine).status, ConnectorStatus::Connecting);
        let connected = ConnectorState {
            status: ConnectorStatus::Connected,
            detail: Some("26 sounds".into()),
            ..Default::default()
        };
        assert!(engine.set_connector_state(ConnectorId::Soundpad, connected.clone()));
        assert!(!engine.set_connector_state(ConnectorId::Soundpad, connected));
        assert_eq!(view(&engine).detail.as_deref(), Some("26 sounds"));
        let json = serde_json::to_value(engine.snapshot()).unwrap();
        assert_eq!(json["connections"]["soundpad"]["enabled"], true);
        assert_eq!(json["connectors"][0]["status"]["state"], "connected");
        c.soundpad.timeout_ms = 1;
        assert!(engine.set_connections(c).is_err());
        // Disabling keeps actions stored but gates them again.
        let mut off = engine.connections().clone();
        off.soundpad.enabled = false;
        engine.set_connections(off).unwrap();
        assert!(!engine.action_enabled(&sound));
        assert_eq!(view(&engine).status, ConnectorStatus::NotConfigured);
    }

    #[test]
    fn recorder_sessions_hold_their_result_until_taken() {
        let (mut engine, _) = engine_with_channel(EndBehavior::Wrap);
        let s1 = engine
            .begin_recording(RecordKind::Keyboard, Movement::Absolute)
            .unwrap();
        assert!(engine.recording_active());
        assert_eq!(
            engine.begin_recording(RecordKind::Mouse, Movement::Relative),
            Err(EngineError::Recording)
        );
        engine.set_recording_phase(s1, RecorderPhase::Recording);
        assert_eq!(engine.snapshot().recorder.phase, RecorderPhase::Recording);
        engine.finish_recording(s1, Ok(Some((tap("A"), vec!["note".into()]))));
        let view = engine.snapshot().recorder;
        assert_eq!(view.phase, RecorderPhase::Idle);
        let result = view.result.unwrap();
        assert_eq!((result.session, result.action), (s1, tap("A")));
        // A late message from an old session changes nothing.
        let s2 = engine
            .begin_recording(RecordKind::Mouse, Movement::Relative)
            .unwrap();
        assert!(engine.recorder().result.is_none());
        engine.finish_recording(s1, Err("old".into()));
        engine.set_recording_phase(s1, RecorderPhase::Idle);
        assert_eq!(engine.recorder().phase, RecorderPhase::Countdown);
        engine.finish_recording(s2, Err("hooks failed".into()));
        assert_eq!(engine.recorder().error.as_deref(), Some("hooks failed"));
        engine.clear_recording();
        assert_eq!(engine.recorder().error, None);
        // Settings are validated and saved.
        assert!(
            engine
                .set_recorder_config(RecorderConfig {
                    stop_hotkey: "Ctrl+Nope".into(),
                    ..RecorderConfig::default()
                })
                .is_err()
        );
        engine.take_dirty();
        engine
            .set_recorder_config(RecorderConfig {
                stop_hotkey: "Alt+F12".into(),
                ..RecorderConfig::default()
            })
            .unwrap();
        assert!(engine.take_dirty().setup);
        assert_eq!(engine.setup().recorder.stop_hotkey, "Alt+F12");
    }

    #[test]
    fn overlay_widgets_can_be_placed_and_configured() {
        use crate::overlay::{OverlayWidget, RadioLayout};
        let (mut engine, id) = engine_with_radio(|_| {});
        let f = engine.setup().functions[0].id.clone();
        engine.toggle_overlay_function(&f).unwrap();
        let p = |x, y| Some(WidgetPosition { x, y });

        engine
            .set_overlay_positions(vec![p(0.1, 0.2), p(2.0, -1.0)])
            .unwrap();
        let widgets = &engine.overlay().widgets;
        assert_eq!(widgets[0].position, p(0.1, 0.2));
        assert_eq!(widgets[1].position, p(1.0, 0.0));
        assert_eq!(
            engine.set_overlay_positions(vec![None]),
            Err(EngineError::WidgetCount {
                expected: 2,
                got: 1
            })
        );

        let mut radio = engine.overlay().widgets[0].clone();
        radio.widget = OverlayWidget::Radio {
            control: id.clone(),
            layout: RadioLayout::Horizontal,
        };
        radio.position = None;
        engine.set_overlay_widget(0, radio.clone()).unwrap();
        assert_eq!(engine.overlay().widgets[0], radio);

        let other = OverlayItem::from(OverlayWidget::Function {
            function: "fn-9".into(),
        });
        assert_eq!(
            engine.set_overlay_widget(0, other.clone()),
            Err(EngineError::WidgetKind)
        );
        assert_eq!(
            engine.set_overlay_widget(5, other),
            Err(EngineError::UnknownWidget(5))
        );
    }

    #[test]
    fn overlay_monitor_and_edit_mode() {
        let (mut engine, _) = engine_with_channel(EndBehavior::Wrap);
        let monitor = MonitorRef {
            name: Some("LG".into()),
            x: 1920,
            y: 0,
            width: 2560,
            height: 1440,
        };
        engine.set_overlay_monitor(Some(monitor.clone()));
        assert_eq!(engine.overlay().monitor, Some(monitor));
        assert!(engine.take_dirty().setup);
        engine.set_overlay_editing(true);
        assert!(engine.snapshot().overlay_editing);
        assert!(!engine.take_dirty().setup); // edit mode is not persisted
    }

    fn restart(engine: &Engine, setup: Setup) -> Engine {
        let mut restarted = Engine::new(engine.registry().clone(), setup, &BTreeMap::new());
        restarted.restore_active_states(&engine.active_states());
        restarted.apply(BackendEvent::Initial(vec![raw()]));
        restarted
    }

    #[test]
    fn toggle_off_trigger_turns_off_and_back_on() {
        let (mut engine, id) = engine_with_radio(|r| r.toggle_off.push(button(5)));
        // Nothing ever active: the first state.
        press(&mut engine, 5, 0);
        assert_eq!(active(&engine, &id).as_deref(), Some("A"));
        assert_eq!(keys(&mut engine), ["F1"]);
        press(&mut engine, 0, 100); // select B, remembers A
        engine.take_actions();
        press(&mut engine, 5, 200);
        assert_eq!(active(&engine, &id), None);
        assert_eq!(keys(&mut engine), ["F4"]);
        press(&mut engine, 5, 300);
        assert_eq!(active(&engine, &id).as_deref(), Some("B"));
        assert_eq!(keys(&mut engine), ["F3"]);
        // All off is remembered as well.
        hat(&mut engine, HatState::UP, 400);
        engine.take_actions();
        press(&mut engine, 5, 500);
        assert_eq!(active(&engine, &id).as_deref(), Some("B"));
        // The UI button does the same.
        engine.command_radio(&id, RadioCommand::ToggleOff).unwrap();
        assert_eq!(active(&engine, &id), None);
    }

    #[test]
    fn toggle_off_works_after_restart() {
        let (mut engine, id) = engine_with_radio(|r| r.toggle_off.push(button(5)));
        press(&mut engine, 0, 0); // B
        press(&mut engine, 5, 100); // off, remembers B
        engine.take_actions();
        assert_eq!(
            engine.active_states()[&id],
            RadioMemory {
                active: None,
                last: Some("st-2".into())
            }
        );
        let mut restarted = restart(&engine, engine.setup().clone());
        assert_eq!(active(&restarted, &id), None);
        press(&mut restarted, 5, 0);
        assert_eq!(active(&restarted, &id).as_deref(), Some("B"));
        assert_eq!(keys(&mut restarted), ["F3"]);

        // Starting empty: the state active at shutdown is the one toggle off brings back.
        let mut setup = engine.setup().clone();
        setup.profile_mut().virtual_controls[0].radio_mut().startup = Startup::Empty;
        let mut before = restart(&engine, engine.setup().clone());
        press(&mut before, 5, 0); // B active at shutdown
        let mut empty = restart(&before, setup);
        assert_eq!(active(&empty, &id), None);
        press(&mut empty, 5, 0);
        assert_eq!(active(&empty, &id).as_deref(), Some("B"));
    }

    #[test]
    fn deleting_the_remembered_state_forgets_it() {
        let (mut engine, id) = engine_with_radio(|r| r.toggle_off.push(button(5)));
        press(&mut engine, 0, 0); // B
        press(&mut engine, 5, 100); // off, remembers B
        let mut control = engine.setup().profile().virtual_controls[0].clone();
        control.radio_mut().states.remove(1);
        engine.save_virtual_control(control).unwrap();
        assert!(!engine.active_states().contains_key(&id));
        engine.take_actions();
        press(&mut engine, 5, 200);
        assert_eq!(active(&engine, &id).as_deref(), Some("A"));
    }

    #[test]
    fn silent_set_is_remembered_too() {
        let (mut engine, id) = engine_with_radio(|r| r.toggle_off.push(button(5)));
        engine.set_radio_active(&id, Some("st-3".into())).unwrap();
        engine.set_radio_active(&id, None).unwrap();
        assert!(engine.take_actions().is_empty());
        press(&mut engine, 5, 0);
        assert_eq!(active(&engine, &id).as_deref(), Some("C"));
    }

    // --- Profiles -------------------------------------------------------------------------

    use crate::profile::{DEFAULT_PROFILE_ID, NoMatch};

    fn switch_to(target: ProfileTarget) -> Action {
        Action::SwitchProfile { target }
    }

    fn active_profile_name(engine: &Engine) -> String {
        engine.setup().profile().name.clone()
    }

    /// The default profile with the radio "Comms" (button 0 selects B), and "DCS" (DCS.exe),
    /// a copy of it whose radio got its own id. The default profile is active.
    fn engine_with_two_profiles() -> (Engine, String, String) {
        let (mut engine, default_radio) = engine_with_radio(|_| {});
        let dcs = engine.duplicate_profile(DEFAULT_PROFILE_ID, "DCS").unwrap();
        engine
            .set_profile_executables(&dcs, vec!["DCS.exe".into()])
            .unwrap();
        let dcs_radio = engine.setup().profile().virtual_controls[0].id.clone();
        engine.select_profile(DEFAULT_PROFILE_ID).unwrap();
        (engine, default_radio, dcs_radio)
    }

    fn profile_id(engine: &Engine, index: usize) -> String {
        engine.setup().profiles[index].id.clone()
    }

    #[test]
    fn new_setup_has_one_default_profile() {
        let engine = Engine::new(
            DeviceRegistry::default(),
            Setup::default(),
            &BTreeMap::new(),
        );
        let snap = engine.snapshot();
        assert_eq!(snap.profiles.len(), 1);
        assert_eq!(snap.profiles[0].name, "Default");
        assert_eq!(snap.active_profile, DEFAULT_PROFILE_ID);
        assert!(snap.profile_switching.auto_switch);
        assert_eq!(snap.foreground, None);
    }

    #[test]
    fn duplicate_is_a_deep_copy_with_new_ids() {
        let (mut engine, default_radio, dcs_radio) = engine_with_two_profiles();
        assert_ne!(default_radio, dcs_radio);
        let setup = engine.setup();
        let (default, dcs) = (&setup.profiles[0], &setup.profiles[1]);
        assert_eq!(dcs.name, "DCS");
        assert_eq!(dcs.executables, ["DCS.exe"]);
        assert!(default.executables.is_empty());
        assert_eq!(
            dcs.virtual_controls[0].radio(),
            default.virtual_controls[0].radio()
        );
        // The copy's overlay shows the copy's radio.
        assert!(dcs.overlay_widgets.has_virtual_control(&dcs_radio));
        assert!(!dcs.overlay_widgets.has_virtual_control(&default_radio));
        // Editing the copy leaves the original alone.
        engine.select_profile(&profile_id(&engine, 1)).unwrap();
        let mut copy = engine.setup().profile().virtual_controls[0].clone();
        copy.name = "Comms DCS".into();
        copy.radio_mut().states.pop();
        engine.save_virtual_control(copy).unwrap();
        let setup = engine.setup();
        assert_eq!(setup.profiles[0].virtual_controls[0].name, "Comms");
        assert_eq!(
            setup.profiles[0].virtual_controls[0].radio().states.len(),
            3
        );
        assert_eq!(
            setup.profiles[1].virtual_controls[0].radio().states.len(),
            2
        );
        // A duplicate without a name gets one, and becomes active.
        let id = engine.duplicate_profile(DEFAULT_PROFILE_ID, " ").unwrap();
        assert_eq!(active_profile_name(&engine), "Default copy");
        assert_eq!(engine.setup().active_profile, id);
        // Control ids stay unique over all profiles.
        let mut ids: Vec<_> = engine.all_controls().map(|c| c.id.clone()).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), 3);
    }

    #[test]
    fn profiles_keep_their_own_radio_states_and_switching_is_silent() {
        let (mut engine, default_radio, dcs_radio) = engine_with_two_profiles();
        // Button 0 selects B in the active (default) profile only.
        press(&mut engine, 0, 0);
        assert_eq!(keys(&mut engine), ["F3"]);
        assert_eq!(active(&engine, &default_radio).as_deref(), Some("B"));
        assert!(!engine.active_states().contains_key(&dcs_radio));
        // Switching to DCS runs nothing and shows DCS's radio, with nothing active.
        engine.take_dirty();
        assert!(engine.foreground_changed(Some("dcs.exe")));
        assert!(engine.take_dirty().setup);
        assert!(engine.take_actions().is_empty());
        assert_eq!(active_profile_name(&engine), "DCS");
        let snap = engine.snapshot();
        assert_eq!(snap.virtual_controls.len(), 1);
        assert_eq!(snap.virtual_controls[0].control.id, dcs_radio);
        assert_eq!(snap.virtual_controls[0].active, None);
        assert_eq!(snap.foreground.as_deref(), Some("dcs.exe"));
        // Input now reaches DCS's radio only.
        press(&mut engine, 0, 100);
        assert_eq!(keys(&mut engine), ["F3"]);
        pulse(&mut engine, 11, 200);
        assert_eq!(keys(&mut engine), ["F4", "F5"]);
        assert_eq!(active(&engine, &dcs_radio).as_deref(), Some("C"));
        assert_eq!(
            engine.active_states()[&default_radio].active.as_deref(),
            Some("st-2")
        );
        // Back to the default profile: its state is still B, again without actions.
        engine.select_profile(DEFAULT_PROFILE_ID).unwrap();
        assert!(engine.take_actions().is_empty());
        assert_eq!(active(&engine, &default_radio).as_deref(), Some("B"));
        // Both are persisted and restored after a restart.
        let states = engine.active_states();
        assert_eq!(states[&default_radio].active.as_deref(), Some("st-2"));
        assert_eq!(states[&dcs_radio].active.as_deref(), Some("st-3"));
        let mut restarted = restart(&engine, engine.setup().clone());
        restarted.select_profile(&profile_id(&engine, 1)).unwrap();
        assert_eq!(active(&restarted, &dcs_radio).as_deref(), Some("C"));
        assert!(restarted.take_actions().is_empty());
    }

    #[test]
    fn foreground_switching_follows_the_settings() {
        let (mut engine, ..) = engine_with_two_profiles();
        assert!(engine.foreground_changed(Some("DCS.exe")));
        assert_eq!(active_profile_name(&engine), "DCS");
        // Alt-tab to Discord keeps the game's profile by default.
        engine.foreground_changed(Some("Discord.exe"));
        assert_eq!(active_profile_name(&engine), "DCS");
        // Detent's own window never switches.
        engine.foreground_changed(None);
        assert_eq!(active_profile_name(&engine), "DCS");
        engine.set_profile_switching(ProfileSwitching {
            auto_switch: true,
            no_match: NoMatch::SwitchToDefault,
        });
        engine.foreground_changed(Some("Discord.exe"));
        assert_eq!(active_profile_name(&engine), "Default");
        // Auto-switch off: only manual selection.
        engine.set_profile_switching(ProfileSwitching {
            auto_switch: false,
            no_match: NoMatch::SwitchToDefault,
        });
        engine.foreground_changed(Some("DCS.exe"));
        assert_eq!(active_profile_name(&engine), "Default");
        assert_eq!(engine.snapshot().foreground.as_deref(), Some("DCS.exe"));
        // The same program again changes nothing visible.
        assert!(!engine.foreground_changed(Some("dcs.exe")));
    }

    #[test]
    fn switch_profile_action_switches_after_the_trigger_and_wins_until_a_match() {
        let (mut engine, default_radio, dcs_radio) = engine_with_two_profiles();
        let dcs = profile_id(&engine, 1);
        engine.set_profile_switching(ProfileSwitching {
            auto_switch: true,
            no_match: NoMatch::SwitchToDefault,
        });
        // In the default profile, state B switches to DCS.
        let mut control = engine.setup().profile().virtual_controls[0].clone();
        control.radio_mut().states[1].on_activate = vec![
            tap("F3"),
            switch_to(ProfileTarget::Profile {
                profile: dcs.clone(),
            }),
        ];
        engine.save_virtual_control(control).unwrap();
        // In DCS, state B goes to the next profile (wraps to the default profile).
        engine.select_profile(&dcs).unwrap();
        let mut control = engine.setup().profile().virtual_controls[0].clone();
        control.radio_mut().states[1].on_activate = vec![switch_to(ProfileTarget::Next)];
        engine.save_virtual_control(control).unwrap();
        engine.select_profile(DEFAULT_PROFILE_ID).unwrap();

        press(&mut engine, 0, 0);
        assert_eq!(active_profile_name(&engine), "DCS");
        // The request stays queued (for the log); only the default profile's radio moved.
        let queued = engine.take_actions();
        assert_eq!(queued.len(), 2);
        assert!(matches!(queued[1].action, Action::SwitchProfile { .. }));
        assert_eq!(active(&engine, &dcs_radio), None);
        assert_eq!(
            engine.active_states()[&default_radio].active.as_deref(),
            Some("st-2")
        );

        // The manual choice wins: a program no profile lists does not fall back.
        engine.foreground_changed(Some("Discord.exe"));
        assert_eq!(active_profile_name(&engine), "DCS");
        // DCS's state B: next profile, wrapping to the first.
        press(&mut engine, 0, 100);
        assert_eq!(active_profile_name(&engine), "Default");
        // DCS comes to the front: a listed program wins over the manual choice.
        engine.foreground_changed(Some("DCS.exe"));
        assert_eq!(active_profile_name(&engine), "DCS");
        // Previous from the first wraps to the last; also from the UI's radio command.
        engine.select_profile(DEFAULT_PROFILE_ID).unwrap();
        let mut control = engine.setup().profile().virtual_controls[0].clone();
        control.radio_mut().states[0].on_activate = vec![switch_to(ProfileTarget::Previous)];
        engine.save_virtual_control(control).unwrap();
        engine.set_radio_active(&default_radio, None).unwrap();
        engine
            .command_radio(&default_radio, RadioCommand::Increment)
            .unwrap();
        assert_eq!(active_profile_name(&engine), "DCS");
        // A target that does not exist is refused when saving.
        let mut control = engine.setup().profile().virtual_controls[0].clone();
        control.radio_mut().states[0].on_activate = vec![switch_to(ProfileTarget::Profile {
            profile: "profile-99".into(),
        })];
        assert_eq!(
            engine.save_virtual_control(control),
            Err(EngineError::UnknownProfile("profile-99".into()))
        );
    }

    #[test]
    fn overlay_widgets_belong_to_the_profile_and_settings_are_global() {
        let (mut engine, default_radio, dcs_radio) = engine_with_two_profiles();
        let dcs = profile_id(&engine, 1);
        assert!(engine.toggle_overlay_profile_name());
        engine
            .set_overlay_positions(vec![Some(WidgetPosition { x: 0.5, y: 0.5 }), None])
            .unwrap();
        let mut overlay = engine.overlay();
        overlay.scale = 2.0;
        overlay.hotkey = "Ctrl+F12".into();
        engine.set_overlay(overlay);
        engine.set_overlay_visible(false);

        engine.select_profile(&dcs).unwrap();
        let snap = engine.snapshot();
        assert_eq!(snap.overlay.widgets.len(), 1);
        assert!(snap.overlay.widgets.has_virtual_control(&dcs_radio));
        assert_eq!(snap.overlay.widgets[0].position, None);
        assert!(!snap.overlay.widgets.has_profile_name());
        assert_eq!(snap.overlay.scale, 2.0);
        assert_eq!(snap.overlay.hotkey, "Ctrl+F12");
        assert!(!snap.overlay.visible);
        // Widgets never end up in the global part of the setup file.
        assert!(engine.setup().overlay.widgets.is_empty());
        let saved = Setup::from_json(&engine.setup().to_json()).unwrap();
        assert_eq!(&saved, engine.setup());

        engine.select_profile(DEFAULT_PROFILE_ID).unwrap();
        let widgets = engine.overlay().widgets;
        assert_eq!(widgets[0].position, Some(WidgetPosition { x: 0.5, y: 0.5 }));
        assert!(widgets.has_virtual_control(&default_radio));
        assert!(widgets.has_profile_name());
        // Removing a counter removes its widget from every profile.
        let f = engine.setup().functions[0].id.clone();
        engine.toggle_overlay_function(&f).unwrap();
        engine.select_profile(&dcs).unwrap();
        engine.toggle_overlay_function(&f).unwrap();
        assert_eq!(engine.overlay().widgets.len(), 2);
        engine.remove_function(&f).unwrap();
        assert_eq!(engine.setup().profiles[0].overlay_widgets.len(), 2);
        assert_eq!(engine.setup().profiles[1].overlay_widgets.len(), 1);
    }

    #[test]
    fn controls_are_found_in_any_profile() {
        let (mut engine, default_radio, dcs_radio) = engine_with_two_profiles();
        // The default profile is active; DCS's radio can still be set, edited and removed.
        engine
            .set_radio_active(&dcs_radio, Some("st-3".into()))
            .unwrap();
        assert_eq!(
            engine.active_states()[&dcs_radio].active.as_deref(),
            Some("st-3")
        );
        assert!(!engine.toggle_overlay_virtual_control(&dcs_radio).unwrap());
        assert!(
            !engine.setup().profiles[1]
                .overlay_widgets
                .has_virtual_control(&dcs_radio)
        );
        assert!(engine.overlay().widgets.has_virtual_control(&default_radio));
        engine.remove_virtual_control(&dcs_radio).unwrap();
        assert!(engine.setup().profiles[1].virtual_controls.is_empty());
        assert_eq!(engine.setup().profiles[0].virtual_controls.len(), 1);
        assert!(!engine.active_states().contains_key(&dcs_radio));
        // Removing an encoder removes its triggers in every profile.
        engine
            .duplicate_profile(DEFAULT_PROFILE_ID, "Copy")
            .unwrap();
        engine.remove_encoder("enc-1").unwrap();
        assert!(
            engine
                .all_controls()
                .all(|c| c.radio().increment.is_empty() && c.radio().decrement.is_empty())
        );
    }

    #[test]
    fn profile_management() {
        let (mut engine, _, dcs_radio) = engine_with_two_profiles();
        let dcs = profile_id(&engine, 1);
        // Create: empty and active.
        let il2 = engine.create_profile("  ");
        assert_eq!(active_profile_name(&engine), "Profile 3");
        assert!(engine.snapshot().virtual_controls.is_empty());
        assert!(engine.overlay().widgets.is_empty());
        // Rename; an empty name is ignored.
        engine.rename_profile(&il2, " IL-2 ").unwrap();
        engine.rename_profile(&il2, " ").unwrap();
        assert_eq!(active_profile_name(&engine), "IL-2");
        // Executables are cleaned up and deduplicated.
        engine
            .set_profile_executables(
                &il2,
                vec![
                    r"C:\Games\IL-2\bin\game\Il-2.exe".into(),
                    "il-2.EXE".into(),
                    "Launcher".into(),
                ],
            )
            .unwrap();
        assert_eq!(
            engine.setup().profile().executables,
            ["Il-2.exe", "Launcher.exe"]
        );
        assert_eq!(
            engine.set_profile_executables(&il2, vec!["  ".into()]),
            Err(EngineError::BadExecutable("  ".into()))
        );
        // Make default: moves to the front.
        engine.make_default_profile(&il2).unwrap();
        assert_eq!(engine.snapshot().profiles[0].name, "IL-2");
        // Deleting the active profile: the (new) default one takes over.
        engine.select_profile(&dcs).unwrap();
        engine.delete_profile(&dcs).unwrap();
        assert_eq!(active_profile_name(&engine), "IL-2");
        assert!(!engine.active_states().contains_key(&dcs_radio));
        assert_eq!(
            engine.select_profile(&dcs),
            Err(EngineError::UnknownProfile(dcs.clone()))
        );
        engine.delete_profile(&il2).unwrap();
        assert_eq!(
            engine.delete_profile(DEFAULT_PROFILE_ID),
            Err(EngineError::LastProfile)
        );
        assert_eq!(active_profile_name(&engine), "Default");
    }

    #[test]
    fn snapshot_json_has_profiles() {
        let (engine, ..) = engine_with_two_profiles();
        let json = serde_json::to_value(engine.snapshot()).unwrap();
        assert_eq!(json["activeProfile"], DEFAULT_PROFILE_ID);
        assert_eq!(json["profiles"][1]["name"], "DCS");
        assert_eq!(json["profiles"][1]["executables"][0], "DCS.exe");
        assert!(json["profiles"][1].get("virtualControls").is_none());
        assert_eq!(json["profileSwitching"]["autoSwitch"], true);
        assert_eq!(json["profileSwitching"]["noMatch"], "keepCurrent");
        assert!(json["foreground"].is_null());
        assert_eq!(json["overlay"]["widgets"][0]["type"], "radio");
    }
}

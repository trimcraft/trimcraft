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
use crate::recording::{
    RecordKind, RecorderConfig, RecorderError, RecorderPhase, RecorderView, RecordingResult,
};
use crate::registry::DeviceRegistry;
use crate::setup::{ButtonPair, Setup};
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
    /// Active and most recently active state per virtual control.
    memory: HashMap<String, RadioMemory>,
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
        for control in &self.setup.virtual_controls {
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
                for trigger in &fired {
                    self.fire(trigger);
                }
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

    /// Runs `trigger` through every virtual control, in setup order.
    fn fire(&mut self, trigger: &Trigger) {
        for control in &self.setup.virtual_controls {
            for command in control.radio().commands_for(trigger) {
                self.dirty.controls |=
                    run_radio_command(control, command, &mut self.memory, &mut self.actions);
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
            virtual_controls: self
                .setup
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
            overlay: self.setup.overlay.clone(),
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
        for control in &mut self.setup.virtual_controls {
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
        self.setup.overlay.remove_function(id);
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
        self.setup.overlay.toggle_control(device, control)
    }

    pub fn toggle_overlay_function(&mut self, id: &str) -> Result<bool, EngineError> {
        self.function(id)?;
        self.dirty.setup = true;
        Ok(self.setup.overlay.toggle_function(id))
    }

    /// Replaces layout settings (anchor, margin, scale, hotkey, visibility, widget order).
    pub fn set_overlay(&mut self, overlay: OverlayConfig) {
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
            .overlay
            .widgets
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
        let widgets = &mut self.setup.overlay.widgets;
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

    /// Creates the control if `control.id` is empty, updates it otherwise. Returns its id.
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
        control.name = control.name.trim().to_owned();
        if control.name.is_empty() {
            control.name = "Radio".into();
        }
        if control.id.is_empty() {
            control.id = next_id(
                "vc",
                self.setup.virtual_controls.iter().map(|c| c.id.as_str()),
            );
            self.setup.overlay.toggle_virtual_control(&control.id);
            self.setup.virtual_controls.push(control.clone());
        } else {
            let slot = self
                .setup
                .virtual_controls
                .iter_mut()
                .find(|c| c.id == control.id)
                .ok_or_else(|| EngineError::UnknownControl(control.id.clone()))?;
            *slot = control.clone();
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
        self.virtual_control(id)?;
        self.setup.virtual_controls.retain(|c| c.id != id);
        self.setup.overlay.remove_virtual_control(id);
        self.memory.remove(id);
        self.dirty.setup = true;
        self.dirty.controls = true;
        Ok(())
    }

    /// Sets the active state (or none) without running actions, e.g. to match the game.
    pub fn set_radio_active(&mut self, id: &str, state: Option<String>) -> Result<(), EngineError> {
        let radio = self
            .setup
            .virtual_controls
            .iter()
            .find(|c| c.id == id)
            .ok_or_else(|| EngineError::UnknownControl(id.into()))?
            .radio();
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
        let control = self
            .setup
            .virtual_controls
            .iter()
            .find(|c| c.id == id)
            .ok_or_else(|| EngineError::UnknownControl(id.into()))?;
        self.dirty.controls |=
            run_radio_command(control, command, &mut self.memory, &mut self.actions);
        Ok(())
    }

    pub fn toggle_overlay_virtual_control(&mut self, id: &str) -> Result<bool, EngineError> {
        self.virtual_control(id)?;
        self.dirty.setup = true;
        Ok(self.setup.overlay.toggle_virtual_control(id))
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

    fn virtual_control(&self, id: &str) -> Result<&VirtualControl, EngineError> {
        self.setup
            .virtual_controls
            .iter()
            .find(|c| c.id == id)
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

fn next_id<'a>(prefix: &str, existing: impl Iterator<Item = &'a str>) -> String {
    let max = existing
        .filter_map(|id| {
            id.strip_prefix(prefix)?
                .strip_prefix('-')?
                .parse::<u32>()
                .ok()
        })
        .max()
        .unwrap_or(0);
    format!("{prefix}-{}", max + 1)
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
                Action::MouseMacro { .. } | Action::Soundpad { .. } | Action::Discord { .. } => {
                    vec![]
                }
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
        setup.virtual_controls[0].radio_mut().startup = Startup::Empty;
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
        let mut control = engine.setup().virtual_controls[0].clone();
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
        let mut control = engine.setup().virtual_controls[0].clone();
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
        assert!(engine.setup().overlay.has_virtual_control(&id));
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
        let saved = &engine.setup().virtual_controls[0];
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
        let radio = engine.setup().virtual_controls[0].radio().clone();
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
        let widgets = &engine.setup().overlay.widgets;
        assert_eq!(widgets[0].position, p(0.1, 0.2));
        assert_eq!(widgets[1].position, p(1.0, 0.0));
        assert_eq!(
            engine.set_overlay_positions(vec![None]),
            Err(EngineError::WidgetCount {
                expected: 2,
                got: 1
            })
        );

        let mut radio = engine.setup().overlay.widgets[0].clone();
        radio.widget = OverlayWidget::Radio {
            control: id.clone(),
            layout: RadioLayout::Horizontal,
        };
        radio.position = None;
        engine.set_overlay_widget(0, radio.clone()).unwrap();
        assert_eq!(engine.setup().overlay.widgets[0], radio);

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
        assert_eq!(engine.setup().overlay.monitor, Some(monitor));
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
        setup.virtual_controls[0].radio_mut().startup = Startup::Empty;
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
        let mut control = engine.setup().virtual_controls[0].clone();
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
}

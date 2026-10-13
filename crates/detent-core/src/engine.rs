//! Ties everything together: live input, encoder detection, stateful functions and virtual
//! controls.
//!
//! The engine is driven by backend events one at a time, so every encoder pulse is counted
//! when it happens, independent of how often the UI or overlay redraws.

use crate::action::{Action, Movement};
use crate::connection::{ConnectionError, Connections, ConnectorId, ConnectorState, ConnectorView};
use crate::device::DeviceId;
use crate::device::DeviceKind;
use crate::diagnostics::{self, AxisThrottle, Category, Diagnostics, Level, Record};
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
use crate::settings::{AppSettings, ShellStatus};
use crate::setup::{ButtonPair, Setup, next_id};
use crate::state::ButtonEdge;
use crate::state::{LiveState, Snapshot};
use crate::trigger::{
    ButtonRef, Condition, DigitalKey, Fired, HatDirection, HoldKey, PressConfig, PressEvent,
    PressKind, PressTracker, StepDirection, Trigger, TriggerInput, ZoneKey, axis_percent,
};
use crate::virtual_control::{
    ActionRequest, Command, ControlError, CounterCommand, ModeCommand, RadioCommand, RadioMemory,
    Startup, TOGGLE_ON, Toggle, ToggleCommand, VirtualControl, VirtualControlKind,
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
    #[error("unknown mode {0} (conditions need a mode of the same profile)")]
    UnknownMode(String),
    #[error("this command does not apply to this kind of control")]
    NotForControl,
}

/// Commands the UI runs on a control, as if a trigger had fired.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ManualCommand {
    /// Next state (radio, mode), counter up.
    Increment,
    /// Previous state (radio, mode), counter down.
    Decrement,
    Clear,
    ToggleOff,
    Toggle,
    On,
    Off,
    Reset,
}

/// Default name of a control of this kind.
fn kind_name(kind: &VirtualControlKind) -> &'static str {
    match kind {
        VirtualControlKind::Radio(_) => "Radio",
        VirtualControlKind::Toggle(_) => "Toggle",
        VirtualControlKind::Momentary(_) => "Momentary",
        VirtualControlKind::Counter(_) => "Counter",
        VirtualControlKind::Mode(_) => "Mode",
    }
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
    /// Id of the active state, if any (radio, mode; `"on"` for a toggle that is on).
    pub active: Option<String>,
    /// Name of the active state, for display.
    pub active_name: Option<String>,
    /// Lit: a radio state is active, a toggle or momentary control is on, always for a mode.
    pub on: bool,
    /// A counter's value, and formatted.
    pub value: Option<f64>,
    pub display: Option<String>,
    /// A mode held in a state by a shift trigger.
    pub shifted: bool,
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
    /// App-wide settings (Settings page).
    pub settings: AppSettings,
    pub shell: ShellStatus,
}

#[derive(Debug, Clone, PartialEq)]
enum Learn {
    Off,
    /// Axis positions (percent) when an axis was first seen in learn mode.
    Waiting(HashMap<(DeviceId, u16), f32>),
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
    /// Press, long and double press tracking per button and hat direction.
    presses: HashMap<DigitalKey, PressTracker>,
    /// Whether each axis zone in use is entered.
    zones: HashMap<ZoneKey, bool>,
    /// Momentary controls and mode shifts held right now.
    runtime: Runtime,
    /// Actions emitted by triggers, oldest first, until [`Engine::take_actions`].
    actions: Vec<ActionRequest>,
    learn: Learn,
    overlay_editing: bool,
    recorder: RecorderView,
    connector_states: HashMap<ConnectorId, ConnectorState>,
    /// Elevation, autostart and foreground warning, as the app reports them.
    shell: ShellStatus,
    dirty: Dirty,
    /// Event log sink; without one nothing is recorded (tests, tools).
    diagnostics: Option<Diagnostics>,
    axis_throttle: AxisThrottle,
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
            presses: HashMap::new(),
            zones: HashMap::new(),
            runtime: Runtime::default(),
            actions: Vec::new(),
            learn: Learn::Off,
            overlay_editing: false,
            recorder: RecorderView::default(),
            connector_states: HashMap::new(),
            shell: ShellStatus::default(),
            dirty: Dirty::default(),
            diagnostics: None,
            axis_throttle: AxisThrottle::default(),
        }
    }

    /// Reports input edges, trigger matches and state changes to the event log.
    pub fn set_diagnostics(&mut self, diagnostics: Diagnostics) {
        self.diagnostics = Some(diagnostics);
    }

    /// Logs the input behind `applied`: axis moves (throttled), button and hat edges.
    fn log_input(
        &mut self,
        axis: Option<(DeviceId, u16, f32, u64)>,
        applied: &crate::state::Applied,
    ) {
        let Some(log) = &self.diagnostics else { return };
        if let Some((device, index, value, time_ns)) = axis
            && let Some(previous) = self.axis_throttle.admit(&device.0, index, value, time_ns)
        {
            log.record(
                with_device(
                    &self.live,
                    &device,
                    Record::new(Category::Input, diagnostics::axis_text(index, value)),
                )
                .detail(diagnostics::axis_detail(value, previous)),
            );
        }
        if let Some(edge) = &applied.button {
            log.record(with_device(
                &self.live,
                &edge.device,
                Record::new(
                    Category::Input,
                    diagnostics::button_text(edge.index, edge.pressed),
                ),
            ));
        }
        if let Some(edge) = &applied.hat {
            log.record(with_device(
                &self.live,
                &edge.device,
                Record::new(
                    Category::Input,
                    diagnostics::hat_text(edge.index, edge.previous, edge.state),
                ),
            ));
        }
    }

    /// Logs an encoder detent.
    fn log_detent(&self, encoder: &str, direction: StepDirection, button: Option<&ButtonEdge>) {
        let Some(log) = &self.diagnostics else { return };
        let binding = self.setup.encoders.iter().find(|e| e.id == encoder);
        let name = binding.map_or(encoder, |e| e.name.as_str());
        let mut record = Record::new(
            Category::Input,
            diagnostics::encoder_text(name, direction == StepDirection::Increment),
        );
        if let Some(edge) = button {
            record = with_device(&self.live, &edge.device, record)
                .detail(format!("from button {} pressed", u32::from(edge.index) + 1));
        }
        log.record(record);
    }

    /// Restores what was saved at the last run (active and most recently active state,
    /// counter values), without running any actions. States that no longer exist are
    /// dropped. A control set to start empty starts empty: a radio with nothing active (but
    /// remembering the state that was active, so toggle off brings it back), a toggle off, a
    /// counter at its initial value, a mode in its first state.
    pub fn restore_active_states(&mut self, saved: &BTreeMap<String, RadioMemory>) {
        for control in self.setup.profiles.iter().flat_map(|p| &p.virtual_controls) {
            let Some(mut memory) = saved.get(&control.id).cloned() else {
                continue;
            };
            memory.retain_existing(control);
            if control.startup() == Startup::Empty {
                match &control.kind {
                    VirtualControlKind::Radio(_) => {
                        memory.last = memory.active.take().or(memory.last);
                    }
                    _ => memory = RadioMemory::default(),
                }
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
    /// emit are queued in order and collected with [`Engine::take_actions`]. Long and double
    /// presses that are due before the event are handled first (see [`Engine::advance`]).
    pub fn apply(&mut self, event: BackendEvent) -> bool {
        let mut changed = false;
        let time = match &event {
            BackendEvent::Axis { time_ns, .. }
            | BackendEvent::Button { time_ns, .. }
            | BackendEvent::Hat { time_ns, .. } => Some(*time_ns),
            _ => None,
        };
        if let Some(t) = time {
            changed |= self.advance(t);
        }
        let axis = match &event {
            BackendEvent::Axis {
                instance,
                index,
                value,
                time_ns,
            } => self
                .live
                .device_id(*instance)
                .map(|d| (d.clone(), *index, *value, *time_ns)),
            _ => None,
        };
        let applied = self.live.apply(event, &mut self.registry);
        self.dirty.registry |= applied.devices_changed;
        if self.diagnostics.is_some() {
            self.log_input(axis.clone(), &applied);
        }
        let mut fired = Vec::new();
        if let Some(edge) = &applied.button {
            self.detector
                .observe(&edge.device, edge.index, edge.pressed, edge.time_ns);
            if edge.pressed {
                for (encoder, direction) in self.step_encoders(&edge.device, edge.index) {
                    self.log_detent(&encoder, direction, Some(edge));
                    fired.push(Fired::Encoder { encoder, direction });
                }
            }
            if self.is_input_device(&edge.device) {
                let key = DigitalKey::Button {
                    device: edge.device.clone(),
                    index: edge.index,
                };
                fired.extend(self.digital_edge(key, edge.pressed, edge.time_ns));
            }
        }
        if let Some(edge) = &applied.hat
            && self.is_input_device(&edge.device)
        {
            let before = HatDirection::from_state(edge.previous);
            let after = HatDirection::from_state(edge.state);
            if before != after {
                let key = |direction| DigitalKey::Hat {
                    device: edge.device.clone(),
                    index: edge.index,
                    direction,
                };
                let t = time.unwrap_or(0);
                if let Some(d) = before {
                    fired.extend(self.digital_edge(key(d), false, t));
                }
                if let Some(d) = after {
                    fired.extend(self.digital_edge(key(d), true, t));
                }
            }
        }
        if let Some((device, index, value, _)) = &axis
            && self.is_input_device(device)
        {
            fired.extend(self.axis_zones(device, *index, *value));
        }
        if matches!(self.learn, Learn::Waiting(_)) {
            if let Some(captured) = self.learn_capture(&fired, axis.as_ref()) {
                if let Some(log) = &self.diagnostics {
                    log.record(Record::new(
                        Category::Trigger,
                        format!(
                            "Learn captured {}",
                            trigger_text(&self.live, &self.setup, &captured)
                        ),
                    ));
                }
                self.learn = Learn::Captured(captured);
                changed = true;
            }
            // Learning swallows input; held buttons start fresh afterwards.
            self.presses.clear();
        } else if !fired.is_empty() {
            self.dispatch(fired);
        }
        changed || applied.state_changed
    }

    /// The earliest time (backend nanoseconds) at which a long press or a delayed short
    /// press is due; the app calls [`Engine::advance`] then. `None`: nothing pending.
    pub fn next_deadline(&self) -> Option<u64> {
        self.presses
            .values()
            .filter_map(PressTracker::deadline)
            .min()
    }

    /// Handles the long presses and delayed short presses due at `now_ns` (backend clock).
    /// Returns whether anything visible changed.
    pub fn advance(&mut self, now_ns: u64) -> bool {
        let mut due: Vec<(DigitalKey, PressEvent)> = Vec::new();
        for (key, tracker) in &mut self.presses {
            due.extend(tracker.due(now_ns).into_iter().map(|e| (key.clone(), e)));
        }
        self.presses.retain(|_, t| !t.is_idle());
        if due.is_empty() || matches!(self.learn, Learn::Waiting(_)) {
            return false;
        }
        // Earlier deadlines first; ties keep a stable order.
        due.sort_by_key(|(key, _)| format!("{key:?}"));
        let controls_before = self.dirty.controls;
        self.dirty.controls = false;
        self.dispatch(
            due.into_iter()
                .map(|(key, e)| Fired::Digital(key, e))
                .collect(),
        );
        let changed = self.dirty.controls;
        self.dirty.controls |= controls_before;
        changed
    }

    /// Press and release of a button or hat direction, through its press tracker.
    fn digital_edge(&mut self, key: DigitalKey, pressed: bool, time_ns: u64) -> Vec<Fired> {
        let cfg = self.press_config(&key);
        let tracker = self.presses.entry(key.clone()).or_default();
        let events = if pressed {
            tracker.down(time_ns, &cfg)
        } else {
            tracker.up(time_ns, &cfg)
        };
        if tracker.is_idle() {
            self.presses.remove(&key);
        }
        events
            .into_iter()
            .map(|e| Fired::Digital(key.clone(), e))
            .collect()
    }

    /// Long and double press times of the active profile's triggers on `key`.
    fn press_config(&self, key: &DigitalKey) -> PressConfig {
        let profile = self.setup.profile();
        let bindings: Vec<(Command, &Trigger)> = profile
            .virtual_controls
            .iter()
            .flat_map(VirtualControl::bindings)
            .collect();
        PressConfig::from_kinds(
            bindings
                .iter()
                .filter(|(_, t)| t.input.digital_key().as_ref() == Some(key))
                .map(|(_, t)| &t.press),
        )
    }

    /// Zones of the active profile on this axis that the value entered or left.
    fn axis_zones(&mut self, device: &DeviceId, index: u16, value: f32) -> Vec<Fired> {
        let percent = axis_percent(value);
        let mut keys: Vec<ZoneKey> = Vec::new();
        for control in &self.setup.profile().virtual_controls {
            for (_, t) in control.bindings() {
                if let Some(z) = t.input.zone_key()
                    && z.device == *device
                    && z.index == index
                    && !keys.contains(&z)
                {
                    keys.push(z);
                }
            }
        }
        let mut fired = Vec::new();
        for zone in keys {
            match self.zones.get(&zone).copied() {
                // First value seen: where the axis is counts as the start, nothing fires.
                None => {
                    let inside = zone.inside(percent, false);
                    self.zones.insert(zone, inside);
                }
                Some(was) => {
                    let inside = zone.inside(percent, was);
                    if inside != was {
                        self.zones.insert(zone.clone(), inside);
                        fired.push(Fired::Zone {
                            zone,
                            entered: inside,
                        });
                    }
                }
            }
        }
        fired
    }

    /// In learn mode: the trigger for what just happened, if it is one. An encoder detent
    /// wins over the plain button press; an axis counts once it moved a quarter of its range,
    /// and gets a zone around where it is.
    fn learn_capture(
        &mut self,
        fired: &[Fired],
        axis: Option<&(DeviceId, u16, f32, u64)>,
    ) -> Option<Trigger> {
        for f in fired {
            match f {
                Fired::Encoder { encoder, direction } => {
                    return Some(
                        TriggerInput::EncoderStep {
                            encoder: encoder.clone(),
                            direction: *direction,
                        }
                        .into(),
                    );
                }
                Fired::Digital(key, PressEvent::Down) => {
                    return Some(
                        match key.clone() {
                            DigitalKey::Button { device, index } => {
                                TriggerInput::Button { device, index }
                            }
                            DigitalKey::Hat {
                                device,
                                index,
                                direction,
                            } => TriggerInput::Hat {
                                device,
                                index,
                                direction,
                            },
                        }
                        .into(),
                    );
                }
                _ => {}
            }
        }
        let (device, index, value, _) = axis?;
        if !self.is_input_device(device) {
            return None;
        }
        let Learn::Waiting(baseline) = &mut self.learn else {
            return None;
        };
        let percent = axis_percent(*value);
        let start = *baseline.entry((device.clone(), *index)).or_insert(percent);
        if (percent - start).abs() < LEARN_AXIS_MOVE {
            return None;
        }
        let min = (percent - LEARN_ZONE_HALF).max(0.0).round();
        let max = (percent + LEARN_ZONE_HALF).min(100.0).round();
        Some(
            TriggerInput::AxisZone {
                device: device.clone(),
                index: *index,
                min,
                max,
            }
            .into(),
        )
    }

    /// Buttons and hats of virtual devices (vJoy) are Detent's own output, never triggers.
    fn is_input_device(&self, device: &DeviceId) -> bool {
        self.live
            .device(device)
            .is_some_and(|d| d.kind == DeviceKind::Physical)
    }

    fn button_held(&self, button: &ButtonRef) -> bool {
        self.live
            .device(&button.device)
            .and_then(|d| d.state.buttons.get(usize::from(button.index)).copied())
            .unwrap_or(false)
    }

    /// The active state id of mode `id` (any profile); `None` if it is no mode.
    fn mode_state(&self, id: &str) -> Option<&str> {
        let control = self.all_controls().find(|c| c.id == id)?;
        let mode = control.mode()?;
        let memory = self.memory.get(id).cloned().unwrap_or_default();
        mode.active(&memory).map(|i| mode.states[i].id.as_str())
    }

    fn conditions_hold(&self, conditions: &[Condition]) -> bool {
        conditions
            .iter()
            .all(|c| self.mode_state(&c.mode) == Some(c.state.as_str()))
    }

    /// Runs fired events in order, then the profile switches they caused, so the events of
    /// one input edge all reach the same profile.
    fn dispatch(&mut self, fired: Vec<Fired>) {
        let queued = self.actions.len();
        for f in &fired {
            self.fire(f);
        }
        self.run_profile_switches(queued);
    }

    /// Runs `fired` through the triggers of the active profile's controls, in setup order.
    /// Triggers whose conditions do not hold or whose modifiers are not held are skipped;
    /// of the rest, only the most specific (most modifiers) fire. All are chosen before any
    /// runs, so a mode change by one of them does not affect the others. A release (or
    /// leaving a zone) also ends what its press started holding.
    fn fire(&mut self, fired: &Fired) {
        let active = self.setup.active_index();
        let mut chosen: Vec<(usize, Command, usize)> = Vec::new();
        for (ci, control) in self.setup.profiles[active]
            .virtual_controls
            .iter()
            .enumerate()
        {
            for (cmd, t) in control.bindings() {
                if t.matches(fired, cmd.is_hold())
                    && self.conditions_hold(&t.conditions)
                    && t.modifiers.iter().all(|m| self.button_held(m))
                {
                    chosen.push((ci, cmd, t.modifiers.len()));
                }
            }
        }
        let most = chosen.iter().map(|c| c.2).max().unwrap_or(0);
        chosen.retain(|c| c.2 == most);
        let hold = fired.hold_key();
        let text = (self.diagnostics.is_some() && (!chosen.is_empty()))
            .then(|| fired_text(&self.live, &self.setup, fired));
        for (ci, cmd, _) in chosen {
            let control = &self.setup.profiles[active].virtual_controls[ci];
            self.dirty.controls |= run_command(
                control,
                cmd,
                hold.as_ref(),
                &mut self.memory,
                &mut self.runtime,
                &mut self.actions,
                self.diagnostics.as_ref().zip(text.as_deref()),
            );
        }
        let released = matches!(fired, Fired::Digital(_, PressEvent::Up))
            || matches!(fired, Fired::Zone { entered: false, .. });
        if released && let Some(key) = hold {
            self.end_holds(&key);
        }
    }

    /// The input `key` was released: momentary controls it held let go, mode shifts it
    /// held go back. In whichever profile the control is (a profile switch while holding
    /// must not leave it stuck).
    fn end_holds(&mut self, key: &HoldKey) {
        let ids: Vec<String> = self
            .runtime
            .held
            .iter()
            .filter(|(_, keys)| keys.contains(key))
            .map(|(id, _)| id.clone())
            .chain(
                self.runtime
                    .shifts
                    .iter()
                    .filter(|(_, s)| s.held.iter().any(|(k, _)| k == key))
                    .map(|(id, _)| id.clone()),
            )
            .collect();
        for id in ids {
            let Ok((p, c)) = self.control_position(&id) else {
                self.runtime.held.remove(&id);
                self.runtime.shifts.remove(&id);
                continue;
            };
            let control = &self.setup.profiles[p].virtual_controls[c];
            self.dirty.controls |= release_hold(
                control,
                key,
                &mut self.memory,
                &mut self.runtime,
                &mut self.actions,
                self.diagnostics.as_ref(),
            );
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
                self.activate_profile(index, "Switch profile action");
            }
        }
    }

    /// Actions emitted since the last call, in the order they must run.
    pub fn take_actions(&mut self) -> Vec<ActionRequest> {
        std::mem::take(&mut self.actions)
    }

    /// Steps the counters bound to encoders; returns a detent per encoder this press belongs
    /// to.
    fn step_encoders(&mut self, device: &DeviceId, button: u16) -> Vec<(String, StepDirection)> {
        let mut fired = Vec::new();
        for encoder in &self.setup.encoders {
            let direction = encoder.direction(device, button);
            if direction == 0 {
                continue;
            }
            fired.push((
                encoder.id.clone(),
                if direction > 0 {
                    StepDirection::Increment
                } else {
                    StepDirection::Decrement
                },
            ));
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

    /// Active and most recently active state per virtual control, and counter values, for
    /// persisting.
    pub fn active_states(&self) -> BTreeMap<String, RadioMemory> {
        self.memory
            .iter()
            .filter(|(_, m)| !m.is_empty())
            .map(|(c, m)| (c.clone(), m.clone()))
            .collect()
    }

    fn control_view(&self, control: &VirtualControl) -> VirtualControlView {
        let memory = self.memory.get(&control.id).cloned().unwrap_or_default();
        let named = |id: &str, name: &str| (Some(id.to_owned()), Some(name.to_owned()));
        let (mut view, on) = match &control.kind {
            VirtualControlKind::Radio(r) => {
                let i = memory.active.as_deref().and_then(|id| r.index_of(id));
                let (a, n) = i.map_or((None, None), |i| named(&r.states[i].id, &r.states[i].name));
                ((a, n, None, None), i.is_some())
            }
            VirtualControlKind::Mode(m) => {
                let i = m.active(&memory);
                let (a, n) = i.map_or((None, None), |i| named(&m.states[i].id, &m.states[i].name));
                ((a, n, None, None), true)
            }
            VirtualControlKind::Toggle(_) => {
                let on = Toggle::is_on(&memory);
                let (a, n) = if on {
                    named(TOGGLE_ON, "On")
                } else {
                    (None, None)
                };
                ((a, n, None, None), on)
            }
            VirtualControlKind::Momentary(m) => {
                let held = self
                    .runtime
                    .held
                    .get(&control.id)
                    .is_some_and(|k| !k.is_empty());
                ((None, None, None, None), m.active(held))
            }
            VirtualControlKind::Counter(c) => {
                let value = c.value(&memory);
                ((None, None, Some(value), Some(c.spec.format(value))), false)
            }
        };
        VirtualControlView {
            control: control.clone(),
            active: view.0.take(),
            active_name: view.1.take(),
            on,
            value: view.2,
            display: view.3.take(),
            shifted: self.runtime.shifts.contains_key(&control.id),
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
                Learn::Waiting(_) => Some(LearnView { captured: None }),
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
            settings: self.setup.settings.clone(),
            shell: self.shell.clone(),
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
            for slot in control.triggers_mut() {
                slot.retain(
                    |t| !matches!(&t.input, TriggerInput::EncoderStep { encoder, .. } if encoder == id),
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
    /// starts with no state active and is put on the overlay. What is held right now
    /// (momentary presses, mode shifts) is dropped for the edited control.
    pub fn save_virtual_control(
        &mut self,
        mut control: VirtualControl,
    ) -> Result<String, EngineError> {
        control.normalize()?;
        let profile = if control.id.is_empty() {
            self.setup.active_index()
        } else {
            self.control_position(&control.id)?.0
        };
        for trigger in control.triggers() {
            if let TriggerInput::EncoderStep { encoder, .. } = &trigger.input
                && !self.setup.encoders.iter().any(|e| &e.id == encoder)
            {
                return Err(EngineError::UnknownEncoder(encoder.clone()));
            }
            for condition in &trigger.conditions {
                let mode = if condition.mode == control.id {
                    control.mode()
                } else {
                    self.setup.profiles[profile]
                        .virtual_controls
                        .iter()
                        .find(|c| c.id == condition.mode)
                        .and_then(VirtualControl::mode)
                };
                let mode = mode.ok_or_else(|| EngineError::UnknownMode(condition.mode.clone()))?;
                if mode.index_of(&condition.state).is_none() {
                    return Err(EngineError::UnknownState(condition.state.clone()));
                }
            }
        }
        for action in control.actions() {
            if let Action::SwitchProfile {
                target: ProfileTarget::Profile { profile },
            } = action
            {
                self.profile_index(profile)?;
            }
        }
        control.name = control.name.trim().to_owned();
        if control.name.is_empty() {
            control.name = kind_name(&control.kind).into();
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
                memory.retain_existing(&control);
                self.dirty.controls |= *memory != before;
            }
            self.runtime.held.remove(&control.id);
            self.runtime.shifts.remove(&control.id);
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
        self.runtime.held.remove(id);
        self.runtime.shifts.remove(id);
        self.dirty.setup = true;
        self.dirty.controls = true;
        Ok(())
    }

    /// Sets the state without running actions, e.g. to match the game: a radio's or mode's
    /// state (`None`: radio off, mode in its first state), a toggle on (`Some("on")`) or off.
    pub fn set_radio_active(&mut self, id: &str, state: Option<String>) -> Result<(), EngineError> {
        let (p, c) = self.control_position(id)?;
        let control = &self.setup.profiles[p].virtual_controls[c];
        let memory = self.memory.entry(id.into()).or_default();
        let index = |found: Option<usize>, state: &String| {
            found.ok_or_else(|| EngineError::UnknownState(state.clone()))
        };
        match (&control.kind, state) {
            (VirtualControlKind::Radio(r), Some(s)) => {
                r.set(memory, Some(index(r.index_of(&s), &s)?))
            }
            (VirtualControlKind::Radio(r), None) => r.set(memory, None),
            (VirtualControlKind::Mode(m), Some(s)) => m.set(memory, index(m.index_of(&s), &s)?),
            (VirtualControlKind::Mode(m), None) => m.set(memory, 0),
            (VirtualControlKind::Toggle(_), s) => Toggle::set(memory, s.is_some()),
            _ => return Err(EngineError::NotForControl),
        }
        self.runtime.shifts.remove(id);
        self.dirty.controls = true;
        Ok(())
    }

    /// Sets a counter control's value without running actions.
    pub fn set_control_value(&mut self, id: &str, value: f64) -> Result<(), EngineError> {
        let (p, c) = self.control_position(id)?;
        let VirtualControlKind::Counter(counter) = &self.setup.profiles[p].virtual_controls[c].kind
        else {
            return Err(EngineError::NotForControl);
        };
        if !value.is_finite() {
            return Err(EngineError::NotForControl);
        }
        counter.set(self.memory.entry(id.into()).or_default(), value);
        self.dirty.controls = true;
        Ok(())
    }

    /// Runs a radio command as if a trigger had fired, including its actions.
    pub fn command_radio(&mut self, id: &str, command: RadioCommand) -> Result<(), EngineError> {
        self.run_manual(id, Command::Radio(command))
    }

    /// Runs a command from the UI as if a trigger had fired, including its actions:
    /// next/previous (radio, mode, counter), clear and toggle off (radio), toggle/on/off
    /// (toggle), reset (counter).
    pub fn command_control(&mut self, id: &str, command: ManualCommand) -> Result<(), EngineError> {
        let (p, c) = self.control_position(id)?;
        let cmd = match (&self.setup.profiles[p].virtual_controls[c].kind, command) {
            (VirtualControlKind::Radio(_), ManualCommand::Increment) => {
                Command::Radio(RadioCommand::Increment)
            }
            (VirtualControlKind::Radio(_), ManualCommand::Decrement) => {
                Command::Radio(RadioCommand::Decrement)
            }
            (VirtualControlKind::Radio(_), ManualCommand::Clear) => {
                Command::Radio(RadioCommand::Clear)
            }
            (VirtualControlKind::Radio(_), ManualCommand::ToggleOff) => {
                Command::Radio(RadioCommand::ToggleOff)
            }
            (VirtualControlKind::Mode(_), ManualCommand::Increment) => {
                Command::Mode(ModeCommand::Next)
            }
            (VirtualControlKind::Mode(_), ManualCommand::Decrement) => {
                Command::Mode(ModeCommand::Previous)
            }
            (VirtualControlKind::Toggle(_), ManualCommand::Toggle) => {
                Command::Toggle(ToggleCommand::Toggle)
            }
            (VirtualControlKind::Toggle(_), ManualCommand::On) => {
                Command::Toggle(ToggleCommand::On)
            }
            (VirtualControlKind::Toggle(_), ManualCommand::Off) => {
                Command::Toggle(ToggleCommand::Off)
            }
            (VirtualControlKind::Counter(_), ManualCommand::Increment) => {
                Command::Counter(CounterCommand::Increment)
            }
            (VirtualControlKind::Counter(_), ManualCommand::Decrement) => {
                Command::Counter(CounterCommand::Decrement)
            }
            (VirtualControlKind::Counter(_), ManualCommand::Reset) => {
                Command::Counter(CounterCommand::Reset)
            }
            _ => return Err(EngineError::NotForControl),
        };
        self.run_manual(id, cmd)
    }

    fn run_manual(&mut self, id: &str, command: Command) -> Result<(), EngineError> {
        let (p, c) = self.control_position(id)?;
        let queued = self.actions.len();
        self.dirty.controls |= run_command(
            &self.setup.profiles[p].virtual_controls[c],
            command,
            None,
            &mut self.memory,
            &mut self.runtime,
            &mut self.actions,
            self.diagnostics.as_ref().zip(Some("manual command")),
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

    /// Starts learn mode: the next button press, hat direction, encoder detent or axis
    /// movement is captured (see [`EngineSnapshot::learn`]) instead of firing triggers.
    pub fn start_learn(&mut self) {
        let mut baseline = HashMap::new();
        for device in self.live.snapshot().devices {
            for (i, v) in device.state.axes.iter().enumerate() {
                if let Ok(i) = u16::try_from(i) {
                    baseline.insert((device.id.clone(), i), axis_percent(*v));
                }
            }
        }
        self.learn = Learn::Waiting(baseline);
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
        if let Some(log) = &self.diagnostics {
            log.record(connector_record(id, self.connector_states.get(&id), &state));
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

    /// Saves the Settings page. Whether the tray notice was shown is the app's to change, so it
    /// is kept.
    pub fn set_settings(&mut self, settings: AppSettings) {
        self.setup.settings = AppSettings {
            tray_notice_shown: self.setup.settings.tray_notice_shown,
            ..settings
        };
        self.dirty.setup = true;
    }

    /// Remembers that the one-time "keeps running in the tray" notification was shown.
    pub fn mark_tray_notice_shown(&mut self) {
        if !self.setup.settings.tray_notice_shown {
            self.setup.settings.tray_notice_shown = true;
            self.dirty.setup = true;
        }
    }

    pub fn shell_status(&self) -> &ShellStatus {
        &self.shell
    }

    /// Stores what the app found out about elevation, autostart and the foreground program.
    /// Returns whether anything changed.
    pub fn set_shell_status(&mut self, status: ShellStatus) -> bool {
        if self.shell == status {
            return false;
        }
        if let Some(log) = &self.diagnostics
            && self.shell.elevated_foreground != status.elevated_foreground
        {
            log.record(match &status.elevated_foreground {
                Some(name) => Record::new(
                    Category::Log,
                    format!(
                        "{name} runs as administrator; Detent's keys and mouse will not reach it"
                    ),
                )
                .level(Level::Warn)
                .detail("restart Detent as administrator (Settings) to fix this"),
                None => Record::new(
                    Category::Log,
                    "No elevated program in the foreground any more",
                ),
            });
        }
        self.shell = status;
        true
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
    fn activate_profile(&mut self, index: usize, reason: &str) -> bool {
        let id = &self.setup.profiles[index].id;
        if *id == self.setup.active_profile {
            return false;
        }
        if let Some(log) = &self.diagnostics {
            let name = |i: usize| self.setup.profiles[i].name.as_str();
            log.record(
                Record::new(
                    Category::Control,
                    format!(
                        "Profile: {} → {}",
                        name(self.setup.active_index()),
                        name(index)
                    ),
                )
                .detail(reason),
            );
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
        self.activate_profile(index, "selected manually");
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
        let reason = match exe {
            Some(exe) => format!("foreground program {exe}"),
            None => "foreground program not recognised".to_owned(),
        };
        let switched = switch.is_some_and(|i| self.activate_profile(i, &reason));
        switched || before.as_deref() != self.auto_switch.foreground()
    }

    /// The program in the foreground as auto-switching last saw it.
    pub fn foreground(&self) -> Option<&str> {
        self.auto_switch.foreground()
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
        self.activate_profile(self.setup.profiles.len() - 1, "profile created");
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
        self.activate_profile(self.setup.profiles.len() - 1, "profile duplicated");
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

/// What is held right now (not persisted): the inputs holding each momentary control, and
/// the mode shifts in effect.
#[derive(Debug, Default)]
struct Runtime {
    held: HashMap<String, Vec<HoldKey>>,
    shifts: HashMap<String, Shift>,
}

/// A mode shifted while inputs are held: the state to go back to, and the held inputs with
/// the state each one shifts to (the latest wins).
#[derive(Debug)]
struct Shift {
    base: usize,
    held: Vec<(HoldKey, usize)>,
}

/// How far (percent of its range) an axis must move in learn mode to be captured.
const LEARN_AXIS_MOVE: f32 = 25.0;
/// A learned axis zone reaches this far (percent) to each side of where the axis is.
const LEARN_ZONE_HALF: f32 = 10.0;

/// Short name of a command for the log, e.g. "next state".
fn command_text(control: &VirtualControl, command: Command) -> String {
    let state = |name: Option<&str>| format!("\"{}\"", name.unwrap_or("?"));
    match (command, &control.kind) {
        (Command::Radio(c), VirtualControlKind::Radio(r)) => match c {
            RadioCommand::Increment => "next state".into(),
            RadioCommand::Decrement => "previous state".into(),
            RadioCommand::Select(i) => {
                format!("select {}", state(r.states.get(i).map(|s| s.name.as_str())))
            }
            RadioCommand::Clear => "clear".into(),
            RadioCommand::ToggleOff => "toggle off".into(),
        },
        (Command::Mode(c), VirtualControlKind::Mode(m)) => match c {
            ModeCommand::Next => "next state".into(),
            ModeCommand::Previous => "previous state".into(),
            ModeCommand::Select(i) => {
                format!("select {}", state(m.states.get(i).map(|s| s.name.as_str())))
            }
            ModeCommand::Shift(i) => format!(
                "shift to {}",
                state(m.states.get(i).map(|s| s.name.as_str()))
            ),
        },
        (Command::Toggle(c), _) => match c {
            ToggleCommand::Toggle => "toggle".into(),
            ToggleCommand::On => "on".into(),
            ToggleCommand::Off => "off".into(),
        },
        (Command::Counter(c), _) => match c {
            CounterCommand::Increment => "increment".into(),
            CounterCommand::Decrement => "decrement".into(),
            CounterCommand::Reset => "reset".into(),
        },
        (Command::Momentary, _) => "press".into(),
        _ => "?".into(),
    }
}

/// The control's state as the log shows it: a state name, on/off, a value.
fn state_text(control: &VirtualControl, memory: &RadioMemory, runtime: &Runtime) -> Option<String> {
    match &control.kind {
        VirtualControlKind::Radio(r) => memory
            .active
            .as_deref()
            .and_then(|id| r.index_of(id))
            .map(|i| r.states[i].name.clone()),
        VirtualControlKind::Mode(m) => m.active(memory).map(|i| m.states[i].name.clone()),
        VirtualControlKind::Toggle(_) => {
            Some(if Toggle::is_on(memory) { "on" } else { "off" }.into())
        }
        VirtualControlKind::Momentary(m) => {
            let held = runtime.held.get(&control.id).is_some_and(|k| !k.is_empty());
            Some(if m.active(held) { "on" } else { "off" }.into())
        }
        VirtualControlKind::Counter(c) => Some(c.spec.format(c.value(memory))),
    }
}

/// Applies `command` to a control: updates its memory (and what is held) and queues the
/// actions. Returns whether its state changed. `hold` is the input a held command is tied
/// to. `log` is the event log with what caused the command, if recording.
fn run_command(
    control: &VirtualControl,
    command: Command,
    hold: Option<&HoldKey>,
    memories: &mut HashMap<String, RadioMemory>,
    runtime: &mut Runtime,
    actions: &mut Vec<ActionRequest>,
    log: Option<(&Diagnostics, &str)>,
) -> bool {
    let id = control.id.as_str();
    let name = control.name.as_str();
    let memory = memories.entry(id.to_owned()).or_default();
    let before = log.map(|_| state_text(control, memory, runtime));
    let queued = actions.len();
    let emitted = match (command, &control.kind) {
        (Command::Radio(c), VirtualControlKind::Radio(r)) => r.apply(name, memory, c),
        (Command::Toggle(c), VirtualControlKind::Toggle(t)) => t.apply(name, memory, c),
        (Command::Counter(c), VirtualControlKind::Counter(k)) => k.apply(name, memory, c),
        (Command::Momentary, VirtualControlKind::Momentary(m)) => {
            let held = runtime.held.entry(id.to_owned()).or_default();
            match hold {
                Some(key) if !held.contains(key) => {
                    let was = m.active(!held.is_empty());
                    held.push(key.clone());
                    let now = m.active(true);
                    (was != now).then(|| m.transition_actions(name, was, now))
                }
                _ => None,
            }
        }
        (Command::Mode(ModeCommand::Shift(i)), VirtualControlKind::Mode(m)) => {
            let current = m.active(memory).unwrap_or(0);
            let shift = runtime.shifts.entry(id.to_owned()).or_insert(Shift {
                base: current,
                held: Vec::new(),
            });
            match hold {
                Some(key) if !shift.held.iter().any(|(k, _)| k == key) => {
                    shift.held.push((key.clone(), i));
                    m.go(name, memory, i)
                }
                _ => None,
            }
        }
        (Command::Mode(c), VirtualControlKind::Mode(m)) => {
            // A latching change ends any shift in effect: releasing goes nowhere.
            runtime.shifts.remove(id);
            let current = m.active(memory).unwrap_or(0);
            m.go(name, memory, m.next(current, c))
        }
        _ => None,
    };
    let changed = emitted.is_some();
    if let Some(e) = emitted {
        actions.extend(e);
    }
    if let Some((log, cause)) = log {
        log_command(
            log,
            control,
            &command_text(control, command),
            cause,
            changed,
            (
                before.flatten(),
                state_text(
                    control,
                    memories.get(id).unwrap_or(&RadioMemory::default()),
                    runtime,
                ),
            ),
            actions.len() - queued,
        );
    }
    changed
}

/// The input `key` was released: ends the control's holds tied to it.
fn release_hold(
    control: &VirtualControl,
    key: &HoldKey,
    memories: &mut HashMap<String, RadioMemory>,
    runtime: &mut Runtime,
    actions: &mut Vec<ActionRequest>,
    log: Option<&Diagnostics>,
) -> bool {
    let id = control.id.as_str();
    let name = control.name.as_str();
    let memory = memories.entry(id.to_owned()).or_default();
    let before = log.map(|_| state_text(control, memory, runtime));
    let queued = actions.len();
    let emitted = match &control.kind {
        VirtualControlKind::Momentary(m) => {
            let held = runtime.held.entry(id.to_owned()).or_default();
            let was = m.active(!held.is_empty());
            held.retain(|k| k != key);
            let now = m.active(!held.is_empty());
            if held.is_empty() {
                runtime.held.remove(id);
            }
            (was != now).then(|| m.transition_actions(name, was, now))
        }
        VirtualControlKind::Mode(m) => {
            let Some(shift) = runtime.shifts.get_mut(id) else {
                return false;
            };
            shift.held.retain(|(k, _)| k != key);
            let to = shift.held.last().map_or(shift.base, |&(_, i)| i);
            if shift.held.is_empty() {
                runtime.shifts.remove(id);
            }
            m.go(name, memory, to)
        }
        _ => None,
    };
    let changed = emitted.is_some();
    if let Some(e) = emitted {
        actions.extend(e);
    }
    if let Some(log) = log
        && changed
    {
        log_command(
            log,
            control,
            "release",
            "input released",
            true,
            (
                before.flatten(),
                state_text(
                    control,
                    memories.get(id).unwrap_or(&RadioMemory::default()),
                    runtime,
                ),
            ),
            actions.len() - queued,
        );
    }
    changed
}

fn log_command(
    log: &Diagnostics,
    control: &VirtualControl,
    role: &str,
    cause: &str,
    changed: bool,
    (before, after): (Option<String>, Option<String>),
    queued: usize,
) {
    log.record(
        Record::new(
            Category::Trigger,
            format!("{cause} → {}: {role}", control.name),
        )
        .detail(if changed {
            "state changed".to_owned()
        } else {
            "no change (already in that state or at the end)".to_owned()
        }),
    );
    if changed {
        log.record(
            Record::new(
                Category::Control,
                diagnostics::state_change_text(&control.name, before.as_deref(), after.as_deref()),
            )
            .detail(format!("{role}; {queued} action(s) queued")),
        );
    }
}

fn device_name(live: &LiveState, id: &DeviceId) -> String {
    live.device(id).map_or_else(
        || id.0.clone(),
        |d| d.label.clone().unwrap_or_else(|| d.name.clone()),
    )
}

fn with_device(live: &LiveState, id: &DeviceId, record: Record) -> Record {
    record.device(id.0.clone(), device_name(live, id))
}

fn hat_word(direction: HatDirection) -> &'static str {
    match direction {
        HatDirection::Up => "up",
        HatDirection::UpRight => "up-right",
        HatDirection::Right => "right",
        HatDirection::DownRight => "down-right",
        HatDirection::Down => "down",
        HatDirection::DownLeft => "down-left",
        HatDirection::Left => "left",
        HatDirection::UpLeft => "up-left",
    }
}

fn encoder_name<'a>(setup: &'a Setup, encoder: &'a str) -> &'a str {
    setup
        .encoders
        .iter()
        .find(|e| e.id == encoder)
        .map_or(encoder, |e| e.name.as_str())
}

fn step_word(direction: StepDirection) -> &'static str {
    if direction == StepDirection::Increment {
        "+"
    } else {
        "−"
    }
}

/// E.g. "Demo Stick · Button 4", "Radio knob +", "Demo Stick · Hat 1 up",
/// "Demo Throttle · Axis 3 80–100 %".
fn input_text(live: &LiveState, setup: &Setup, input: &TriggerInput) -> String {
    match input {
        TriggerInput::Button { device, index } => format!(
            "{} · Button {}",
            device_name(live, device),
            u32::from(*index) + 1
        ),
        TriggerInput::Hat {
            device,
            index,
            direction,
        } => format!(
            "{} · Hat {} {}",
            device_name(live, device),
            u32::from(*index) + 1,
            hat_word(*direction)
        ),
        TriggerInput::EncoderStep { encoder, direction } => {
            format!("{} {}", encoder_name(setup, encoder), step_word(*direction))
        }
        TriggerInput::AxisZone {
            device,
            index,
            min,
            max,
        } => format!(
            "{} · Axis {} {min}–{max} %",
            device_name(live, device),
            u32::from(*index) + 1
        ),
    }
}

/// A trigger with its press kind and modifiers, e.g. "Shift + Demo Stick · Button 4 (long
/// 500 ms)".
fn trigger_text(live: &LiveState, setup: &Setup, trigger: &Trigger) -> String {
    let mut text = String::new();
    for m in &trigger.modifiers {
        text.push_str(&format!(
            "{} · Button {} + ",
            device_name(live, &m.device),
            u32::from(m.index) + 1
        ));
    }
    text.push_str(&input_text(live, setup, &trigger.input));
    let zone = matches!(trigger.input, TriggerInput::AxisZone { .. });
    match trigger.press {
        PressKind::Press => {}
        PressKind::Release if zone => text.push_str(" (leaving)"),
        PressKind::Release => text.push_str(" (release)"),
        PressKind::Long { ms } => text.push_str(&format!(" (long {ms} ms)")),
        PressKind::Double { ms } => text.push_str(&format!(" (double {ms} ms)")),
    }
    text
}

/// What happened, for the log, e.g. "Demo Stick · Button 4 long press (500 ms)".
fn fired_text(live: &LiveState, setup: &Setup, fired: &Fired) -> String {
    match fired {
        Fired::Digital(key, event) => {
            let input = match key.clone() {
                DigitalKey::Button { device, index } => TriggerInput::Button { device, index },
                DigitalKey::Hat {
                    device,
                    index,
                    direction,
                } => TriggerInput::Hat {
                    device,
                    index,
                    direction,
                },
            };
            let base = input_text(live, setup, &input);
            match event {
                PressEvent::Down | PressEvent::Press => base,
                PressEvent::Up => format!("{base} released"),
                PressEvent::Long(ms) => format!("{base} long press ({ms} ms)"),
                PressEvent::Double(ms) => format!("{base} double press ({ms} ms)"),
            }
        }
        Fired::Encoder { encoder, direction } => {
            format!("{} {}", encoder_name(setup, encoder), step_word(*direction))
        }
        Fired::Zone { zone, entered } => format!(
            "{} · Axis {} {} {}–{} %",
            device_name(live, &zone.device),
            u32::from(zone.index) + 1,
            if *entered { "entered" } else { "left" },
            zone.min(),
            zone.max()
        ),
    }
}

fn connector_record(
    id: ConnectorId,
    before: Option<&ConnectorState>,
    now: &ConnectorState,
) -> Record {
    use crate::connection::ConnectorStatus as S;
    let status = |s: &S| match s {
        S::NotConfigured => "not configured".to_owned(),
        S::Connecting => "connecting".to_owned(),
        S::WaitingForApproval => "waiting for approval".to_owned(),
        S::Connected => "connected".to_owned(),
        S::Error { message } => format!("error: {message}"),
    };
    let name = id.name();
    let mut record = if before.is_none_or(|b| b.status != now.status) {
        let level = if matches!(now.status, S::Error { .. }) {
            Level::Error
        } else {
            Level::Info
        };
        Record::new(
            Category::Connector,
            format!("{name}: {}", status(&now.status)),
        )
        .level(level)
    } else if let Some(warning) = &now.warning
        && before.is_none_or(|b| b.warning != now.warning)
    {
        Record::new(
            Category::Connector,
            format!("{name}: problem running an action"),
        )
        .level(Level::Warn)
        .detail(warning.clone())
    } else if before.is_some_and(|b| b.warning.is_some()) && now.warning.is_none() {
        Record::new(Category::Connector, format!("{name}: problem resolved"))
    } else {
        Record::new(Category::Connector, format!("{name}: details changed"))
    };
    if record.detail.is_none() {
        let mut parts = Vec::new();
        if let Some(d) = &now.detail {
            parts.push(d.clone());
        }
        if let Some(v) = &now.voice {
            parts.push(v.describe());
        }
        if !parts.is_empty() {
            record = record.detail(parts.join("; "));
        }
    }
    record
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
        Trigger::from(TriggerInput::Button {
            device: dev(),
            index,
        })
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
            increment: vec![Trigger::from(TriggerInput::EncoderStep {
                encoder: "enc-1".into(),
                direction: StepDirection::Increment,
            })],
            decrement: vec![Trigger::from(TriggerInput::EncoderStep {
                encoder: "enc-1".into(),
                direction: StepDirection::Decrement,
            })],
            clear: vec![Trigger::from(TriggerInput::Hat {
                device: dev(),
                index: 0,
                direction: HatDirection::Up,
            })],
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
        setup.profile_mut().virtual_controls[0]
            .radio_mut()
            .unwrap()
            .startup = Startup::Empty;
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
            value: None,
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
            Some(Trigger::from(TriggerInput::EncoderStep {
                encoder: "enc-1".into(),
                direction: StepDirection::Increment
            }))
        );
        assert_eq!(active(&engine, &id).as_deref(), Some("B"));
        assert_eq!(keys(&mut engine), ["F3"]);
        engine.stop_learn();
        assert_eq!(engine.snapshot().learn, None);

        engine.start_learn();
        hat(&mut engine, HatState::LEFT | HatState::DOWN, 200);
        assert_eq!(
            engine.snapshot().learn.unwrap().captured,
            Some(Trigger::from(TriggerInput::Hat {
                device: dev(),
                index: 0,
                direction: HatDirection::DownLeft
            }))
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
        control.radio_mut().unwrap().states[0]
            .select
            .push(Trigger::from(TriggerInput::Button {
                device: vjoy_id,
                index: 3,
            }));
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
        control.radio_mut().unwrap().states.swap(0, 1);
        control.radio_mut().unwrap().states[0].name = "Bravo".into();
        engine.save_virtual_control(control.clone()).unwrap();
        assert_eq!(active(&engine, &id).as_deref(), Some("Bravo"));

        control.radio_mut().unwrap().states.remove(0);
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
        r.increment.push(Trigger::from(TriggerInput::EncoderStep {
            encoder: "enc-9".into(),
            direction: StepDirection::Increment,
        }));
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
        assert_eq!(saved.radio().unwrap().states[0].on_activate, [tap("F1")]);
        assert_eq!(saved.radio().unwrap().states[0].name, "State 1");
    }

    #[test]
    fn removing_encoder_removes_its_triggers() {
        let (mut engine, id) = engine_with_radio(|_| {});
        engine.remove_encoder("enc-1").unwrap();
        let radio = engine.setup().profile().virtual_controls[0]
            .radio()
            .unwrap()
            .clone();
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
                last: Some("st-2".into()),
                value: None,
            }
        );
        let mut restarted = restart(&engine, engine.setup().clone());
        assert_eq!(active(&restarted, &id), None);
        press(&mut restarted, 5, 0);
        assert_eq!(active(&restarted, &id).as_deref(), Some("B"));
        assert_eq!(keys(&mut restarted), ["F3"]);

        // Starting empty: the state active at shutdown is the one toggle off brings back.
        let mut setup = engine.setup().clone();
        setup.profile_mut().virtual_controls[0]
            .radio_mut()
            .unwrap()
            .startup = Startup::Empty;
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
        control.radio_mut().unwrap().states.remove(1);
        engine.save_virtual_control(control).unwrap();
        assert!(!engine.active_states().contains_key(&id));
        engine.take_actions();
        press(&mut engine, 5, 200);
        assert_eq!(active(&engine, &id).as_deref(), Some("A"));
    }

    // --- Event log -------------------------------------------------------------------------

    fn logged(engine: &mut Engine) -> Diagnostics {
        let log = Diagnostics::new(100);
        engine.set_diagnostics(log.clone());
        log
    }

    fn texts(log: &Diagnostics) -> Vec<String> {
        log.all().into_iter().map(|e| e.text).collect()
    }

    #[test]
    fn button_press_matches_trigger_and_changes_state() {
        let (mut engine, _) = engine_with_radio(|_| {});
        let log = logged(&mut engine);
        press(&mut engine, 0, 0);
        assert_eq!(
            texts(&log),
            [
                "Button 1 pressed",
                "Panel · Button 1 → Comms: select \"B\"",
                "Comms: (none) → B",
                "Button 1 released",
            ]
        );
        let entries = log.all();
        assert_eq!(entries[0].device_name.as_deref(), Some("Panel"));
        assert_eq!(entries[0].category, Category::Input);
        assert_eq!(entries[1].category, Category::Trigger);
        assert_eq!(entries[2].category, Category::Control);
        assert!(
            entries[2]
                .detail
                .as_deref()
                .unwrap()
                .contains("1 action(s) queued")
        );
    }

    #[test]
    fn encoder_detents_are_logged_with_direction() {
        let (mut engine, _) = engine_with_radio(|_| {});
        let log = logged(&mut engine);
        pulse(&mut engine, 11, 0);
        let t = texts(&log);
        assert!(
            t.contains(&"Encoder Radio knob: detent +".to_owned()),
            "{t:?}"
        );
        assert!(
            t.contains(&"Radio knob + → Comms: next state".to_owned()),
            "{t:?}"
        );
        pulse(&mut engine, 10, 100);
        assert!(texts(&log).contains(&"Encoder Radio knob: detent −".to_owned()));
    }

    #[test]
    fn matching_trigger_without_change_is_still_logged() {
        let (mut engine, _) = engine_with_radio(|_| {});
        press(&mut engine, 0, 0);
        let log = logged(&mut engine);
        press(&mut engine, 0, 100);
        let t = texts(&log);
        assert!(t.iter().any(|x| x.contains("select \"B\"")), "{t:?}");
        assert!(!t.iter().any(|x| x.contains("→ B")), "{t:?}");
    }

    #[test]
    fn hats_axes_and_learn_are_logged() {
        let (mut engine, _) = engine_with_radio(|_| {});
        let log = logged(&mut engine);
        engine.apply(BackendEvent::Hat {
            instance: 1,
            index: 0,
            state: HatState(HatState::UP),
            time_ns: 0,
        });
        engine.apply(BackendEvent::Axis {
            instance: 1,
            index: 2,
            value: 0.5,
            time_ns: 0,
        });
        // Below the throttle's change threshold: not logged.
        engine.apply(BackendEvent::Axis {
            instance: 1,
            index: 2,
            value: 0.51,
            time_ns: 500 * MS,
        });
        let t = texts(&log);
        assert!(t.contains(&"Hat 1: centered → up".to_owned()), "{t:?}");
        assert_eq!(
            t.iter().filter(|x| x.starts_with("Axis 3")).count(),
            1,
            "{t:?}"
        );

        engine.start_learn();
        press(&mut engine, 7, 1000);
        assert!(texts(&log).contains(&"Learn captured Panel · Button 8".to_owned()));
    }

    #[test]
    fn connector_changes_are_logged_once() {
        use crate::connection::ConnectorStatus;
        let (mut engine, _) = engine_with_radio(|_| {});
        let log = logged(&mut engine);
        let state = |status| ConnectorState {
            status,
            ..Default::default()
        };
        engine.set_connector_state(ConnectorId::Soundpad, state(ConnectorStatus::Connected));
        engine.set_connector_state(ConnectorId::Soundpad, state(ConnectorStatus::Connected));
        engine.set_connector_state(
            ConnectorId::Soundpad,
            state(ConnectorStatus::Error {
                message: "pipe closed".into(),
            }),
        );
        let entries = log.all();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].category, Category::Connector);
        assert_eq!(entries[1].level, Level::Error);
        assert!(entries[1].text.contains("pipe closed"));
    }

    #[test]
    fn profile_switches_and_elevated_foreground_are_logged() {
        let (mut engine, ..) = engine_with_two_profiles();
        let log = logged(&mut engine);
        engine.foreground_changed(Some("DCS.exe"));
        engine.select_profile(DEFAULT_PROFILE_ID).unwrap();
        let entries = log.all();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].text, "Profile: Default → DCS");
        assert_eq!(
            entries[0].detail.as_deref(),
            Some("foreground program DCS.exe")
        );
        assert_eq!(entries[1].text, "Profile: DCS → Default");
        assert_eq!(entries[1].detail.as_deref(), Some("selected manually"));

        let shell = |name: Option<&str>| ShellStatus {
            elevated_foreground: name.map(Into::into),
            ..engine_shell(&engine)
        };
        log.clear();
        let warn = shell(Some("DCS.exe"));
        engine.set_shell_status(warn);
        let entries = log.all();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].level, Level::Warn);
        assert!(entries[0].text.starts_with("DCS.exe runs as administrator"));
        // Unrelated status changes (autostart) log nothing.
        log.clear();
        let mut other = engine_shell(&engine);
        other.autostart_note = Some("x".into());
        engine.set_shell_status(other);
        assert!(log.all().is_empty());
    }

    fn engine_shell(engine: &Engine) -> ShellStatus {
        engine.shell_status().clone()
    }

    #[test]
    fn nothing_is_recorded_without_a_sink() {
        let (mut engine, _) = engine_with_radio(|_| {});
        press(&mut engine, 0, 0);
        // No panic and no sink: nothing to assert beyond the state change.
        assert_eq!(engine.take_actions().len(), 1);
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
            dcs.virtual_controls[0].radio().unwrap(),
            default.virtual_controls[0].radio().unwrap()
        );
        // The copy's overlay shows the copy's radio.
        assert!(dcs.overlay_widgets.has_virtual_control(&dcs_radio));
        assert!(!dcs.overlay_widgets.has_virtual_control(&default_radio));
        // Editing the copy leaves the original alone.
        engine.select_profile(&profile_id(&engine, 1)).unwrap();
        let mut copy = engine.setup().profile().virtual_controls[0].clone();
        copy.name = "Comms DCS".into();
        copy.radio_mut().unwrap().states.pop();
        engine.save_virtual_control(copy).unwrap();
        let setup = engine.setup();
        assert_eq!(setup.profiles[0].virtual_controls[0].name, "Comms");
        assert_eq!(
            setup.profiles[0].virtual_controls[0]
                .radio()
                .unwrap()
                .states
                .len(),
            3
        );
        assert_eq!(
            setup.profiles[1].virtual_controls[0]
                .radio()
                .unwrap()
                .states
                .len(),
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
        control.radio_mut().unwrap().states[1].on_activate = vec![
            tap("F3"),
            switch_to(ProfileTarget::Profile {
                profile: dcs.clone(),
            }),
        ];
        engine.save_virtual_control(control).unwrap();
        // In DCS, state B goes to the next profile (wraps to the default profile).
        engine.select_profile(&dcs).unwrap();
        let mut control = engine.setup().profile().virtual_controls[0].clone();
        control.radio_mut().unwrap().states[1].on_activate = vec![switch_to(ProfileTarget::Next)];
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
        control.radio_mut().unwrap().states[0].on_activate =
            vec![switch_to(ProfileTarget::Previous)];
        engine.save_virtual_control(control).unwrap();
        engine.set_radio_active(&default_radio, None).unwrap();
        engine
            .command_radio(&default_radio, RadioCommand::Increment)
            .unwrap();
        assert_eq!(active_profile_name(&engine), "DCS");
        // A target that does not exist is refused when saving.
        let mut control = engine.setup().profile().virtual_controls[0].clone();
        control.radio_mut().unwrap().states[0].on_activate =
            vec![switch_to(ProfileTarget::Profile {
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
                .all(|c| c.radio().unwrap().increment.is_empty()
                    && c.radio().unwrap().decrement.is_empty())
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

    // --- Settings and shell status ----------------------------------------------------------

    #[test]
    fn settings_are_saved_but_the_tray_notice_flag_stays_the_apps() {
        let mut engine = Engine::new(
            DeviceRegistry::default(),
            Setup::default(),
            &BTreeMap::new(),
        );
        engine.mark_tray_notice_shown();
        assert!(engine.take_dirty().setup);
        engine.mark_tray_notice_shown();
        assert!(!engine.take_dirty().setup);
        engine.set_settings(AppSettings {
            start_with_windows: true,
            tray_notice_shown: false,
            ..AppSettings::default()
        });
        assert!(engine.take_dirty().setup);
        let snapshot = engine.snapshot();
        assert!(snapshot.settings.start_with_windows);
        assert!(snapshot.settings.tray_notice_shown);
    }

    #[test]
    fn shell_status_reports_changes_only() {
        let mut engine = Engine::new(
            DeviceRegistry::default(),
            Setup::default(),
            &BTreeMap::new(),
        );
        let status = ShellStatus {
            elevated: false,
            elevated_foreground: Some("game.exe".into()),
            ..ShellStatus::default()
        };
        assert!(engine.set_shell_status(status.clone()));
        assert!(!engine.set_shell_status(status));
        assert!(!engine.take_dirty().setup);
        let json = serde_json::to_value(engine.snapshot()).unwrap();
        assert_eq!(json["shell"]["elevatedForeground"], "game.exe");
        assert_eq!(json["shell"]["autostart"], "off");
        assert_eq!(json["settings"]["closeToTray"], true);
    }
}

/// Toggle, momentary, counter and mode controls; press kinds, modifiers, conditions, axis
/// zones and timers.
#[cfg(test)]
mod control_tests {
    use super::*;
    use crate::action::KeyStep;
    use crate::device::Fingerprint;
    use crate::event::RawDevice;
    use crate::function::{CounterSpec, EndBehavior};
    use crate::state::HatState;
    use crate::virtual_control::{CounterControl, Mode, ModeState, Momentary, Toggle};

    const MS: u64 = 1_000_000;

    fn dev() -> DeviceId {
        DeviceId("3344:0001#1".into())
    }

    /// A stick with 32 buttons, a hat and 4 axes, plus encoder `enc-1` on buttons 10/11.
    fn engine() -> Engine {
        let mut engine = Engine::new(
            DeviceRegistry::default(),
            Setup::default(),
            &BTreeMap::new(),
        );
        engine.apply(BackendEvent::Initial(vec![RawDevice {
            instance: 1,
            fingerprint: Fingerprint {
                vendor_id: 0x3344,
                product_id: 0x0001,
                serial: Some("S1".into()),
                path: None,
                name: "Stick".into(),
            },
            backend_guid: "g".into(),
            axes: vec![-1.0; 4],
            buttons: vec![false; 32],
            hats: vec![HatState::CENTERED],
        }]));
        engine.add_encoder("Knob".into(), dev(), 10, 11).unwrap();
        engine
    }

    fn key(name: &str) -> Action {
        Action::keyboard(vec![KeyStep::KeyTap {
            key: name.into(),
            hold_ms: None,
        }])
    }

    fn button(index: u16) -> Trigger {
        TriggerInput::Button {
            device: dev(),
            index,
        }
        .into()
    }

    fn with(mut t: Trigger, f: impl FnOnce(&mut Trigger)) -> Trigger {
        f(&mut t);
        t
    }

    fn hat_up() -> Trigger {
        TriggerInput::Hat {
            device: dev(),
            index: 0,
            direction: HatDirection::Up,
        }
        .into()
    }

    fn encoder(direction: StepDirection) -> Trigger {
        TriggerInput::EncoderStep {
            encoder: "enc-1".into(),
            direction,
        }
        .into()
    }

    fn zone(index: u16, min: f32, max: f32) -> Trigger {
        TriggerInput::AxisZone {
            device: dev(),
            index,
            min,
            max,
        }
        .into()
    }

    fn add(engine: &mut Engine, name: &str, kind: VirtualControlKind) -> String {
        engine
            .save_virtual_control(VirtualControl {
                id: String::new(),
                name: name.into(),
                kind,
            })
            .unwrap()
    }

    fn down(engine: &mut Engine, index: u16, ms: u64) {
        engine.apply(BackendEvent::Button {
            instance: 1,
            index,
            pressed: true,
            time_ns: ms * MS,
        });
    }

    fn up(engine: &mut Engine, index: u16, ms: u64) {
        engine.apply(BackendEvent::Button {
            instance: 1,
            index,
            pressed: false,
            time_ns: ms * MS,
        });
    }

    fn tap_button(engine: &mut Engine, index: u16, ms: u64) {
        down(engine, index, ms);
        up(engine, index, ms + 30);
    }

    fn hat(engine: &mut Engine, mask: u8, ms: u64) {
        engine.apply(BackendEvent::Hat {
            instance: 1,
            index: 0,
            state: HatState(mask),
            time_ns: ms * MS,
        });
    }

    /// Moves axis `index` to `percent` of its range.
    fn axis(engine: &mut Engine, index: u16, percent: f32, ms: u64) {
        engine.apply(BackendEvent::Axis {
            instance: 1,
            index,
            value: percent / 50.0 - 1.0,
            time_ns: ms * MS,
        });
    }

    /// Keys tapped by the queued actions, in order.
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

    fn view(engine: &Engine, id: &str) -> VirtualControlView {
        engine
            .snapshot()
            .virtual_controls
            .into_iter()
            .find(|c| c.control.id == id)
            .unwrap()
    }

    fn on(engine: &Engine, id: &str) -> bool {
        view(engine, id).on
    }

    fn toggle_on_off() -> Toggle {
        Toggle {
            on_on: vec![key("A")],
            on_off: vec![key("B")],
            ..Default::default()
        }
    }

    fn mode_states(names: &[&str]) -> Vec<ModeState> {
        names
            .iter()
            .map(|n| ModeState {
                name: (*n).into(),
                on_activate: vec![key(&format!("F{}", n.len()))],
                ..Default::default()
            })
            .collect()
    }

    fn mode_state_id(engine: &Engine, mode: &str, index: usize) -> String {
        let (p, c) = engine.control_position(mode).unwrap();
        engine.setup().profiles[p].virtual_controls[c]
            .mode()
            .unwrap()
            .states[index]
            .id
            .clone()
    }

    #[test]
    fn toggle_switches_with_its_three_trigger_kinds_and_restores() {
        let mut e = engine();
        let id = add(
            &mut e,
            "Lights",
            VirtualControlKind::Toggle(Toggle {
                toggle: vec![button(0)],
                on: vec![button(1)],
                off: vec![button(2)],
                ..toggle_on_off()
            }),
        );
        assert!(!on(&e, &id));
        tap_button(&mut e, 0, 0);
        assert!(on(&e, &id));
        assert_eq!(keys(&mut e), ["A"]);
        tap_button(&mut e, 1, 100);
        assert!(keys(&mut e).is_empty(), "already on");
        tap_button(&mut e, 0, 200);
        assert_eq!(keys(&mut e), ["B"]);
        tap_button(&mut e, 1, 300);
        assert_eq!(keys(&mut e), ["A"]);
        // Restored at a restart, silently; with startup "empty" it starts off.
        let saved = e.active_states();
        let mut again = Engine::new(e.registry().clone(), e.setup().clone(), &BTreeMap::new());
        again.restore_active_states(&saved);
        assert!(on(&again, &id) && again.take_actions().is_empty());
        let mut setup = e.setup().clone();
        let VirtualControlKind::Toggle(t) = &mut setup.profile_mut().virtual_controls[0].kind
        else {
            unreachable!()
        };
        t.startup = Startup::Empty;
        let mut again = Engine::new(e.registry().clone(), setup, &BTreeMap::new());
        again.restore_active_states(&saved);
        assert!(!on(&again, &id));
        // Set silently from the UI.
        e.set_radio_active(&id, None).unwrap();
        assert!(!on(&e, &id) && e.take_actions().is_empty());
        e.command_control(&id, ManualCommand::Toggle).unwrap();
        assert_eq!(keys(&mut e), ["A"]);
    }

    #[test]
    fn momentary_is_on_while_held_by_button_hat_or_zone() {
        let mut e = engine();
        let id = add(
            &mut e,
            "PTT",
            VirtualControlKind::Momentary(Momentary {
                triggers: vec![button(0), hat_up(), zone(2, 90.0, 100.0)],
                on_press: vec![key("P")],
                on_release: vec![key("R")],
                invert: false,
            }),
        );
        down(&mut e, 0, 0);
        assert!(on(&e, &id));
        assert_eq!(keys(&mut e), ["P"]);
        // A second trigger held as well: still on, no actions.
        hat(&mut e, HatState::UP, 10);
        up(&mut e, 0, 20);
        assert!(on(&e, &id) && keys(&mut e).is_empty());
        hat(&mut e, 0, 30);
        assert!(!on(&e, &id));
        assert_eq!(keys(&mut e), ["R"]);
        // Axis zone: the first value only sets where the axis is.
        axis(&mut e, 2, 10.0, 40);
        axis(&mut e, 2, 95.0, 50);
        assert!(on(&e, &id));
        axis(&mut e, 2, 88.5, 60); // within the hysteresis
        assert!(on(&e, &id));
        axis(&mut e, 2, 80.0, 70);
        assert!(!on(&e, &id));
        assert_eq!(keys(&mut e), ["P", "R"]);
    }

    #[test]
    fn inverted_momentary_and_releases_after_a_profile_switch() {
        let mut e = engine();
        let id = add(
            &mut e,
            "Gear",
            VirtualControlKind::Momentary(Momentary {
                triggers: vec![button(0)],
                on_press: vec![key("P")],
                on_release: vec![key("R")],
                invert: true,
            }),
        );
        assert!(on(&e, &id));
        down(&mut e, 0, 0);
        assert!(!on(&e, &id));
        assert_eq!(keys(&mut e), ["R"]);
        up(&mut e, 0, 10);
        assert_eq!(keys(&mut e), ["P"]);
        // Held, then the profile changes: the release still lets go.
        let other = e.create_profile("Other");
        down(&mut e, 0, 20);
        e.select_profile(crate::profile::DEFAULT_PROFILE_ID)
            .unwrap();
        down(&mut e, 0, 30); // nothing new
        e.select_profile(&other).unwrap();
        up(&mut e, 0, 40);
        e.select_profile(crate::profile::DEFAULT_PROFILE_ID)
            .unwrap();
        assert!(on(&e, &id));
    }

    #[test]
    fn counter_control_steps_from_encoder_button_and_hat() {
        let mut e = engine();
        let id = add(
            &mut e,
            "COM1",
            VirtualControlKind::Counter(CounterControl {
                spec: CounterSpec {
                    min: 118.0,
                    max: 118.1,
                    step: 0.025,
                    initial: 118.0,
                    end: EndBehavior::Clamp,
                    decimals: 3,
                },
                increment: vec![encoder(StepDirection::Increment), button(1)],
                decrement: vec![encoder(StepDirection::Decrement)],
                reset: vec![hat_up()],
                on_change: vec![key("C")],
                startup: Startup::RestoreLast,
            }),
        );
        assert_eq!(view(&e, &id).display.as_deref(), Some("118.000"));
        tap_button(&mut e, 11, 0);
        tap_button(&mut e, 1, 100);
        let queued = e.take_actions();
        assert_eq!(queued.len(), 2);
        assert_eq!(queued[1].value.as_deref(), Some("118.050"));
        assert_eq!(view(&e, &id).value, Some(118.05));
        for i in 0..10 {
            tap_button(&mut e, 1, 200 + i * 50);
        }
        assert_eq!(view(&e, &id).display.as_deref(), Some("118.100"));
        e.take_actions();
        tap_button(&mut e, 10, 900);
        assert_eq!(view(&e, &id).display.as_deref(), Some("118.075"));
        // Persisted and restored.
        assert_eq!(e.active_states()[&id].value, Some(118.075));
        hat(&mut e, HatState::UP, 1000);
        assert_eq!(view(&e, &id).display.as_deref(), Some("118.000"));
        e.set_control_value(&id, 118.05).unwrap();
        assert_eq!(view(&e, &id).value, Some(118.05));
        e.command_control(&id, ManualCommand::Reset).unwrap();
        assert_eq!(view(&e, &id).value, Some(118.0));
        assert_eq!(
            e.command_control(&id, ManualCommand::Toggle),
            Err(EngineError::NotForControl)
        );
    }

    #[test]
    fn mode_select_next_and_shift_with_return() {
        let mut e = engine();
        let id = add(
            &mut e,
            "Layer",
            VirtualControlKind::Mode(Mode {
                states: mode_states(&["Nav", "Combat", "Landing"]),
                next: vec![button(5)],
                ..Default::default()
            }),
        );
        let (p, c) = e.control_position(&id).unwrap();
        let mut control = e.setup().profiles[p].virtual_controls[c].clone();
        let VirtualControlKind::Mode(m) = &mut control.kind else {
            unreachable!()
        };
        m.states[2].select.push(button(6));
        m.states[1].shift.push(button(7));
        m.states[2].shift.push(button(8));
        e.save_virtual_control(control).unwrap();

        // The first state by default.
        assert_eq!(view(&e, &id).active_name.as_deref(), Some("Nav"));
        tap_button(&mut e, 5, 0);
        assert_eq!(view(&e, &id).active_name.as_deref(), Some("Combat"));
        assert_eq!(keys(&mut e), ["F6"]);
        tap_button(&mut e, 6, 100);
        assert_eq!(view(&e, &id).active_name.as_deref(), Some("Landing"));
        tap_button(&mut e, 5, 200); // wraps
        assert_eq!(view(&e, &id).active_name.as_deref(), Some("Nav"));
        e.take_actions();
        // Shift: Combat while 7 is held, back to Nav on release.
        down(&mut e, 7, 300);
        assert_eq!(view(&e, &id).active_name.as_deref(), Some("Combat"));
        assert!(view(&e, &id).shifted);
        // A second shift on top, released in the other order.
        down(&mut e, 8, 310);
        assert_eq!(view(&e, &id).active_name.as_deref(), Some("Landing"));
        up(&mut e, 7, 320);
        assert_eq!(view(&e, &id).active_name.as_deref(), Some("Landing"));
        up(&mut e, 8, 330);
        assert_eq!(view(&e, &id).active_name.as_deref(), Some("Nav"));
        assert!(!view(&e, &id).shifted);
        // A latching change while shifted ends the shift: releasing goes nowhere.
        down(&mut e, 7, 400);
        tap_button(&mut e, 6, 410);
        up(&mut e, 7, 450);
        assert_eq!(view(&e, &id).active_name.as_deref(), Some("Landing"));
    }

    #[test]
    fn conditions_limit_triggers_to_mode_states() {
        let mut e = engine();
        let mode = add(
            &mut e,
            "Layer",
            VirtualControlKind::Mode(Mode {
                states: mode_states(&["Nav", "Combat"]),
                next: vec![button(5)],
                ..Default::default()
            }),
        );
        let gear = add(
            &mut e,
            "Gear",
            VirtualControlKind::Mode(Mode {
                states: mode_states(&["Up", "Down"]),
                next: vec![button(6)],
                ..Default::default()
            }),
        );
        let combat = mode_state_id(&e, &mode, 1);
        let gear_down = mode_state_id(&e, &gear, 1);
        let cond = |m: &str, s: &str| Condition {
            mode: m.into(),
            state: s.into(),
        };
        let toggle = add(
            &mut e,
            "Master arm",
            VirtualControlKind::Toggle(Toggle {
                toggle: vec![with(button(0), |t| {
                    t.conditions = vec![cond(&mode, &combat)];
                })],
                on: vec![with(button(1), |t| {
                    t.conditions = vec![cond(&mode, &combat), cond(&gear, &gear_down)];
                })],
                ..toggle_on_off()
            }),
        );
        tap_button(&mut e, 0, 0);
        assert!(!on(&e, &toggle), "Nav: no match");
        tap_button(&mut e, 5, 100); // Combat
        tap_button(&mut e, 0, 200);
        assert!(on(&e, &toggle));
        tap_button(&mut e, 0, 300);
        // Both conditions must hold.
        tap_button(&mut e, 1, 400);
        assert!(!on(&e, &toggle));
        tap_button(&mut e, 6, 500); // gear down
        tap_button(&mut e, 1, 600);
        assert!(on(&e, &toggle));
        // Conditions are checked when saving.
        let bad = VirtualControl {
            id: String::new(),
            name: "X".into(),
            kind: VirtualControlKind::Toggle(Toggle {
                toggle: vec![with(button(0), |t| {
                    t.conditions = vec![cond("vc-99", "st-1")]
                })],
                ..Default::default()
            }),
        };
        assert_eq!(
            e.save_virtual_control(bad.clone()),
            Err(EngineError::UnknownMode("vc-99".into()))
        );
        let mut bad = bad;
        let VirtualControlKind::Toggle(t) = &mut bad.kind else {
            unreachable!()
        };
        t.toggle[0].conditions = vec![cond(&mode, "st-99")];
        assert_eq!(
            e.save_virtual_control(bad),
            Err(EngineError::UnknownState("st-99".into()))
        );
    }

    #[test]
    fn long_press_short_press_and_deadlines() {
        let mut e = engine();
        let id = add(
            &mut e,
            "Radio",
            VirtualControlKind::Counter(CounterControl {
                spec: CounterSpec::default(),
                increment: vec![button(0)],
                reset: vec![with(button(0), |t| t.press = PressKind::Long { ms: 500 })],
                on_change: vec![key("C")],
                ..Default::default()
            }),
        );
        // Short: fires on release, not on press.
        down(&mut e, 0, 1000);
        assert_eq!(view(&e, &id).value, Some(1.0));
        assert_eq!(e.next_deadline(), Some(1500 * MS));
        up(&mut e, 0, 1200);
        assert_eq!(view(&e, &id).value, Some(2.0));
        assert_eq!(e.next_deadline(), None);
        // Long: fires at the deadline while still held; no short press on release.
        down(&mut e, 0, 2000);
        assert!(!e.advance(2499 * MS));
        assert!(e.advance(2500 * MS));
        assert_eq!(view(&e, &id).value, Some(1.0));
        up(&mut e, 0, 3000);
        assert_eq!(view(&e, &id).value, Some(1.0));
        assert_eq!(keys(&mut e).len(), 2);
        // Any later event handles a missed deadline first.
        e.set_control_value(&id, 7.0).unwrap();
        down(&mut e, 0, 4000);
        hat(&mut e, HatState::UP, 4600);
        assert_eq!(view(&e, &id).value, Some(1.0));
        up(&mut e, 0, 4700);
        assert_eq!(view(&e, &id).value, Some(1.0));
    }

    #[test]
    fn double_press_and_release() {
        let mut e = engine();
        let id = add(
            &mut e,
            "Lights",
            VirtualControlKind::Toggle(Toggle {
                toggle: vec![button(0)],
                on: vec![with(button(0), |t| t.press = PressKind::Double { ms: 300 })],
                off: vec![with(button(1), |t| t.press = PressKind::Release)],
                ..toggle_on_off()
            }),
        );
        // A single press toggles once the double press window has passed.
        tap_button(&mut e, 0, 0);
        assert!(!on(&e, &id));
        assert_eq!(e.next_deadline(), Some(300 * MS));
        e.advance(300 * MS);
        assert!(on(&e, &id));
        // A double press only switches on.
        tap_button(&mut e, 0, 1000);
        tap_button(&mut e, 0, 1200);
        assert!(on(&e, &id));
        e.advance(2000 * MS);
        assert!(on(&e, &id), "no short press after a double press");
        // Release kind: on release only.
        down(&mut e, 1, 3000);
        assert!(on(&e, &id));
        up(&mut e, 1, 3100);
        assert!(!on(&e, &id));
    }

    #[test]
    fn modifiers_most_specific_trigger_wins() {
        let mut e = engine();
        let shift = ButtonRef {
            device: dev(),
            index: 20,
        };
        let id = add(
            &mut e,
            "Ch",
            VirtualControlKind::Counter(CounterControl {
                spec: CounterSpec {
                    min: 0.0,
                    max: 100.0,
                    initial: 50.0,
                    ..Default::default()
                },
                increment: vec![button(0)],
                decrement: vec![with(button(0), |t| t.modifiers = vec![shift.clone()])],
                ..Default::default()
            }),
        );
        tap_button(&mut e, 0, 0);
        assert_eq!(view(&e, &id).value, Some(51.0));
        down(&mut e, 20, 100);
        tap_button(&mut e, 0, 150);
        assert_eq!(view(&e, &id).value, Some(50.0), "only the modifier trigger");
        up(&mut e, 20, 300);
        tap_button(&mut e, 0, 400);
        assert_eq!(view(&e, &id).value, Some(51.0));
    }

    #[test]
    fn zone_enter_and_leave_triggers() {
        let mut e = engine();
        let id = add(
            &mut e,
            "Afterburner",
            VirtualControlKind::Toggle(Toggle {
                on: vec![zone(1, 80.0, 100.0)],
                off: vec![with(zone(1, 80.0, 100.0), |t| t.press = PressKind::Release)],
                ..toggle_on_off()
            }),
        );
        axis(&mut e, 1, 50.0, 0);
        axis(&mut e, 1, 85.0, 10);
        assert!(on(&e, &id));
        assert_eq!(keys(&mut e), ["A"]);
        axis(&mut e, 1, 79.0, 20);
        axis(&mut e, 1, 81.0, 30);
        assert!(keys(&mut e).is_empty(), "jitter at the edge does not fire");
        axis(&mut e, 1, 60.0, 40);
        assert!(!on(&e, &id));
        assert_eq!(keys(&mut e), ["B"]);
    }

    #[test]
    fn learn_captures_axes_hats_and_encoders() {
        let mut e = engine();
        e.start_learn();
        axis(&mut e, 3, 10.0, 0); // within a quarter of where it was
        assert_eq!(e.snapshot().learn.unwrap().captured, None);
        axis(&mut e, 3, 95.0, 10);
        let captured = e.snapshot().learn.unwrap().captured.unwrap();
        assert_eq!(captured, zone(3, 85.0, 100.0));
        e.start_learn();
        tap_button(&mut e, 11, 100);
        assert_eq!(
            e.snapshot().learn.unwrap().captured,
            Some(encoder(StepDirection::Increment))
        );
        e.start_learn();
        hat(&mut e, HatState::UP, 200);
        assert_eq!(e.snapshot().learn.unwrap().captured, Some(hat_up()));
        e.stop_learn();
        assert!(e.take_actions().is_empty());
    }

    #[test]
    fn held_controls_refuse_encoders_and_releases() {
        let mut e = engine();
        let err = e.save_virtual_control(VirtualControl {
            id: String::new(),
            name: String::new(),
            kind: VirtualControlKind::Momentary(Momentary {
                triggers: vec![with(button(0), |t| t.press = PressKind::Release)],
                ..Default::default()
            }),
        });
        assert!(matches!(err, Err(EngineError::Control(_))));
        let id = add(
            &mut e,
            " ",
            VirtualControlKind::Momentary(Momentary::default()),
        );
        assert_eq!(view(&e, &id).control.name, "Momentary");
        // New controls of every kind go on the overlay.
        assert!(e.overlay().widgets.has_virtual_control(&id));
    }

    #[test]
    fn snapshot_json_for_new_kinds() {
        let mut e = engine();
        add(&mut e, "T", VirtualControlKind::Toggle(toggle_on_off()));
        let json = serde_json::to_value(e.snapshot()).unwrap();
        let c = &json["virtualControls"][0];
        assert_eq!(c["kind"]["type"], "toggle");
        assert_eq!(c["on"], false);
        assert!(c["value"].is_null() && c["display"].is_null());
        assert_eq!(c["shifted"], false);
    }
}

//! Ties everything together: live input, encoder detection and stateful functions.
//!
//! The engine is driven by backend events one at a time, so every encoder pulse is counted
//! when it happens, independent of how often the UI or overlay redraws.

use crate::device::DeviceId;
use crate::encoder::{
    DetectorConfig, EncoderBinding, EncoderDetector, EncoderSuggestion, PulseStats,
};
use crate::event::BackendEvent;
use crate::function::{CounterInput, Function, FunctionKind, SpecError};
use crate::overlay::{ControlRef, OverlayConfig};
use crate::registry::DeviceRegistry;
use crate::setup::{ButtonPair, Setup};
use crate::state::{LiveState, Snapshot};
use serde::Serialize;
use std::collections::{BTreeMap, HashMap, HashSet};
use thiserror::Error;

/// Which persisted parts changed since the last [`Engine::take_dirty`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Dirty {
    pub registry: bool,
    pub setup: bool,
    pub values: bool,
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
    #[error(transparent)]
    Spec(#[from] SpecError),
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
pub struct EngineSnapshot {
    #[serde(flatten)]
    pub live: Snapshot,
    pub encoders: Vec<EncoderView>,
    pub suggestions: Vec<EncoderSuggestion>,
    /// Buttons that pulse like an encoder but whose partner was not seen yet.
    pub half_detected: Vec<ButtonPair>,
    pub button_stats: Vec<ButtonStats>,
    pub functions: Vec<FunctionView>,
    pub overlay: OverlayConfig,
    pub detector: DetectorConfig,
}

#[derive(Debug)]
pub struct Engine {
    registry: DeviceRegistry,
    setup: Setup,
    live: LiveState,
    detector: EncoderDetector,
    positions: HashMap<String, i64>,
    steps: HashMap<(String, bool), u64>,
    dirty: Dirty,
}

impl Engine {
    /// `values` are the function values saved at the last run.
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
            dirty: Dirty::default(),
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
    pub fn apply(&mut self, event: BackendEvent) -> bool {
        let applied = self.live.apply(event, &mut self.registry);
        self.dirty.registry |= applied.devices_changed;
        if let Some(edge) = applied.button {
            self.detector
                .observe(&edge.device, edge.index, edge.pressed, edge.time_ns);
            if edge.pressed {
                self.step_encoders(&edge.device, edge.index);
            }
        }
        applied.state_changed
    }

    fn step_encoders(&mut self, device: &DeviceId, button: u16) {
        for encoder in &self.setup.encoders {
            let direction = encoder.direction(device, button);
            if direction == 0 {
                continue;
            }
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
    }

    /// Current function values, for persisting.
    pub fn values(&self) -> BTreeMap<String, f64> {
        self.setup
            .functions
            .iter()
            .map(|f| (f.id.clone(), self.value_of(f)))
            .collect()
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

    pub fn set_detector_config(&mut self, config: DetectorConfig) {
        self.setup.detector = config;
        self.detector = EncoderDetector::new(config);
        self.dirty.setup = true;
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
    use crate::device::Fingerprint;
    use crate::event::RawDevice;
    use crate::function::{CounterSpec, EndBehavior};
    use crate::state::HatState;

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
}

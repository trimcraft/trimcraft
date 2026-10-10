//! Live input state of all devices, keyed by stable [`DeviceId`].

use crate::device::{DeviceId, DeviceKind, Fingerprint, IdentityConfidence};
use crate::event::{BackendEvent, RawDevice};
use crate::overlay::ControlRef;
use crate::registry::DeviceRegistry;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};

/// Hat switch position as a bitmask of [`HatState::UP`], `RIGHT`, `DOWN`, `LEFT`.
/// Diagonals set two bits; zero is centred. Same layout as SDL and most HID devices.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct HatState(pub u8);

impl HatState {
    pub const CENTERED: Self = Self(0);
    pub const UP: u8 = 1;
    pub const RIGHT: u8 = 2;
    pub const DOWN: u8 = 4;
    pub const LEFT: u8 = 8;
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DeviceState {
    /// Normalised to -1.0..=1.0.
    pub axes: Vec<f32>,
    pub buttons: Vec<bool>,
    pub hats: Vec<HatState>,
}

impl DeviceState {
    fn from_raw(raw: &RawDevice) -> Self {
        Self {
            axes: raw.axes.clone(),
            buttons: raw.buttons.clone(),
            hats: raw.hats.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceSnapshot {
    pub id: DeviceId,
    pub name: String,
    pub label: Option<String>,
    pub kind: DeviceKind,
    pub fingerprint: Fingerprint,
    pub backend_guid: String,
    pub confidence: IdentityConfidence,
    pub connected: bool,
    pub state: DeviceState,
}

/// The most recently moved control, used to highlight "what did I just touch".
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Activity {
    pub device: DeviceId,
    pub control: ControlRef,
    pub time_ns: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub devices: Vec<DeviceSnapshot>,
    pub last_activity: Option<Activity>,
    pub backend_error: Option<String>,
}

/// What an applied event changed, so callers know what to persist or re-render.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Applied {
    /// The set of devices or their identities changed (registry must be saved).
    pub devices_changed: bool,
    /// A button edge, for encoder detection and counters.
    pub button: Option<ButtonEdge>,
    /// A hat change, for triggers.
    pub hat: Option<HatEdge>,
    pub state_changed: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ButtonEdge {
    pub device: DeviceId,
    pub index: u16,
    pub pressed: bool,
    pub time_ns: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HatEdge {
    pub device: DeviceId,
    pub index: u16,
    pub previous: HatState,
    pub state: HatState,
}

#[derive(Debug, Clone)]
struct LiveDevice {
    snapshot: DeviceSnapshot,
}

/// Axis moves smaller than this do not count as "activity" (sensor noise).
const ACTIVITY_AXIS_THRESHOLD: f32 = 0.05;

#[derive(Debug, Default)]
pub struct LiveState {
    devices: BTreeMap<DeviceId, LiveDevice>,
    instances: HashMap<u32, DeviceId>,
    last_activity: Option<Activity>,
    /// Axis value at the time it last counted as activity.
    activity_axis_ref: HashMap<(DeviceId, u16), f32>,
    backend_error: Option<String>,
}

impl LiveState {
    pub fn apply(&mut self, event: BackendEvent, registry: &mut DeviceRegistry) -> Applied {
        match event {
            BackendEvent::Initial(raws) => {
                let connected = self.connected_ids();
                let fps: Vec<Fingerprint> = raws.iter().map(|r| r.fingerprint.clone()).collect();
                let resolutions = registry.resolve_batch(&fps, &connected);
                for (raw, res) in raws.into_iter().zip(resolutions) {
                    self.insert(raw, res.id, res.confidence, registry);
                }
                Applied {
                    devices_changed: true,
                    state_changed: true,
                    ..Default::default()
                }
            }
            BackendEvent::Added(raw) => {
                let res = registry.resolve(&raw.fingerprint, &self.connected_ids());
                self.insert(raw, res.id, res.confidence, registry);
                Applied {
                    devices_changed: true,
                    state_changed: true,
                    ..Default::default()
                }
            }
            BackendEvent::Removed { instance } => {
                if let Some(id) = self.instances.remove(&instance)
                    && let Some(dev) = self.devices.get_mut(&id)
                {
                    dev.snapshot.connected = false;
                }
                Applied {
                    devices_changed: true,
                    state_changed: true,
                    ..Default::default()
                }
            }
            BackendEvent::Axis {
                instance,
                index,
                value,
                time_ns,
            } => {
                let Some((id, state)) = self.state_mut(instance) else {
                    return Applied::default();
                };
                if let Some(slot) = state.axes.get_mut(usize::from(index)) {
                    *slot = value;
                }
                let key = (id.clone(), index);
                let reference = *self.activity_axis_ref.entry(key.clone()).or_insert(0.0);
                if (value - reference).abs() >= ACTIVITY_AXIS_THRESHOLD {
                    self.activity_axis_ref.insert(key, value);
                    self.last_activity = Some(Activity {
                        device: id,
                        control: ControlRef::Axis { index },
                        time_ns,
                    });
                }
                Applied {
                    state_changed: true,
                    ..Default::default()
                }
            }
            BackendEvent::Button {
                instance,
                index,
                pressed,
                time_ns,
            } => {
                let Some((id, state)) = self.state_mut(instance) else {
                    return Applied::default();
                };
                if let Some(slot) = state.buttons.get_mut(usize::from(index)) {
                    *slot = pressed;
                }
                if pressed {
                    self.last_activity = Some(Activity {
                        device: id.clone(),
                        control: ControlRef::Button { index },
                        time_ns,
                    });
                }
                Applied {
                    state_changed: true,
                    button: Some(ButtonEdge {
                        device: id,
                        index,
                        pressed,
                        time_ns,
                    }),
                    ..Default::default()
                }
            }
            BackendEvent::Hat {
                instance,
                index,
                state: hat,
                time_ns,
            } => {
                let Some((id, state)) = self.state_mut(instance) else {
                    return Applied::default();
                };
                let previous = state
                    .hats
                    .get_mut(usize::from(index))
                    .map(|slot| std::mem::replace(slot, hat));
                if hat != HatState::CENTERED {
                    self.last_activity = Some(Activity {
                        device: id.clone(),
                        control: ControlRef::Hat { index },
                        time_ns,
                    });
                }
                Applied {
                    state_changed: true,
                    hat: previous.map(|previous| HatEdge {
                        device: id,
                        index,
                        previous,
                        state: hat,
                    }),
                    ..Default::default()
                }
            }
            BackendEvent::Failed(message) => {
                self.backend_error = Some(message);
                Applied {
                    state_changed: true,
                    ..Default::default()
                }
            }
        }
    }

    /// Refreshes user labels after the registry changed.
    pub fn sync_labels(&mut self, registry: &DeviceRegistry) {
        for (id, dev) in &mut self.devices {
            dev.snapshot.label = registry.get(id).and_then(|k| k.label.clone());
        }
    }

    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            devices: self.devices.values().map(|d| d.snapshot.clone()).collect(),
            last_activity: self.last_activity.clone(),
            backend_error: self.backend_error.clone(),
        }
    }

    pub fn device(&self, id: &DeviceId) -> Option<&DeviceSnapshot> {
        self.devices.get(id).map(|d| &d.snapshot)
    }

    fn connected_ids(&self) -> HashSet<DeviceId> {
        self.instances.values().cloned().collect()
    }

    fn state_mut(&mut self, instance: u32) -> Option<(DeviceId, &mut DeviceState)> {
        let id = self.instances.get(&instance)?.clone();
        let dev = self.devices.get_mut(&id)?;
        Some((id, &mut dev.snapshot.state))
    }

    fn insert(
        &mut self,
        raw: RawDevice,
        id: DeviceId,
        confidence: IdentityConfidence,
        registry: &DeviceRegistry,
    ) {
        self.instances.insert(raw.instance, id.clone());
        for (index, value) in raw.axes.iter().enumerate() {
            let index = u16::try_from(index).unwrap_or(u16::MAX);
            self.activity_axis_ref.insert((id.clone(), index), *value);
        }
        let snapshot = DeviceSnapshot {
            id: id.clone(),
            name: raw.fingerprint.name.clone(),
            label: registry.get(&id).and_then(|k| k.label.clone()),
            kind: raw.fingerprint.kind(),
            state: DeviceState::from_raw(&raw),
            fingerprint: raw.fingerprint,
            backend_guid: raw.backend_guid,
            confidence,
            connected: true,
        };
        self.devices.insert(id, LiveDevice { snapshot });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(instance: u32, path: &str) -> RawDevice {
        RawDevice {
            instance,
            fingerprint: Fingerprint {
                vendor_id: 0x044f,
                product_id: 0x0404,
                serial: None,
                path: Some(path.into()),
                name: "Throttle".into(),
            },
            backend_guid: "g".into(),
            axes: vec![0.0; 3],
            buttons: vec![false; 4],
            hats: vec![HatState::CENTERED],
        }
    }

    #[test]
    fn events_update_the_right_device() {
        let mut reg = DeviceRegistry::default();
        let mut live = LiveState::default();
        live.apply(
            BackendEvent::Initial(vec![raw(10, "a"), raw(11, "b")]),
            &mut reg,
        );
        live.apply(
            BackendEvent::Axis {
                instance: 11,
                index: 2,
                value: 0.5,
                time_ns: 1,
            },
            &mut reg,
        );
        let applied = live.apply(
            BackendEvent::Button {
                instance: 10,
                index: 1,
                pressed: true,
                time_ns: 2,
            },
            &mut reg,
        );
        assert_eq!(applied.button.unwrap().device.0, "044f:0404#1");

        let snap = live.snapshot();
        assert_eq!(snap.devices[0].state.buttons, [false, true, false, false]);
        assert_eq!(snap.devices[1].state.axes[2], 0.5);
        assert_eq!(
            snap.last_activity.unwrap().control,
            ControlRef::Button { index: 1 }
        );
    }

    #[test]
    fn removed_device_stays_listed_as_disconnected_and_returns_with_same_id() {
        let mut reg = DeviceRegistry::default();
        let mut live = LiveState::default();
        live.apply(BackendEvent::Initial(vec![raw(10, "a")]), &mut reg);
        live.apply(BackendEvent::Removed { instance: 10 }, &mut reg);
        assert!(!live.snapshot().devices[0].connected);

        live.apply(BackendEvent::Added(raw(42, "a")), &mut reg);
        let snap = live.snapshot();
        assert_eq!(snap.devices.len(), 1);
        assert!(snap.devices[0].connected);
        assert_eq!(snap.devices[0].confidence, IdentityConfidence::Path);
    }

    #[test]
    fn small_axis_noise_is_not_activity() {
        let mut reg = DeviceRegistry::default();
        let mut live = LiveState::default();
        live.apply(BackendEvent::Initial(vec![raw(10, "a")]), &mut reg);
        live.apply(
            BackendEvent::Axis {
                instance: 10,
                index: 0,
                value: 0.01,
                time_ns: 1,
            },
            &mut reg,
        );
        live.apply(
            BackendEvent::Axis {
                instance: 10,
                index: 0,
                value: 0.02,
                time_ns: 2,
            },
            &mut reg,
        );
        assert!(live.snapshot().last_activity.is_none());
        live.apply(
            BackendEvent::Axis {
                instance: 10,
                index: 0,
                value: 0.3,
                time_ns: 3,
            },
            &mut reg,
        );
        assert!(live.snapshot().last_activity.is_some());
    }

    #[test]
    fn events_for_unknown_instances_are_ignored() {
        let mut reg = DeviceRegistry::default();
        let mut live = LiveState::default();
        let applied = live.apply(
            BackendEvent::Axis {
                instance: 99,
                index: 0,
                value: 1.0,
                time_ns: 0,
            },
            &mut reg,
        );
        assert_eq!(applied, Applied::default());
    }
}

//! Events an input backend delivers. Backends know nothing about Detent's device ids; they
//! address devices by their own session-local `instance` number.

use crate::device::Fingerprint;
use crate::state::HatState;

/// A device as the backend sees it when it appears.
#[derive(Debug, Clone, PartialEq)]
pub struct RawDevice {
    /// Backend-local handle, valid until the device disappears.
    pub instance: u32,
    pub fingerprint: Fingerprint,
    /// Backend-specific type id (e.g. the SDL GUID), shown for diagnostics only.
    pub backend_guid: String,
    pub axes: Vec<f32>,
    pub buttons: Vec<bool>,
    pub hats: Vec<HatState>,
}

/// Timestamps are monotonic nanoseconds from the backend's own clock.
#[derive(Debug, Clone, PartialEq)]
pub enum BackendEvent {
    /// Devices present at startup, delivered together so identities resolve correctly.
    Initial(Vec<RawDevice>),
    Added(RawDevice),
    Removed {
        instance: u32,
    },
    Axis {
        instance: u32,
        index: u16,
        value: f32,
        time_ns: u64,
    },
    Button {
        instance: u32,
        index: u16,
        pressed: bool,
        time_ns: u64,
    },
    Hat {
        instance: u32,
        index: u16,
        state: HatState,
        time_ns: u64,
    },
    /// The backend failed and stopped; the message is shown to the user.
    Failed(String),
}

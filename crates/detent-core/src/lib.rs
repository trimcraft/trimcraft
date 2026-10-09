//! Platform-independent core of Detent.
//!
//! Nothing in this crate touches an operating system API, so all of it is unit-tested on
//! every platform. Input backends (see `detent-input`) translate OS events into
//! [`BackendEvent`]s; the [`Engine`] turns them into the state the UI and the overlay show.

pub mod device;
pub mod encoder;
pub mod engine;
pub mod event;
pub mod function;
pub mod overlay;
pub mod registry;
pub mod setup;
pub mod state;

pub use device::{DeviceId, DeviceKind, Fingerprint, IdentityConfidence, VirtualDriver};
pub use encoder::{DetectorConfig, EncoderBinding};
pub use engine::{Dirty, Engine, EngineError, EngineSnapshot};
pub use event::{BackendEvent, RawDevice};
pub use function::{CounterInput, CounterSpec, EndBehavior, Function, FunctionKind};
pub use overlay::{Anchor, ControlRef, OverlayConfig, OverlayWidget};
pub use registry::DeviceRegistry;
pub use setup::{ButtonPair, Setup};
pub use state::{DeviceSnapshot, DeviceState, HatState, LiveState, Snapshot};

//! Platform-independent core of Detent.
//!
//! Nothing in this crate touches an operating system API, so all of it is unit-tested on
//! every platform. Input backends (see `detent-input`) translate OS events into
//! [`BackendEvent`]s; the [`Engine`] turns them into the state the UI and the overlay show.

pub mod action;
pub mod connection;
pub mod device;
pub mod diagnostics;
pub mod discord;
pub mod encoder;
pub mod engine;
pub mod event;
pub mod function;
pub mod overlay;
pub mod path;
pub mod profile;
pub mod recording;
pub mod registry;
pub mod settings;
pub mod setup;
pub mod soundpad;
pub mod state;
pub mod trigger;
pub mod virtual_control;

pub use action::{
    Action, InputOp, KeyInfo, KeyStep, MouseButton, MouseStep, Movement, PathPoint, Playback,
    RelPoint,
};
pub use connection::{Connections, ConnectorId, ConnectorState, ConnectorStatus};
pub use device::{DeviceId, DeviceKind, Fingerprint, IdentityConfidence, VirtualDriver};
pub use diagnostics::{ActionEvent, Category, Diagnostics, Entry, Level, Record};
pub use discord::{DiscordCommand, VoiceState};
pub use encoder::{DetectorConfig, EncoderBinding};
pub use engine::{Dirty, Engine, EngineError, EngineSnapshot, ManualCommand};
pub use event::{BackendEvent, RawDevice};
pub use function::{CounterInput, CounterSpec, EndBehavior, Function, FunctionKind};
pub use overlay::{
    Anchor, ControlRef, MonitorRef, OverlayConfig, OverlayItem, OverlayWidget, RadioLayout,
    WidgetList, WidgetPosition,
};
pub use path::Screens;
pub use profile::{NoMatch, Profile, ProfileSwitching, ProfileTarget};
pub use registry::DeviceRegistry;
pub use settings::{AppSettings, AutostartKind, ShellStatus};
pub use setup::{ButtonPair, Setup};
pub use soundpad::{Sound, SoundOutput, SoundRef, SoundpadCommand};
pub use state::{DeviceSnapshot, DeviceState, HatState, LiveState, Snapshot};
pub use trigger::{ButtonRef, Condition, PressKind, TriggerInput};
pub use virtual_control::{
    ActionRequest, CounterControl, HatDirection, Mode, ModeState, Momentary, Phase, Radio,
    RadioCommand, RadioMemory, RadioState, Startup, StepDirection, Toggle, Trigger, VirtualControl,
    VirtualControlKind,
};

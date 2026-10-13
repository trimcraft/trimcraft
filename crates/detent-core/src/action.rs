//! Actions: what Detent does when a virtual control changes, e.g. press a key, play a
//! Soundpad sound or mute Discord.
//!
//! The core only describes actions and expands macros into low-level [`InputOp`]s. Executing
//! them (Windows `SendInput`, the Soundpad pipe) happens in the app, on its own thread, so the
//! engine never blocks and stays free of OS calls.
//!
//! Keys are stored by name (see [`KEYS`]) and sent as keyboard scan codes, not virtual-key
//! codes: games that read DirectInput or RawInput only see scan codes.
//!
//! Mouse macros can move the cursor to absolute positions, stored as fractions of a monitor
//! (see [`crate::path`]), or by relative amounts. Recorded movement is kept as paths of
//! waypoints with timestamps; playback interpolates between them.

use crate::discord::DiscordCommand;
use crate::overlay::MonitorRef;
use crate::path::{self, Sample, Screens};
use crate::profile::ProfileTarget;
use crate::soundpad::SoundpadCommand;
use crate::vjoy::{VjoyCommand, VjoyError};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Hold time of a key tap or mouse click when none is given. Long enough that games which
/// sample input once per frame still see the press.
pub const DEFAULT_HOLD_MS: u32 = 50;
/// Upper bound for one delay or hold (and the duration of one path), so a typo cannot stall
/// the action queue for hours.
pub const MAX_DELAY_MS: u32 = 60_000;
/// Upper bound on steps per macro. Recorded macros are long, hence generous.
pub const MAX_STEPS: usize = 2_000;
/// Upper bound on waypoints in one path.
pub const MAX_PATH_POINTS: usize = 10_000;
/// Playback speed limits, in percent of the recorded speed.
pub const MIN_SPEED_PERCENT: u32 = 10;
pub const MAX_SPEED_PERCENT: u32 = 1_000;
/// While playing a path, the cursor is updated at least this often (125 Hz).
pub const PATH_TICK_MS: f64 = 8.0;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "type"
)]
pub enum Action {
    KeyboardMacro {
        steps: Vec<KeyStep>,
        #[serde(default)]
        playback: Playback,
    },
    MouseMacro {
        steps: Vec<MouseStep>,
        #[serde(default)]
        playback: Playback,
        /// How this macro records movement. Steps keep their own kind either way.
        #[serde(default)]
        movement: Movement,
        /// The monitor absolute positions refer to; `None` is the primary monitor.
        #[serde(default)]
        monitor: Option<MonitorRef>,
    },
    /// A remote control command for Soundpad.
    Soundpad { command: SoundpadCommand },
    /// Mute or deafen in Discord.
    Discord { command: DiscordCommand },
    /// Makes another profile active. The engine handles it; it sends no input.
    SwitchProfile { target: ProfileTarget },
    /// Presses a vJoy button, sets an axis or a POV. The engine handles it (vJoy output is
    /// part of its state), so it takes effect when it fires, not after earlier macros.
    Vjoy { command: VjoyCommand },
}

/// How a mouse macro records movement.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Movement {
    /// Cursor positions on the monitor: for menus, maps, anything with a visible cursor.
    #[default]
    Absolute,
    /// Raw mouse deltas: for mouse-look, where the game keeps the cursor centred.
    Relative,
}

/// How a macro is played back.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Playback {
    /// 100 is the recorded speed, 200 twice as fast. Scales waits and paths, not hold times
    /// (games need a minimum press length).
    pub speed_percent: u32,
    /// Waits shorter than this are skipped (0 keeps all).
    pub min_delay_ms: u32,
    /// Absolute paths jump straight to their end instead of following the path.
    pub endpoint_only: bool,
}

impl Default for Playback {
    fn default() -> Self {
        Self {
            speed_percent: 100,
            min_delay_ms: 0,
            endpoint_only: false,
        }
    }
}

impl Playback {
    fn speed(&self) -> f64 {
        f64::from(self.speed_percent.max(1)) / 100.0
    }

    /// A wait as played: `None` if it is dropped, otherwise scaled by the speed.
    fn delay(&self, ms: u32) -> Option<u32> {
        (ms >= self.min_delay_ms).then(|| (f64::from(ms) / self.speed()).round() as u32)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "type"
)]
pub enum KeyStep {
    KeyDown {
        key: String,
    },
    KeyUp {
        key: String,
    },
    /// Down, hold, up.
    KeyTap {
        key: String,
        #[serde(default)]
        hold_ms: Option<u32>,
    },
    Delay {
        ms: u32,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MouseButton {
    Left,
    Right,
    Middle,
    X1,
    X2,
}

/// A waypoint of an absolute path: fractions of the macro's monitor, milliseconds after the
/// path starts.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PathPoint {
    pub x: f32,
    pub y: f32,
    pub t: u32,
}

/// A waypoint of a relative path: offset in mickeys from where the path starts, milliseconds
/// after the path starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RelPoint {
    pub dx: i32,
    pub dy: i32,
    pub t: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "type"
)]
pub enum MouseStep {
    ButtonDown {
        button: MouseButton,
    },
    ButtonUp {
        button: MouseButton,
    },
    /// Down, hold, up.
    Click {
        button: MouseButton,
        #[serde(default)]
        hold_ms: Option<u32>,
    },
    /// Wheel detents; positive scrolls up (away from the user).
    Wheel {
        delta: i32,
    },
    /// Relative move in mickeys; Windows pointer acceleration may scale it.
    MoveRelative {
        dx: i32,
        dy: i32,
    },
    /// Jump to a position, as fractions of the macro's monitor.
    MoveTo {
        x: f32,
        y: f32,
    },
    /// Recorded absolute movement. Starts with a jump to the first waypoint.
    MovePath {
        points: Vec<PathPoint>,
    },
    /// Recorded relative movement, e.g. mouse-look.
    MoveRelativePath {
        points: Vec<RelPoint>,
    },
    Delay {
        ms: u32,
    },
}

/// One low-level input operation, ready for the platform executor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputOp {
    Key {
        scan: u16,
        extended: bool,
        down: bool,
    },
    MouseButton {
        button: MouseButton,
        down: bool,
    },
    Wheel {
        delta: i32,
    },
    Move {
        dx: i32,
        dy: i32,
    },
    /// Absolute position over the virtual desktop, 0..=65535 on both axes
    /// (`MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK`).
    MoveAbsolute {
        x: i32,
        y: i32,
    },
    Sleep {
        ms: u32,
    },
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ActionError {
    #[error("unknown key \"{0}\"")]
    UnknownKey(String),
    #[error("delays, hold times and paths must be at most {MAX_DELAY_MS} ms")]
    TooLong,
    #[error("a macro can have at most {MAX_STEPS} steps")]
    TooManySteps,
    #[error("choose a Soundpad sound to play")]
    NoSound,
    #[error("a path needs 1 to {MAX_PATH_POINTS} waypoints")]
    BadPath,
    #[error("positions must be numbers")]
    BadPosition,
    #[error("speed must be between {MIN_SPEED_PERCENT}% and {MAX_SPEED_PERCENT}%")]
    BadSpeed,
    #[error(transparent)]
    Vjoy(#[from] VjoyError),
}

/// A key Detent can send: stable name and its set-1 scan code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyInfo {
    pub name: &'static str,
    pub scan: u16,
    /// Needs the `E0` prefix (`KEYEVENTF_EXTENDEDKEY`).
    pub extended: bool,
}

const fn k(name: &'static str, scan: u16) -> KeyInfo {
    KeyInfo {
        name,
        scan,
        extended: false,
    }
}

const fn e(name: &'static str, scan: u16) -> KeyInfo {
    KeyInfo {
        name,
        scan,
        extended: true,
    }
}

/// All keys that can be used in keyboard macros, in picker order. Names are part of the
/// setup file format: never rename one.
pub const KEYS: &[KeyInfo] = &[
    k("A", 0x1E),
    k("B", 0x30),
    k("C", 0x2E),
    k("D", 0x20),
    k("E", 0x12),
    k("F", 0x21),
    k("G", 0x22),
    k("H", 0x23),
    k("I", 0x17),
    k("J", 0x24),
    k("K", 0x25),
    k("L", 0x26),
    k("M", 0x32),
    k("N", 0x31),
    k("O", 0x18),
    k("P", 0x19),
    k("Q", 0x10),
    k("R", 0x13),
    k("S", 0x1F),
    k("T", 0x14),
    k("U", 0x16),
    k("V", 0x2F),
    k("W", 0x11),
    k("X", 0x2D),
    k("Y", 0x15),
    k("Z", 0x2C),
    k("1", 0x02),
    k("2", 0x03),
    k("3", 0x04),
    k("4", 0x05),
    k("5", 0x06),
    k("6", 0x07),
    k("7", 0x08),
    k("8", 0x09),
    k("9", 0x0A),
    k("0", 0x0B),
    k("F1", 0x3B),
    k("F2", 0x3C),
    k("F3", 0x3D),
    k("F4", 0x3E),
    k("F5", 0x3F),
    k("F6", 0x40),
    k("F7", 0x41),
    k("F8", 0x42),
    k("F9", 0x43),
    k("F10", 0x44),
    k("F11", 0x57),
    k("F12", 0x58),
    k("F13", 0x64),
    k("F14", 0x65),
    k("F15", 0x66),
    k("F16", 0x67),
    k("F17", 0x68),
    k("F18", 0x69),
    k("F19", 0x6A),
    k("F20", 0x6B),
    k("F21", 0x6C),
    k("F22", 0x6D),
    k("F23", 0x6E),
    k("F24", 0x76),
    k("LeftShift", 0x2A),
    k("RightShift", 0x36),
    k("LeftCtrl", 0x1D),
    e("RightCtrl", 0x1D),
    k("LeftAlt", 0x38),
    e("RightAlt", 0x38),
    e("LeftWin", 0x5B),
    e("RightWin", 0x5C),
    e("Menu", 0x5D),
    k("Escape", 0x01),
    k("Tab", 0x0F),
    k("CapsLock", 0x3A),
    k("Space", 0x39),
    k("Enter", 0x1C),
    k("Backspace", 0x0E),
    e("Insert", 0x52),
    e("Delete", 0x53),
    e("Home", 0x47),
    e("End", 0x4F),
    e("PageUp", 0x49),
    e("PageDown", 0x51),
    e("Up", 0x48),
    e("Down", 0x50),
    e("Left", 0x4B),
    e("Right", 0x4D),
    e("PrintScreen", 0x37),
    k("ScrollLock", 0x46),
    e("NumLock", 0x45),
    k("Numpad0", 0x52),
    k("Numpad1", 0x4F),
    k("Numpad2", 0x50),
    k("Numpad3", 0x51),
    k("Numpad4", 0x4B),
    k("Numpad5", 0x4C),
    k("Numpad6", 0x4D),
    k("Numpad7", 0x47),
    k("Numpad8", 0x48),
    k("Numpad9", 0x49),
    k("NumpadAdd", 0x4E),
    k("NumpadSubtract", 0x4A),
    k("NumpadMultiply", 0x37),
    e("NumpadDivide", 0x35),
    k("NumpadDecimal", 0x53),
    e("NumpadEnter", 0x1C),
    k("Minus", 0x0C),
    k("Equals", 0x0D),
    k("LeftBracket", 0x1A),
    k("RightBracket", 0x1B),
    k("Semicolon", 0x27),
    k("Apostrophe", 0x28),
    k("Grave", 0x29),
    k("Backslash", 0x2B),
    k("Comma", 0x33),
    k("Period", 0x34),
    k("Slash", 0x35),
    k("IntlBackslash", 0x56),
];

/// Looks a key up by name, ignoring case.
pub fn key_info(name: &str) -> Option<&'static KeyInfo> {
    KEYS.iter()
        .find(|k| k.name.eq_ignore_ascii_case(name.trim()))
}

/// Looks a key up by scan code and extended flag, as a keyboard reports it.
pub fn key_by_scan(scan: u16, extended: bool) -> Option<&'static KeyInfo> {
    KEYS.iter()
        .find(|k| k.scan == scan && k.extended == extended)
}

fn check_ms(ms: u32) -> Result<(), ActionError> {
    if ms > MAX_DELAY_MS {
        Err(ActionError::TooLong)
    } else {
        Ok(())
    }
}

fn canonical_key(name: &mut String) -> Result<(), ActionError> {
    let info = key_info(name).ok_or_else(|| ActionError::UnknownKey(name.clone()))?;
    *name = info.name.to_owned();
    Ok(())
}

fn check_playback(p: &Playback) -> Result<(), ActionError> {
    if !(MIN_SPEED_PERCENT..=MAX_SPEED_PERCENT).contains(&p.speed_percent) {
        return Err(ActionError::BadSpeed);
    }
    check_ms(p.min_delay_ms)
}

/// Waypoints must exist, stay within limits and be in time order (sorted if not).
fn check_path<P>(points: &mut [P], t: impl Fn(&P) -> u32) -> Result<(), ActionError> {
    if points.is_empty() || points.len() > MAX_PATH_POINTS {
        return Err(ActionError::BadPath);
    }
    points.sort_by_key(&t);
    check_ms(points.last().map(&t).unwrap_or(0))
}

fn check_position(x: f32, y: f32) -> Result<(), ActionError> {
    // Slightly off the monitor is fine (a drag onto a neighbouring monitor); far off is a typo.
    let ok = |v: f32| v.is_finite() && (-10.0..=10.0).contains(&v);
    if ok(x) && ok(y) {
        Ok(())
    } else {
        Err(ActionError::BadPosition)
    }
}

impl Action {
    pub fn keyboard(steps: Vec<KeyStep>) -> Self {
        Action::KeyboardMacro {
            steps,
            playback: Playback::default(),
        }
    }

    pub fn mouse(steps: Vec<MouseStep>) -> Self {
        Action::MouseMacro {
            steps,
            playback: Playback::default(),
            movement: Movement::default(),
            monitor: None,
        }
    }

    /// Checks keys and limits, rewrites key names to their canonical spelling and puts path
    /// waypoints in time order.
    pub fn normalize(&mut self) -> Result<(), ActionError> {
        match self {
            Action::KeyboardMacro { steps, playback } => {
                check_playback(playback)?;
                if steps.len() > MAX_STEPS {
                    return Err(ActionError::TooManySteps);
                }
                for step in steps {
                    match step {
                        KeyStep::KeyDown { key } | KeyStep::KeyUp { key } => canonical_key(key)?,
                        KeyStep::KeyTap { key, hold_ms } => {
                            canonical_key(key)?;
                            check_ms(hold_ms.unwrap_or(0))?;
                        }
                        KeyStep::Delay { ms } => check_ms(*ms)?,
                    }
                }
            }
            Action::MouseMacro {
                steps, playback, ..
            } => {
                check_playback(playback)?;
                if steps.len() > MAX_STEPS {
                    return Err(ActionError::TooManySteps);
                }
                for step in steps {
                    match step {
                        MouseStep::Click { hold_ms, .. } => check_ms(hold_ms.unwrap_or(0))?,
                        MouseStep::Delay { ms } => check_ms(*ms)?,
                        MouseStep::MoveTo { x, y } => check_position(*x, *y)?,
                        MouseStep::MovePath { points } => {
                            check_path(points, |p| p.t)?;
                            for p in points.iter() {
                                check_position(p.x, p.y)?;
                            }
                        }
                        MouseStep::MoveRelativePath { points } => check_path(points, |p| p.t)?,
                        MouseStep::ButtonDown { .. }
                        | MouseStep::ButtonUp { .. }
                        | MouseStep::Wheel { .. }
                        | MouseStep::MoveRelative { .. } => {}
                    }
                }
            }
            Action::Soundpad { command } => {
                if let SoundpadCommand::Play { sound, .. } = command {
                    if sound.is_empty() {
                        return Err(ActionError::NoSound);
                    }
                    sound.title = sound.title.trim().to_owned();
                    sound.path = sound.path.trim().to_owned();
                }
            }
            Action::Vjoy { command } => command.validate()?,
            Action::Discord { .. } | Action::SwitchProfile { .. } => {}
        }
        Ok(())
    }

    /// Whether playing needs the monitor layout (absolute moves).
    pub fn needs_screens(&self) -> bool {
        match self {
            Action::MouseMacro { steps, .. } => steps
                .iter()
                .any(|s| matches!(s, MouseStep::MoveTo { .. } | MouseStep::MovePath { .. })),
            _ => false,
        }
    }

    /// Expands the action into input operations, in order, with the playback settings
    /// applied. Absolute moves need `screens`; without them (or without any monitor) they are
    /// skipped. Problems (also unknown keys, only possible in a hand-edited setup file) are
    /// returned as warnings. Soundpad commands send no input and expand to nothing.
    pub fn expand(&self, screens: Option<&Screens>) -> (Vec<InputOp>, Vec<String>) {
        let mut ops = Vec::new();
        let mut warnings = Vec::new();
        match self {
            Action::KeyboardMacro { steps, playback } => {
                expand_keys(steps, playback, &mut ops, &mut warnings);
            }
            Action::MouseMacro {
                steps,
                playback,
                monitor,
                ..
            } => {
                let target = screens.and_then(|s| Some((s, s.target(monitor.as_ref())?)));
                for step in steps {
                    expand_mouse_step(step, playback, target, &mut ops, &mut warnings);
                }
            }
            Action::Soundpad { .. }
            | Action::Discord { .. }
            | Action::SwitchProfile { .. }
            | Action::Vjoy { .. } => {}
        }
        (ops, warnings)
    }

    /// Short description for logs, e.g. "keys: tap LeftCtrl, down F1".
    pub fn describe(&self) -> String {
        let hold = |h: &Option<u32>| h.map(|ms| format!(" {ms}ms")).unwrap_or_default();
        let percent = |x: f32| format!("{:.1}%", x * 100.0);
        let (kind, parts, playback): (&str, Vec<String>, &Playback) = match self {
            Action::Soundpad { command } => return format!("soundpad: {}", command.describe()),
            Action::Discord { command } => return format!("discord: {}", command.describe()),
            Action::SwitchProfile { target } => {
                return format!("switch profile: {}", target.describe());
            }
            Action::Vjoy { command } => return format!("vjoy: {}", command.describe()),
            Action::KeyboardMacro { steps, playback } => (
                "keys",
                steps
                    .iter()
                    .map(|s| match s {
                        KeyStep::KeyDown { key } => format!("down {key}"),
                        KeyStep::KeyUp { key } => format!("up {key}"),
                        KeyStep::KeyTap { key, hold_ms } => format!("tap {key}{}", hold(hold_ms)),
                        KeyStep::Delay { ms } => format!("wait {ms}ms"),
                    })
                    .collect(),
                playback,
            ),
            Action::MouseMacro {
                steps, playback, ..
            } => (
                "mouse",
                steps
                    .iter()
                    .map(|s| match s {
                        MouseStep::ButtonDown { button } => format!("down {button:?}"),
                        MouseStep::ButtonUp { button } => format!("up {button:?}"),
                        MouseStep::Click { button, hold_ms } => {
                            format!("click {button:?}{}", hold(hold_ms))
                        }
                        MouseStep::Wheel { delta } => format!("wheel {delta}"),
                        MouseStep::MoveRelative { dx, dy } => format!("move {dx},{dy}"),
                        MouseStep::MoveTo { x, y } => format!("to {},{}", percent(*x), percent(*y)),
                        MouseStep::MovePath { points } => format!(
                            "path {} points {}ms",
                            points.len(),
                            points.last().map_or(0, |p| p.t)
                        ),
                        MouseStep::MoveRelativePath { points } => format!(
                            "relative path {} points {}ms",
                            points.len(),
                            points.last().map_or(0, |p| p.t)
                        ),
                        MouseStep::Delay { ms } => format!("wait {ms}ms"),
                    })
                    .collect(),
                playback,
            ),
        };
        let speed = if playback.speed_percent == 100 {
            String::new()
        } else {
            format!(" at {}%", playback.speed_percent)
        };
        format!("{kind}{speed}: {}", parts.join(", "))
    }
}

fn expand_keys(
    steps: &[KeyStep],
    playback: &Playback,
    ops: &mut Vec<InputOp>,
    warnings: &mut Vec<String>,
) {
    let mut key = |name: &str, down: bool, ops: &mut Vec<InputOp>| match key_info(name) {
        Some(info) => ops.push(InputOp::Key {
            scan: info.scan,
            extended: info.extended,
            down,
        }),
        None => {
            let warning = format!("unknown key \"{name}\" skipped");
            if !warnings.contains(&warning) {
                warnings.push(warning);
            }
        }
    };
    for step in steps {
        match step {
            KeyStep::KeyDown { key: k } => key(k, true, ops),
            KeyStep::KeyUp { key: k } => key(k, false, ops),
            KeyStep::KeyTap { key: k, hold_ms } => {
                if key_info(k).is_some() {
                    key(k, true, ops);
                    ops.push(InputOp::Sleep {
                        ms: hold_ms.unwrap_or(DEFAULT_HOLD_MS),
                    });
                }
                key(k, false, ops);
            }
            KeyStep::Delay { ms } => {
                if let Some(ms) = playback.delay(*ms) {
                    ops.push(InputOp::Sleep { ms });
                }
            }
        }
    }
}

/// Appends moves along `plan` (times in ms from the path start), sleeping in between. Times
/// are rounded cumulatively, so rounding never adds up to drift.
fn push_timed(
    plan: &[Sample],
    ops: &mut Vec<InputOp>,
    mut op_at: impl FnMut(&Sample) -> Option<InputOp>,
) {
    let mut elapsed = 0u64;
    for s in plan {
        let at = s.t.max(0.0).round() as u64;
        if at > elapsed {
            ops.push(InputOp::Sleep {
                ms: u32::try_from(at - elapsed).unwrap_or(u32::MAX),
            });
            elapsed = at;
        }
        if let Some(op) = op_at(s) {
            ops.push(op);
        }
    }
}

fn expand_mouse_step(
    step: &MouseStep,
    playback: &Playback,
    target: Option<(&Screens, &MonitorRef)>,
    ops: &mut Vec<InputOp>,
    warnings: &mut Vec<String>,
) {
    let mut absolute = |fx: f32, fy: f32| -> Option<InputOp> {
        let Some((screens, monitor)) = target else {
            let warning = "no monitor known; absolute mouse moves skipped".to_owned();
            if !warnings.contains(&warning) {
                warnings.push(warning);
            }
            return None;
        };
        let (px, py) = path::from_fraction(monitor, fx, fy);
        let (x, y) = screens.to_absolute(px, py)?;
        Some(InputOp::MoveAbsolute { x, y })
    };
    match step {
        &MouseStep::ButtonDown { button } => ops.push(InputOp::MouseButton { button, down: true }),
        &MouseStep::ButtonUp { button } => ops.push(InputOp::MouseButton {
            button,
            down: false,
        }),
        &MouseStep::Click { button, hold_ms } => {
            ops.push(InputOp::MouseButton { button, down: true });
            ops.push(InputOp::Sleep {
                ms: hold_ms.unwrap_or(DEFAULT_HOLD_MS),
            });
            ops.push(InputOp::MouseButton {
                button,
                down: false,
            });
        }
        &MouseStep::Wheel { delta } => ops.push(InputOp::Wheel { delta }),
        &MouseStep::MoveRelative { dx, dy } => ops.push(InputOp::Move { dx, dy }),
        &MouseStep::MoveTo { x, y } => ops.extend(absolute(x, y)),
        MouseStep::MovePath { points } => {
            if playback.endpoint_only {
                if let Some(last) = points.last() {
                    ops.extend(absolute(last.x, last.y));
                }
                return;
            }
            let waypoints: Vec<Sample> = points
                .iter()
                .map(|p| Sample::new(f64::from(p.x), f64::from(p.y), f64::from(p.t)))
                .collect();
            let plan = path::schedule(&waypoints, playback.speed(), PATH_TICK_MS);
            push_timed(&plan, ops, |s| absolute(s.x as f32, s.y as f32));
        }
        MouseStep::MoveRelativePath { points } => {
            // From the origin, where the path starts.
            let waypoints: Vec<Sample> = std::iter::once(Sample::new(0.0, 0.0, 0.0))
                .chain(
                    points
                        .iter()
                        .map(|p| Sample::new(f64::from(p.dx), f64::from(p.dy), f64::from(p.t))),
                )
                .collect();
            let plan = path::schedule(&waypoints, playback.speed(), PATH_TICK_MS);
            let (mut sent_x, mut sent_y) = (0i64, 0i64);
            push_timed(&plan, ops, |s| {
                let (x, y) = (s.x.round() as i64, s.y.round() as i64);
                let (dx, dy) = (x - sent_x, y - sent_y);
                (sent_x, sent_y) = (x, y);
                (dx != 0 || dy != 0).then_some(InputOp::Move {
                    dx: dx as i32,
                    dy: dy as i32,
                })
            });
        }
        &MouseStep::Delay { ms } => {
            if let Some(ms) = playback.delay(ms) {
                ops.push(InputOp::Sleep { ms });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn tap(key: &str, hold_ms: Option<u32>) -> KeyStep {
        KeyStep::KeyTap {
            key: key.into(),
            hold_ms,
        }
    }

    #[test]
    fn key_names_are_unique_and_scan_codes_distinct() {
        let names: HashSet<String> = KEYS.iter().map(|k| k.name.to_lowercase()).collect();
        assert_eq!(names.len(), KEYS.len());
        let codes: HashSet<(u16, bool)> = KEYS.iter().map(|k| (k.scan, k.extended)).collect();
        assert_eq!(codes.len(), KEYS.len());
    }

    #[test]
    fn extended_keys_carry_the_flag() {
        assert_eq!(
            key_info("Up").map(|k| (k.scan, k.extended)),
            Some((0x48, true))
        );
        assert_eq!(
            key_info("Numpad8").map(|k| (k.scan, k.extended)),
            Some((0x48, false))
        );
        assert!(key_info("RightCtrl").unwrap().extended);
        assert!(!key_info("LeftCtrl").unwrap().extended);
    }

    #[test]
    fn keyboard_macro_expands_in_order() {
        let a = Action::keyboard(vec![
            KeyStep::KeyDown {
                key: "LeftCtrl".into(),
            },
            tap("F1", None),
            KeyStep::Delay { ms: 100 },
            KeyStep::KeyUp {
                key: "LeftCtrl".into(),
            },
            tap("Up", Some(10)),
        ]);
        let (ops, warnings) = a.expand(None);
        assert!(warnings.is_empty());
        let key = |scan, extended, down| InputOp::Key {
            scan,
            extended,
            down,
        };
        assert_eq!(
            ops,
            [
                key(0x1D, false, true),
                key(0x3B, false, true),
                InputOp::Sleep {
                    ms: DEFAULT_HOLD_MS
                },
                key(0x3B, false, false),
                InputOp::Sleep { ms: 100 },
                key(0x1D, false, false),
                key(0x48, true, true),
                InputOp::Sleep { ms: 10 },
                key(0x48, true, false),
            ]
        );
    }

    #[test]
    fn mouse_macro_expands_in_order() {
        let a = Action::mouse(vec![
            MouseStep::Click {
                button: MouseButton::X1,
                hold_ms: Some(5),
            },
            MouseStep::Wheel { delta: -2 },
            MouseStep::MoveRelative { dx: 10, dy: -3 },
            MouseStep::ButtonDown {
                button: MouseButton::Right,
            },
            MouseStep::Delay { ms: 7 },
            MouseStep::ButtonUp {
                button: MouseButton::Right,
            },
        ]);
        let (ops, _) = a.expand(None);
        assert_eq!(
            ops,
            [
                InputOp::MouseButton {
                    button: MouseButton::X1,
                    down: true
                },
                InputOp::Sleep { ms: 5 },
                InputOp::MouseButton {
                    button: MouseButton::X1,
                    down: false
                },
                InputOp::Wheel { delta: -2 },
                InputOp::Move { dx: 10, dy: -3 },
                InputOp::MouseButton {
                    button: MouseButton::Right,
                    down: true
                },
                InputOp::Sleep { ms: 7 },
                InputOp::MouseButton {
                    button: MouseButton::Right,
                    down: false
                },
            ]
        );
    }

    #[test]
    fn unknown_keys_are_skipped_and_reported() {
        let a = Action::keyboard(vec![
            tap("Hyper", None),
            tap("A", Some(1)),
            tap("Hyper", None),
        ]);
        let (ops, warnings) = a.expand(None);
        assert_eq!(warnings, [r#"unknown key "Hyper" skipped"#]);
        assert_eq!(ops.len(), 3);
    }

    fn sleeps(ops: &[InputOp]) -> Vec<u32> {
        ops.iter()
            .filter_map(|op| match op {
                InputOp::Sleep { ms } => Some(*ms),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn speed_scales_waits_but_not_holds_and_short_waits_can_be_dropped() {
        let steps = vec![
            KeyStep::Delay { ms: 100 },
            tap("A", Some(40)),
            KeyStep::Delay { ms: 5 },
            tap("B", None),
        ];
        let play = |speed_percent, min_delay_ms| {
            let a = Action::KeyboardMacro {
                steps: steps.clone(),
                playback: Playback {
                    speed_percent,
                    min_delay_ms,
                    endpoint_only: false,
                },
            };
            sleeps(&a.expand(None).0)
        };
        assert_eq!(play(100, 0), [100, 40, 5, 50]);
        assert_eq!(play(200, 0), [50, 40, 3, 50]);
        assert_eq!(play(50, 0), [200, 40, 10, 50]);
        // Waits below 10 ms are dropped; holds stay.
        assert_eq!(play(100, 10), [100, 40, 50]);
        let mouse = Action::MouseMacro {
            steps: vec![MouseStep::Delay { ms: 30 }, MouseStep::Delay { ms: 300 }],
            playback: Playback {
                speed_percent: 300,
                min_delay_ms: 50,
                endpoint_only: false,
            },
            movement: Movement::Relative,
            monitor: None,
        };
        assert_eq!(sleeps(&mouse.expand(None).0), [100]);
    }

    fn screens() -> Screens {
        Screens {
            monitors: vec![
                MonitorRef {
                    name: Some("A".into()),
                    x: 0,
                    y: 0,
                    width: 2000,
                    height: 1000,
                },
                MonitorRef {
                    name: Some("B".into()),
                    x: 2000,
                    y: 0,
                    width: 2000,
                    height: 1000,
                },
            ],
            primary: Some(0),
        }
    }

    fn abs_path(endpoint_only: bool, monitor: Option<MonitorRef>) -> Action {
        Action::MouseMacro {
            steps: vec![MouseStep::MovePath {
                points: vec![
                    PathPoint {
                        x: 0.0,
                        y: 0.0,
                        t: 0,
                    },
                    PathPoint {
                        x: 0.5,
                        y: 0.5,
                        t: 16,
                    },
                ],
            }],
            playback: Playback {
                endpoint_only,
                ..Playback::default()
            },
            movement: Movement::Absolute,
            monitor,
        }
    }

    #[test]
    fn absolute_paths_follow_the_waypoints_on_the_macro_monitor() {
        let sc = screens();
        let (ops, warnings) = abs_path(false, None).expand(Some(&sc));
        assert!(warnings.is_empty());
        // Desktop is 4000x1000; the primary monitor is the left half.
        assert_eq!(
            ops,
            [
                InputOp::MoveAbsolute { x: 0, y: 0 },
                InputOp::Sleep { ms: 8 },
                InputOp::MoveAbsolute { x: 8192, y: 16384 },
                InputOp::Sleep { ms: 8 },
                InputOp::MoveAbsolute { x: 16384, y: 32768 },
            ]
        );
        // On the right monitor, positions shift by half the desktop.
        let (ops, _) = abs_path(true, Some(sc.monitors[1].clone())).expand(Some(&sc));
        assert_eq!(ops, [InputOp::MoveAbsolute { x: 49152, y: 32768 }]);
        // Without monitors, absolute moves are skipped with a warning.
        let (ops, warnings) = abs_path(false, None).expand(None);
        assert_eq!(sleeps(&ops), [8, 8]);
        assert_eq!(warnings.len(), 1);
        assert!(abs_path(false, None).needs_screens());
    }

    #[test]
    fn relative_paths_send_deltas_that_add_up() {
        let a = Action::MouseMacro {
            steps: vec![MouseStep::MoveRelativePath {
                points: vec![
                    RelPoint { dx: 3, dy: 0, t: 0 },
                    RelPoint {
                        dx: 100,
                        dy: -10,
                        t: 40,
                    },
                ],
            }],
            playback: Playback {
                speed_percent: 200,
                ..Playback::default()
            },
            movement: Movement::Relative,
            monitor: None,
        };
        let (ops, _) = a.expand(None);
        let (dx, dy) = ops.iter().fold((0, 0), |(x, y), op| match op {
            InputOp::Move { dx, dy } => (x + dx, y + dy),
            _ => (x, y),
        });
        assert_eq!((dx, dy), (100, -10));
        // Twice as fast: 20 ms in total, in 8 ms steps.
        assert_eq!(sleeps(&ops).iter().sum::<u32>(), 20);
        assert_eq!(ops[0], InputOp::Move { dx: 3, dy: 0 });
        assert!(!a.needs_screens());
    }

    #[test]
    fn normalize_canonicalises_and_validates() {
        let mut a = Action::keyboard(vec![tap(" leftctrl ", None)]);
        a.normalize().unwrap();
        assert_eq!(a, Action::keyboard(vec![tap("LeftCtrl", None)]));

        let mut bad = Action::keyboard(vec![tap("Nope", None)]);
        assert_eq!(bad.normalize(), Err(ActionError::UnknownKey("Nope".into())));
        let mut long = Action::mouse(vec![MouseStep::Delay {
            ms: MAX_DELAY_MS + 1,
        }]);
        assert_eq!(long.normalize(), Err(ActionError::TooLong));
        let mut slow = Action::KeyboardMacro {
            steps: vec![],
            playback: Playback {
                speed_percent: 5,
                ..Playback::default()
            },
        };
        assert_eq!(slow.normalize(), Err(ActionError::BadSpeed));
        let mut empty_path = Action::mouse(vec![MouseStep::MovePath { points: vec![] }]);
        assert_eq!(empty_path.normalize(), Err(ActionError::BadPath));
        let mut nan = Action::mouse(vec![MouseStep::MoveTo {
            x: f32::NAN,
            y: 0.0,
        }]);
        assert_eq!(nan.normalize(), Err(ActionError::BadPosition));
        // Waypoints are put in time order.
        let mut unordered = Action::mouse(vec![MouseStep::MoveRelativePath {
            points: vec![
                RelPoint { dx: 2, dy: 0, t: 9 },
                RelPoint { dx: 1, dy: 0, t: 3 },
            ],
        }]);
        unordered.normalize().unwrap();
        let Action::MouseMacro { steps, .. } = &unordered else {
            unreachable!()
        };
        assert_eq!(
            steps[0],
            MouseStep::MoveRelativePath {
                points: vec![
                    RelPoint { dx: 1, dy: 0, t: 3 },
                    RelPoint { dx: 2, dy: 0, t: 9 },
                ],
            }
        );
    }

    #[test]
    fn json_shape_is_stable() {
        let a = Action::keyboard(vec![tap("F1", Some(20)), KeyStep::Delay { ms: 5 }]);
        assert_eq!(
            serde_json::to_string(&a).unwrap(),
            r#"{"type":"keyboardMacro","steps":[{"type":"keyTap","key":"F1","holdMs":20},{"type":"delay","ms":5}],"playback":{"speedPercent":100,"minDelayMs":0,"endpointOnly":false}}"#
        );
        // Macros from before playback settings load with defaults.
        let m: Action = serde_json::from_str(
            r#"{"type":"mouseMacro","steps":[{"type":"click","button":"x2"},
                {"type":"moveRelative","dx":1,"dy":2},{"type":"wheel","delta":1}]}"#,
        )
        .unwrap();
        assert_eq!(m.describe(), "mouse: click X2, move 1,2, wheel 1");
        let Action::MouseMacro {
            playback,
            movement,
            monitor,
            ..
        } = &m
        else {
            unreachable!()
        };
        assert_eq!(
            (playback, movement, monitor),
            (&Playback::default(), &Movement::Absolute, &None)
        );
        let recorded: Action = serde_json::from_str(
            r#"{"type":"mouseMacro","movement":"relative","playback":{"speedPercent":50},
                "monitor":{"name":"\\\\.\\DISPLAY1","x":0,"y":0,"width":3440,"height":1440},
                "steps":[{"type":"moveTo","x":0.5,"y":0.25},
                {"type":"movePath","points":[{"x":0.1,"y":0.2,"t":0},{"x":0.3,"y":0.4,"t":120}]},
                {"type":"moveRelativePath","points":[{"dx":5,"dy":-2,"t":0},{"dx":40,"dy":3,"t":64}]}]}"#,
        )
        .unwrap();
        assert_eq!(
            recorded.describe(),
            "mouse at 50%: to 50.0%,25.0%, path 2 points 120ms, relative path 2 points 64ms"
        );
        let json = serde_json::to_value(&recorded).unwrap();
        assert_eq!(json["playback"]["minDelayMs"], 0);
        assert_eq!(json["steps"][1]["points"][1]["t"], 120);
        assert_eq!(serde_json::from_value::<Action>(json).unwrap(), recorded);
        let sp: Action = serde_json::from_str(
            r#"{"type":"soundpad","command":{"type":"play","sound":{"title":"Siren","path":"C:\\s.mp3"},"output":"mic"}}"#,
        )
        .unwrap();
        assert_eq!(sp.describe(), r#"soundpad: play "Siren" (mic)"#);
        assert!(sp.expand(None).0.is_empty());
        let mut dc: Action =
            serde_json::from_str(r#"{"type":"discord","command":"toggleDeafen"}"#).unwrap();
        assert_eq!(dc.normalize(), Ok(()));
        assert_eq!(dc.describe(), "discord: toggle deafen");
        assert!(dc.expand(None).0.is_empty());
        assert_eq!(
            serde_json::to_string(&dc).unwrap(),
            r#"{"type":"discord","command":"toggleDeafen"}"#
        );
    }

    #[test]
    fn switch_profile_sends_no_input_and_needs_no_connection() {
        let mut a: Action =
            serde_json::from_str(r#"{"type":"switchProfile","target":{"type":"previous"}}"#)
                .unwrap();
        assert_eq!(a.normalize(), Ok(()));
        assert_eq!(a.describe(), "switch profile: previous");
        assert!(a.expand(None).0.is_empty());
        assert!(!a.needs_screens());
        assert_eq!(crate::connection::ConnectorId::for_action(&a), None);
        let to = Action::SwitchProfile {
            target: ProfileTarget::Profile {
                profile: "profile-2".into(),
            },
        };
        assert_eq!(
            serde_json::to_string(&to).unwrap(),
            r#"{"type":"switchProfile","target":{"type":"profile","profile":"profile-2"}}"#
        );
    }

    #[test]
    fn keys_are_found_by_scan_code() {
        assert_eq!(key_by_scan(0x48, true).map(|k| k.name), Some("Up"));
        assert_eq!(key_by_scan(0x48, false).map(|k| k.name), Some("Numpad8"));
        assert_eq!(key_by_scan(0x1D, true).map(|k| k.name), Some("RightCtrl"));
        assert_eq!(key_by_scan(0x7F, false), None);
    }

    #[test]
    fn soundpad_play_needs_a_sound() {
        use crate::soundpad::{SoundOutput, SoundRef};
        let mut a = Action::Soundpad {
            command: SoundpadCommand::Play {
                sound: SoundRef::default(),
                output: SoundOutput::Both,
            },
        };
        assert_eq!(a.normalize(), Err(ActionError::NoSound));
        let mut stop = Action::Soundpad {
            command: SoundpadCommand::Stop,
        };
        assert_eq!(stop.normalize(), Ok(()));
    }
}

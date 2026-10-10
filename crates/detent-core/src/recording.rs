//! The macro recorder's logic: turning raw keyboard and mouse events into macro steps.
//!
//! The app captures events with low-level hooks (and Raw Input for relative movement) and
//! hands them over as [`Recorded`] events with timestamps. Everything after that is here:
//! removing the events that stopped the recording, pairing presses and releases, turning
//! gaps into waits, folding press-wait-release into taps and clicks, and thinning and
//! simplifying mouse movement into paths of waypoints.
//!
//! A keyboard macro records only keys and a mouse macro only buttons, wheel and movement;
//! the two kinds are never mixed.

use crate::action::{
    Action, KeyInfo, KeyStep, MAX_DELAY_MS, MAX_STEPS, MouseButton, MouseStep, Movement, PathPoint,
    Playback, RelPoint, key_by_scan, key_info,
};
use crate::overlay::MonitorRef;
use crate::path::{self, Sample, Screens};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;

/// Default hotkey that stops a recording.
pub const DEFAULT_STOP_HOTKEY: &str = "Ctrl+Shift+F9";
/// Seconds between pressing "Record" and the start of recording, to switch to the game.
pub const COUNTDOWN_MS: u64 = 3_000;
/// One wheel detent in Windows' units.
const WHEEL_DELTA: i32 = 120;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RecordKind {
    Keyboard,
    Mouse,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RecorderConfig {
    /// Stops a recording; Tauri accelerator syntax, e.g. "Ctrl+Shift+F9".
    pub stop_hotkey: String,
    /// How far (pixels, or mickeys for relative movement) a simplified path may deviate from
    /// the recorded one.
    pub tolerance: f32,
    /// Mouse movement is sampled at most this often.
    pub sample_interval_ms: u32,
}

impl Default for RecorderConfig {
    fn default() -> Self {
        Self {
            stop_hotkey: DEFAULT_STOP_HOTKEY.into(),
            tolerance: 2.0,
            sample_interval_ms: 8,
        }
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum RecorderError {
    #[error("hotkey \"{0}\" is not valid: use modifiers and one key, e.g. Ctrl+Shift+F9")]
    BadHotkey(String),
    #[error("Escape cancels a recording and cannot stop it")]
    EscapeHotkey,
    #[error("tolerance must be between 0 and 100, sample interval between 1 and 100 ms")]
    BadSettings,
}

impl RecorderConfig {
    pub fn validate(&self) -> Result<(), RecorderError> {
        Hotkey::parse(&self.stop_hotkey)?;
        if !(0.0..=100.0).contains(&self.tolerance) || !(1..=100).contains(&self.sample_interval_ms)
        {
            return Err(RecorderError::BadSettings);
        }
        Ok(())
    }
}

/// A hotkey as keys: modifiers (either side) plus one key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hotkey {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub win: bool,
    pub key: &'static KeyInfo,
}

const MODIFIERS: [(&str, &str); 4] = [
    ("LeftCtrl", "RightCtrl"),
    ("LeftShift", "RightShift"),
    ("LeftAlt", "RightAlt"),
    ("LeftWin", "RightWin"),
];

impl Hotkey {
    /// Parses Tauri accelerator syntax ("Ctrl+Shift+F9", "Alt+Super+K", "CmdOrCtrl+F1").
    pub fn parse(text: &str) -> Result<Self, RecorderError> {
        let bad = || RecorderError::BadHotkey(text.into());
        let mut hk = (false, false, false, false);
        let mut key = None;
        for part in text.split('+').map(str::trim) {
            match part.to_ascii_lowercase().as_str() {
                "ctrl" | "control" | "cmdorctrl" | "commandorcontrol" => hk.0 = true,
                "shift" => hk.1 = true,
                "alt" | "option" => hk.2 = true,
                "super" | "win" | "meta" | "cmd" | "command" => hk.3 = true,
                "esc" | "escape" => return Err(RecorderError::EscapeHotkey),
                _ if key.is_some() => return Err(bad()),
                _ => {
                    let info = key_info(part).ok_or_else(bad)?;
                    if MODIFIERS
                        .iter()
                        .any(|(l, r)| info.name == *l || info.name == *r)
                    {
                        return Err(bad());
                    }
                    key = Some(info);
                }
            }
        }
        Ok(Self {
            ctrl: hk.0,
            shift: hk.1,
            alt: hk.2,
            win: hk.3,
            key: key.ok_or_else(bad)?,
        })
    }

    /// Whether `key` is one of this hotkey's modifiers (either side).
    pub fn has_modifier(&self, key: &KeyInfo) -> bool {
        let used = [self.ctrl, self.shift, self.alt, self.win];
        MODIFIERS
            .iter()
            .zip(used)
            .any(|((l, r), on)| on && (key.name == *l || key.name == *r))
    }
}

/// A raw input event as captured.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RawEvent {
    /// Scan code and extended flag, as the keyboard hook reports them.
    Key {
        scan: u16,
        extended: bool,
        down: bool,
    },
    Button {
        button: MouseButton,
        down: bool,
    },
    /// Wheel movement in Windows' units (120 per detent; precise wheels send less).
    Wheel {
        delta: i32,
    },
    /// Cursor position in physical pixels of the virtual desktop.
    Cursor {
        x: i32,
        y: i32,
    },
    /// Raw mouse movement in mickeys.
    RawMove {
        dx: i32,
        dy: i32,
    },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Recorded {
    /// Microseconds since recording started.
    pub t_us: u64,
    pub event: RawEvent,
}

/// How a recording ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopReason {
    /// The stop hotkey; its key press was not recorded, but its modifiers were.
    Hotkey,
    /// The Stop button in Detent's window; for mouse macros the click on it (and the movement
    /// towards it) was recorded.
    Ui,
}

/// Removes the events that belong to stopping: the hotkey's modifier presses still held at
/// the end, or the final left click (on the Stop button) and the movement leading to it.
pub fn trim_stop(events: &mut Vec<Recorded>, stop: StopReason, hotkey: Option<&Hotkey>) {
    match stop {
        StopReason::Hotkey => {
            let Some(hotkey) = hotkey else { return };
            // Per key: index of the first press since its last release.
            let mut since: HashMap<(u16, bool), usize> = HashMap::new();
            for (i, e) in events.iter().enumerate() {
                if let RawEvent::Key {
                    scan,
                    extended,
                    down,
                } = e.event
                {
                    if down {
                        since.entry((scan, extended)).or_insert(i);
                    } else {
                        since.remove(&(scan, extended));
                    }
                }
            }
            let drop: Vec<((u16, bool), usize)> = since
                .into_iter()
                .filter(|((scan, ext), _)| {
                    key_by_scan(*scan, *ext).is_some_and(|k| hotkey.has_modifier(k))
                })
                .collect();
            let mut i = 0;
            events.retain(|e| {
                let keep = !matches!(e.event, RawEvent::Key { scan, extended, .. }
                    if drop.iter().any(|(k, from)| *k == (scan, extended) && i >= *from));
                i += 1;
                keep
            });
        }
        StopReason::Ui => {
            let last_click = events.iter().rposition(|e| {
                e.event
                    == RawEvent::Button {
                        button: MouseButton::Left,
                        down: true,
                    }
            });
            if let Some(i) = last_click {
                events.truncate(i);
                while matches!(
                    events.last().map(|e| e.event),
                    Some(RawEvent::Cursor { .. } | RawEvent::RawMove { .. })
                ) {
                    events.pop();
                }
            }
        }
    }
}

/// Turns gaps between events into waits.
struct Clock {
    last_us: Option<u64>,
}

impl Clock {
    /// The wait before an event at `t_us`, if any; the event's time becomes the reference.
    fn wait(&mut self, t_us: u64) -> Option<u32> {
        let wait = self.last_us.map(|last| {
            let ms = (t_us.saturating_sub(last) as f64 / 1000.0).round();
            (ms as u32).min(MAX_DELAY_MS)
        });
        self.last_us = Some(t_us);
        wait.filter(|&ms| ms > 0)
    }
}

fn ms(us: u64) -> f64 {
    us as f64 / 1000.0
}

/// Keyboard events to steps. Auto-repeated presses are dropped, releases of keys pressed
/// before the recording started are dropped, keys still held at the end are released, and
/// press-wait-release of one key becomes a tap with that hold time.
pub fn keyboard_steps(events: &[Recorded]) -> (Vec<KeyStep>, Vec<String>) {
    let mut steps = Vec::new();
    let mut held: Vec<&'static KeyInfo> = Vec::new();
    let mut unknown = 0;
    let mut clock = Clock { last_us: None };
    for e in events {
        let RawEvent::Key {
            scan,
            extended,
            down,
        } = e.event
        else {
            continue;
        };
        let Some(info) = key_by_scan(scan, extended) else {
            unknown += 1;
            continue;
        };
        let was_held = held.iter().any(|k| k.name == info.name);
        if down == was_held {
            continue;
        }
        if down {
            held.push(info);
        } else {
            held.retain(|k| k.name != info.name);
        }
        if let Some(ms) = clock.wait(e.t_us) {
            steps.push(KeyStep::Delay { ms });
        }
        let key = info.name.to_owned();
        steps.push(if down {
            KeyStep::KeyDown { key }
        } else {
            KeyStep::KeyUp { key }
        });
    }
    for k in held {
        steps.push(KeyStep::KeyUp {
            key: k.name.to_owned(),
        });
    }
    let mut notes = Vec::new();
    if unknown > 0 {
        notes.push(format!(
            "{unknown} key events skipped (keys Detent cannot send)"
        ));
    }
    let steps = fold_taps(steps);
    (cap_steps(steps, &mut notes), notes)
}

fn fold_taps(steps: Vec<KeyStep>) -> Vec<KeyStep> {
    let mut out: Vec<KeyStep> = Vec::with_capacity(steps.len());
    let mut i = 0;
    while i < steps.len() {
        if let KeyStep::KeyDown { key } = &steps[i] {
            let (hold, up_at) = match steps.get(i + 1) {
                Some(KeyStep::Delay { ms }) => (*ms, i + 2),
                _ => (0, i + 1),
            };
            if matches!(steps.get(up_at), Some(KeyStep::KeyUp { key: k }) if k == key) {
                out.push(KeyStep::KeyTap {
                    key: key.clone(),
                    hold_ms: Some(hold),
                });
                i = up_at + 1;
                continue;
            }
        }
        out.push(steps[i].clone());
        i += 1;
    }
    out
}

fn fold_clicks(steps: Vec<MouseStep>) -> Vec<MouseStep> {
    let mut out: Vec<MouseStep> = Vec::with_capacity(steps.len());
    let mut i = 0;
    while i < steps.len() {
        if let MouseStep::ButtonDown { button } = steps[i] {
            let (hold, up_at) = match steps.get(i + 1) {
                Some(MouseStep::Delay { ms }) => (*ms, i + 2),
                _ => (0, i + 1),
            };
            if steps.get(up_at) == Some(&MouseStep::ButtonUp { button }) {
                out.push(MouseStep::Click {
                    button,
                    hold_ms: Some(hold),
                });
                i = up_at + 1;
                continue;
            }
        }
        out.push(steps[i].clone());
        i += 1;
    }
    out
}

fn cap_steps<T>(mut steps: Vec<T>, notes: &mut Vec<String>) -> Vec<T> {
    if steps.len() > MAX_STEPS {
        steps.truncate(MAX_STEPS);
        notes.push(format!("recording cut to {MAX_STEPS} steps"));
    }
    steps
}

#[derive(Debug, Clone, Copy)]
pub struct PathOptions {
    /// See [`RecorderConfig::tolerance`].
    pub tolerance: f64,
    /// See [`RecorderConfig::sample_interval_ms`].
    pub sample_interval_ms: f64,
}

impl From<&RecorderConfig> for PathOptions {
    fn from(c: &RecorderConfig) -> Self {
        Self {
            tolerance: f64::from(c.tolerance),
            sample_interval_ms: f64::from(c.sample_interval_ms),
        }
    }
}

/// Mouse events to steps. `movement` decides which movement events count: cursor positions
/// (absolute, stored relative to the monitor the recording started on) or raw deltas
/// (relative). Movement between two other events becomes one path, thinned to the sample
/// interval and simplified; a path of a single position becomes a plain jump.
pub fn mouse_steps(
    events: &[Recorded],
    movement: Movement,
    screens: &Screens,
    options: PathOptions,
) -> (Vec<MouseStep>, Option<MonitorRef>, Vec<String>) {
    let mut notes = Vec::new();
    let monitor = match movement {
        Movement::Absolute => events
            .iter()
            .find_map(|e| match e.event {
                RawEvent::Cursor { x, y } => Some((x, y)),
                _ => None,
            })
            .and_then(|(x, y)| screens.monitor_at(x, y).cloned()),
        Movement::Relative => None,
    };
    let mut steps = Vec::new();
    let mut clock = Clock { last_us: None };
    // Current path: samples in pixels (absolute) or cumulative mickeys (relative), times in
    // microseconds since recording start.
    let mut path: Vec<(u64, f64, f64)> = Vec::new();
    let flush = |path: &mut Vec<(u64, f64, f64)>, steps: &mut Vec<MouseStep>, clock: &mut Clock| {
        let Some(&(start, ..)) = path.first() else {
            return;
        };
        if let Some(ms) = clock.wait(start) {
            steps.push(MouseStep::Delay { ms });
        }
        let samples: Vec<Sample> = path
            .iter()
            .map(|&(t, x, y)| Sample::new(x, y, ms(t - start)))
            .collect();
        let samples = path::cap_rate(&samples, options.sample_interval_ms);
        let points = path::simplify(&samples, options.tolerance);
        clock.last_us = Some(path.last().map_or(start, |p| p.0));
        path.clear();
        match movement {
            Movement::Absolute => {
                let Some(m) = &monitor else { return };
                let frac: Vec<PathPoint> = points
                    .iter()
                    .map(|p| {
                        let (x, y) = path::to_fraction(m, p.x, p.y);
                        PathPoint {
                            x,
                            y,
                            t: p.t.round() as u32,
                        }
                    })
                    .collect();
                steps.push(match frac[..] {
                    [only] => MouseStep::MoveTo {
                        x: only.x,
                        y: only.y,
                    },
                    _ => MouseStep::MovePath { points: frac },
                });
            }
            Movement::Relative => steps.push(MouseStep::MoveRelativePath {
                points: points
                    .iter()
                    .map(|p| RelPoint {
                        dx: p.x.round() as i32,
                        dy: p.y.round() as i32,
                        t: p.t.round() as u32,
                    })
                    .collect(),
            }),
        }
    };
    let max_path_us = u64::from(MAX_DELAY_MS) * 1000;
    let mut held: Vec<MouseButton> = Vec::new();
    let mut wheel = 0;
    for e in events {
        match e.event {
            RawEvent::Cursor { x, y } if movement == Movement::Absolute => {
                if path.first().is_some_and(|p| e.t_us - p.0 > max_path_us) {
                    flush(&mut path, &mut steps, &mut clock);
                }
                path.push((e.t_us, f64::from(x), f64::from(y)));
            }
            RawEvent::RawMove { dx, dy } if movement == Movement::Relative => {
                if path.first().is_some_and(|p| e.t_us - p.0 > max_path_us) {
                    flush(&mut path, &mut steps, &mut clock);
                }
                let (x, y) = path.last().map_or((0.0, 0.0), |p| (p.1, p.2));
                path.push((e.t_us, x + f64::from(dx), y + f64::from(dy)));
            }
            RawEvent::Button { button, down } => {
                if down == held.contains(&button) {
                    continue;
                }
                flush(&mut path, &mut steps, &mut clock);
                if down {
                    held.push(button);
                } else {
                    held.retain(|b| *b != button);
                }
                if let Some(ms) = clock.wait(e.t_us) {
                    steps.push(MouseStep::Delay { ms });
                }
                steps.push(if down {
                    MouseStep::ButtonDown { button }
                } else {
                    MouseStep::ButtonUp { button }
                });
            }
            RawEvent::Wheel { delta } => {
                wheel += delta;
                let detents = wheel / WHEEL_DELTA;
                if detents == 0 {
                    continue;
                }
                wheel -= detents * WHEEL_DELTA;
                flush(&mut path, &mut steps, &mut clock);
                if let Some(ms) = clock.wait(e.t_us) {
                    steps.push(MouseStep::Delay { ms });
                }
                steps.push(MouseStep::Wheel { delta: detents });
            }
            _ => {}
        }
    }
    flush(&mut path, &mut steps, &mut clock);
    for button in held {
        steps.push(MouseStep::ButtonUp { button });
    }
    if movement == Movement::Absolute
        && monitor.is_none()
        && events
            .iter()
            .any(|e| matches!(e.event, RawEvent::Cursor { .. }))
    {
        notes.push("no monitor found; cursor movement was not recorded".into());
    }
    let steps = fold_clicks(steps);
    (cap_steps(steps, &mut notes), monitor, notes)
}

/// Everything a finished recording needs to become a macro.
pub struct Finished<'a> {
    pub kind: RecordKind,
    pub movement: Movement,
    pub events: Vec<Recorded>,
    pub stop: StopReason,
    pub config: &'a RecorderConfig,
    pub screens: &'a Screens,
}

/// The recorded macro (default playback settings) and notes for the user.
pub fn finish(f: Finished<'_>) -> (Action, Vec<String>) {
    let mut events = f.events;
    let hotkey = Hotkey::parse(&f.config.stop_hotkey).ok();
    trim_stop(&mut events, f.stop, hotkey.as_ref());
    match f.kind {
        RecordKind::Keyboard => {
            let (steps, notes) = keyboard_steps(&events);
            (Action::keyboard(steps), notes)
        }
        RecordKind::Mouse => {
            let (steps, monitor, notes) =
                mouse_steps(&events, f.movement, f.screens, f.config.into());
            (
                Action::MouseMacro {
                    steps,
                    playback: Playback::default(),
                    movement: f.movement,
                    monitor,
                },
                notes,
            )
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RecorderPhase {
    #[default]
    Idle,
    /// Waiting before recording starts, so the user can switch to the game.
    Countdown,
    Recording,
}

/// A finished recording, until the UI has taken it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingResult {
    pub session: u64,
    /// The recorded macro; its steps go into the macro being edited.
    pub action: Action,
    pub notes: Vec<String>,
}

/// The recorder's state, as the UI and the overlay show it.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecorderView {
    /// Counts recordings, so the UI can tell results apart.
    pub session: u64,
    pub phase: RecorderPhase,
    pub kind: Option<RecordKind>,
    pub movement: Movement,
    pub result: Option<RecordingResult>,
    /// Why the last recording failed (e.g. hooks could not be installed).
    pub error: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(t_ms: u64, name: &str, down: bool) -> Recorded {
        let info = key_info(name).unwrap();
        Recorded {
            t_us: t_ms * 1000,
            event: RawEvent::Key {
                scan: info.scan,
                extended: info.extended,
                down,
            },
        }
    }

    fn ev(t_ms: u64, event: RawEvent) -> Recorded {
        Recorded {
            t_us: t_ms * 1000,
            event,
        }
    }

    fn tap(name: &str, hold: u32) -> KeyStep {
        KeyStep::KeyTap {
            key: name.into(),
            hold_ms: Some(hold),
        }
    }

    fn wait(ms: u32) -> KeyStep {
        KeyStep::Delay { ms }
    }

    #[test]
    fn hotkeys_parse() {
        let hk = Hotkey::parse("Ctrl+Shift+F9").unwrap();
        assert!(hk.ctrl && hk.shift && !hk.alt && !hk.win);
        assert_eq!(hk.key.name, "F9");
        assert!(hk.has_modifier(key_info("RightCtrl").unwrap()));
        assert!(!hk.has_modifier(key_info("LeftAlt").unwrap()));
        assert!(!hk.has_modifier(key_info("F9").unwrap()));
        assert_eq!(Hotkey::parse("alt + super + k").unwrap().key.name, "K");
        assert!(Hotkey::parse("CmdOrCtrl+F1").unwrap().ctrl);
        assert!(matches!(
            Hotkey::parse("Ctrl+Shift"),
            Err(RecorderError::BadHotkey(_))
        ));
        assert!(Hotkey::parse("Ctrl+A+B").is_err());
        assert!(Hotkey::parse("Ctrl+Hyper").is_err());
        assert_eq!(Hotkey::parse("Ctrl+Esc"), Err(RecorderError::EscapeHotkey));
        assert!(RecorderConfig::default().validate().is_ok());
        let bad = RecorderConfig {
            sample_interval_ms: 0,
            ..RecorderConfig::default()
        };
        assert_eq!(bad.validate(), Err(RecorderError::BadSettings));
    }

    #[test]
    fn keys_become_taps_and_waits() {
        let events = [
            key(1000, "LeftCtrl", true),
            key(1100, "F1", true),
            key(1180, "F1", false),
            key(1250, "LeftCtrl", false),
            key(2250, "A", true),
            key(2300, "A", false),
        ];
        let (steps, notes) = keyboard_steps(&events);
        assert!(notes.is_empty());
        assert_eq!(
            steps,
            [
                KeyStep::KeyDown {
                    key: "LeftCtrl".into()
                },
                wait(100),
                tap("F1", 80),
                wait(70),
                KeyStep::KeyUp {
                    key: "LeftCtrl".into()
                },
                wait(1000),
                tap("A", 50),
            ]
        );
    }

    #[test]
    fn repeats_stray_releases_and_held_keys_are_cleaned_up() {
        let events = [
            // Released, but pressed before the recording started.
            key(0, "Enter", false),
            key(10, "W", true),
            // Auto-repeat while W is held.
            key(500, "W", true),
            key(530, "W", true),
            key(900, "W", false),
            key(1000, "Up", true),
        ];
        let (steps, _) = keyboard_steps(&events);
        assert_eq!(
            steps,
            [
                tap("W", 890),
                wait(100),
                // Held at the end: released, so nothing stays stuck.
                tap("Up", 0),
            ]
        );
    }

    #[test]
    fn extended_and_unknown_scan_codes() {
        let events = [
            ev(
                0,
                RawEvent::Key {
                    scan: 0x48,
                    extended: true,
                    down: true,
                },
            ),
            ev(
                5,
                RawEvent::Key {
                    scan: 0x7F,
                    extended: false,
                    down: true,
                },
            ),
            ev(
                20,
                RawEvent::Key {
                    scan: 0x48,
                    extended: true,
                    down: false,
                },
            ),
        ];
        let (steps, notes) = keyboard_steps(&events);
        assert_eq!(steps, [tap("Up", 20)]);
        assert_eq!(notes.len(), 1);
    }

    #[test]
    fn stop_hotkey_modifiers_are_removed() {
        let hk = Hotkey::parse("Ctrl+Shift+F9").unwrap();
        let mut events = vec![
            key(0, "LeftCtrl", true),
            key(50, "C", true),
            key(80, "C", false),
            key(100, "LeftCtrl", false),
            key(900, "LeftShift", true),
            key(950, "RightCtrl", true),
            // Auto-repeat of the held modifiers before F9 is pressed.
            key(1450, "RightCtrl", true),
        ];
        trim_stop(&mut events, StopReason::Hotkey, Some(&hk));
        let (steps, _) = keyboard_steps(&events);
        assert_eq!(
            steps,
            [
                KeyStep::KeyDown {
                    key: "LeftCtrl".into()
                },
                wait(50),
                tap("C", 30),
                wait(20),
                KeyStep::KeyUp {
                    key: "LeftCtrl".into()
                },
            ]
        );
    }

    #[test]
    fn ui_stop_drops_the_click_on_stop_and_the_way_there() {
        let mut events = vec![
            ev(0, RawEvent::Cursor { x: 10, y: 10 }),
            ev(
                100,
                RawEvent::Button {
                    button: MouseButton::Left,
                    down: true,
                },
            ),
            ev(
                150,
                RawEvent::Button {
                    button: MouseButton::Left,
                    down: false,
                },
            ),
            ev(400, RawEvent::Cursor { x: 500, y: 500 }),
            ev(450, RawEvent::Cursor { x: 900, y: 900 }),
            ev(
                600,
                RawEvent::Button {
                    button: MouseButton::Left,
                    down: true,
                },
            ),
            ev(
                650,
                RawEvent::Button {
                    button: MouseButton::Left,
                    down: false,
                },
            ),
        ];
        trim_stop(&mut events, StopReason::Ui, None);
        assert_eq!(events.len(), 3);
        assert_eq!(events[2].t_us, 150_000);
    }

    fn screens() -> Screens {
        Screens {
            monitors: vec![
                MonitorRef {
                    name: Some(r"\\.\DISPLAY1".into()),
                    x: 0,
                    y: 0,
                    width: 3440,
                    height: 1440,
                },
                MonitorRef {
                    name: Some(r"\\.\DISPLAY2".into()),
                    x: 3440,
                    y: -1000,
                    width: 1440,
                    height: 3440,
                },
            ],
            primary: Some(0),
        }
    }

    const OPTS: PathOptions = PathOptions {
        tolerance: 2.0,
        sample_interval_ms: 8.0,
    };

    fn click(t: u64, down: bool) -> Recorded {
        ev(
            t,
            RawEvent::Button {
                button: MouseButton::Left,
                down,
            },
        )
    }

    #[test]
    fn absolute_movement_becomes_monitor_relative_paths() {
        let mut events = vec![ev(
            0,
            RawEvent::Cursor {
                x: 3440 + 720,
                y: -1000 + 860,
            },
        )];
        // A straight, even drag on the portrait monitor at 1000 Hz, 100 ms.
        events.push(click(200, true));
        for i in 1..=100 {
            events.push(ev(
                200 + i,
                RawEvent::Cursor {
                    x: 4160 + i as i32,
                    y: -140,
                },
            ));
        }
        events.push(click(320, false));
        let (steps, monitor, notes) = mouse_steps(&events, Movement::Absolute, &screens(), OPTS);
        assert!(notes.is_empty());
        assert_eq!(monitor.unwrap().name.as_deref(), Some(r"\\.\DISPLAY2"));
        assert_eq!(
            steps,
            [
                MouseStep::MoveTo { x: 0.5, y: 0.25 },
                MouseStep::Delay { ms: 200 },
                MouseStep::ButtonDown {
                    button: MouseButton::Left
                },
                MouseStep::Delay { ms: 1 },
                // Simplified to its ends.
                MouseStep::MovePath {
                    points: vec![
                        PathPoint {
                            x: (721.0f64 / 1440.0) as f32,
                            y: 0.25,
                            t: 0
                        },
                        PathPoint {
                            x: (820.0f64 / 1440.0) as f32,
                            y: 0.25,
                            t: 99
                        },
                    ]
                },
                MouseStep::Delay { ms: 20 },
                MouseStep::ButtonUp {
                    button: MouseButton::Left
                },
            ]
        );
    }

    #[test]
    fn relative_movement_accumulates_deltas_and_ignores_cursor() {
        let mut events = vec![ev(0, RawEvent::Cursor { x: 1720, y: 720 })];
        for i in 0..50 {
            events.push(ev(10 + i, RawEvent::RawMove { dx: 4, dy: -1 }));
            // The game re-centres the cursor: positions are meaningless here.
            events.push(ev(10 + i, RawEvent::Cursor { x: 1720, y: 720 }));
        }
        events.push(click(100, true));
        events.push(click(140, false));
        let (steps, monitor, _) = mouse_steps(&events, Movement::Relative, &screens(), OPTS);
        assert_eq!(monitor, None);
        assert_eq!(
            steps,
            [
                MouseStep::MoveRelativePath {
                    points: vec![
                        RelPoint {
                            dx: 4,
                            dy: -1,
                            t: 0
                        },
                        RelPoint {
                            dx: 200,
                            dy: -50,
                            t: 49
                        },
                    ]
                },
                MouseStep::Delay { ms: 41 },
                MouseStep::Click {
                    button: MouseButton::Left,
                    hold_ms: Some(40)
                },
            ]
        );
    }

    #[test]
    fn wheel_accumulates_precise_scrolling_and_buttons_are_released() {
        let events = [
            ev(0, RawEvent::Wheel { delta: 60 }),
            ev(10, RawEvent::Wheel { delta: 60 }),
            ev(20, RawEvent::Wheel { delta: -240 }),
            ev(
                30,
                RawEvent::Button {
                    button: MouseButton::Right,
                    down: true,
                },
            ),
            // A release without a press (pressed before the recording) is dropped.
            ev(
                40,
                RawEvent::Button {
                    button: MouseButton::X1,
                    down: false,
                },
            ),
        ];
        let (steps, ..) = mouse_steps(&events, Movement::Absolute, &screens(), OPTS);
        assert_eq!(
            steps,
            [
                MouseStep::Wheel { delta: 1 },
                MouseStep::Delay { ms: 10 },
                MouseStep::Wheel { delta: -2 },
                MouseStep::Delay { ms: 10 },
                // Held at the end: released.
                MouseStep::Click {
                    button: MouseButton::Right,
                    hold_ms: Some(0)
                },
            ]
        );
    }

    #[test]
    fn finish_builds_the_macro_and_trims_the_stop() {
        let config = RecorderConfig::default();
        let (action, notes) = finish(Finished {
            kind: RecordKind::Keyboard,
            movement: Movement::Absolute,
            events: vec![
                key(0, "A", true),
                key(30, "A", false),
                key(500, "LeftCtrl", true),
                key(520, "LeftShift", true),
            ],
            stop: StopReason::Hotkey,
            config: &config,
            screens: &screens(),
        });
        assert!(notes.is_empty());
        assert_eq!(action, Action::keyboard(vec![tap("A", 30)]));
        let (mouse, _) = finish(Finished {
            kind: RecordKind::Mouse,
            movement: Movement::Relative,
            events: vec![ev(0, RawEvent::RawMove { dx: 1, dy: 1 })],
            stop: StopReason::Ui,
            config: &config,
            screens: &screens(),
        });
        assert_eq!(mouse.describe(), "mouse: relative path 1 points 0ms");
        // Keyboard recordings ignore mouse events and vice versa.
        let (keys, _) = finish(Finished {
            kind: RecordKind::Keyboard,
            movement: Movement::Absolute,
            events: vec![click(0, true), click(10, false)],
            stop: StopReason::Hotkey,
            config: &config,
            screens: &screens(),
        });
        assert_eq!(keys, Action::keyboard(vec![]));
    }

    #[test]
    fn view_json_shape() {
        let view = RecorderView {
            session: 3,
            phase: RecorderPhase::Recording,
            kind: Some(RecordKind::Mouse),
            movement: Movement::Relative,
            result: None,
            error: None,
        };
        assert_eq!(
            serde_json::to_string(&view).unwrap(),
            r#"{"session":3,"phase":"recording","kind":"mouse","movement":"relative","result":null,"error":null}"#
        );
        let config: RecorderConfig = serde_json::from_str(r#"{"stopHotkey":"Alt+F12"}"#).unwrap();
        assert_eq!(config.tolerance, 2.0);
    }
}

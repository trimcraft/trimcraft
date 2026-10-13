//! Triggers: what a virtual control listens to.
//!
//! A [`Trigger`] is an input (a button, a hat direction, an encoder notch or an axis zone),
//! how it is pressed (press, release, long press, double press), modifier buttons that must be
//! held, and conditions on modes ("only when Mode X is in state S").
//!
//! The engine turns input edges into [`Fired`] events and asks each trigger whether it
//! [`matches`](Trigger::matches). Long and double presses need time: [`PressTracker`] works on
//! timestamps and reports the next deadline, so the app can wake exactly then instead of
//! polling.

use crate::device::DeviceId;
use crate::state::HatState;
use serde::{Deserialize, Serialize};

/// Default hold time of a long press.
pub const DEFAULT_LONG_MS: u32 = 500;
/// Default window of a double press (from the first press to the second).
pub const DEFAULT_DOUBLE_MS: u32 = 300;
/// Limits for long press and double press times.
pub const MIN_PRESS_MS: u32 = 50;
pub const MAX_PRESS_MS: u32 = 5_000;
/// An axis leaves a zone only this far (percent of its range) outside it, so a value
/// jittering at the edge does not fire again and again.
pub const ZONE_HYSTERESIS: f32 = 2.0;

const MS: u64 = 1_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum HatDirection {
    Up,
    UpRight,
    Right,
    DownRight,
    Down,
    DownLeft,
    Left,
    UpLeft,
}

impl HatDirection {
    pub fn from_state(state: HatState) -> Option<Self> {
        const U: u8 = HatState::UP;
        const R: u8 = HatState::RIGHT;
        const D: u8 = HatState::DOWN;
        const L: u8 = HatState::LEFT;
        Some(match state.0 {
            x if x == U => Self::Up,
            x if x == U | R => Self::UpRight,
            x if x == R => Self::Right,
            x if x == D | R => Self::DownRight,
            x if x == D => Self::Down,
            x if x == D | L => Self::DownLeft,
            x if x == L => Self::Left,
            x if x == U | L => Self::UpLeft,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StepDirection {
    Decrement,
    Increment,
}

/// The input a trigger listens to.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum TriggerInput {
    Button {
        device: DeviceId,
        index: u16,
    },
    /// A hat direction: pressed while the hat points that way.
    Hat {
        device: DeviceId,
        index: u16,
        direction: HatDirection,
    },
    /// One trimcraft of a configured encoder.
    EncoderStep {
        encoder: String,
        direction: StepDirection,
    },
    /// A range of an axis, in percent of its range (0 = one end, 100 = the other). "Pressed"
    /// while the axis is inside.
    AxisZone {
        device: DeviceId,
        index: u16,
        min: f32,
        max: f32,
    },
}

/// How a button or hat direction must be pressed. For axis zones `Press` is entering and
/// `Release` leaving; encoder notches only know `Press`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum PressKind {
    /// Fires on press. If the same input also has long or double press triggers (in the
    /// active profile), it fires on release instead, as a short press.
    #[default]
    Press,
    Release,
    /// Fires once the input is held this long.
    Long {
        ms: u32,
    },
    /// Fires on the second press within this time from the first.
    Double {
        ms: u32,
    },
}

/// A button that must be held as a modifier.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ButtonRef {
    pub device: DeviceId,
    pub index: u16,
}

/// "Only when mode `mode` is in state `state`" (ids).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Condition {
    pub mode: String,
    pub state: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Trigger {
    #[serde(flatten)]
    pub input: TriggerInput,
    #[serde(default)]
    pub press: PressKind,
    /// Buttons that must be held. With modifiers, the most specific trigger wins: the same
    /// input with fewer (or no) modifiers does not fire.
    #[serde(default)]
    pub modifiers: Vec<ButtonRef>,
    /// All must hold.
    #[serde(default)]
    pub conditions: Vec<Condition>,
}

impl From<TriggerInput> for Trigger {
    fn from(input: TriggerInput) -> Self {
        Self {
            input,
            press: PressKind::Press,
            modifiers: Vec::new(),
            conditions: Vec::new(),
        }
    }
}

/// A button or a hat direction: something with press and release edges.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum DigitalKey {
    Button {
        device: DeviceId,
        index: u16,
    },
    Hat {
        device: DeviceId,
        index: u16,
        direction: HatDirection,
    },
}

/// An axis zone, comparable and hashable (the bounds by their bits).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ZoneKey {
    pub device: DeviceId,
    pub index: u16,
    min: u32,
    max: u32,
}

impl ZoneKey {
    pub fn new(device: DeviceId, index: u16, min: f32, max: f32) -> Self {
        Self {
            device,
            index,
            min: min.to_bits(),
            max: max.to_bits(),
        }
    }

    pub fn min(&self) -> f32 {
        f32::from_bits(self.min)
    }

    pub fn max(&self) -> f32 {
        f32::from_bits(self.max)
    }

    /// Whether `percent` is inside, given whether it was inside before (hysteresis).
    pub fn inside(&self, percent: f32, was_inside: bool) -> bool {
        let (min, max) = (self.min(), self.max());
        if was_inside {
            percent >= min - ZONE_HYSTERESIS && percent <= max + ZONE_HYSTERESIS
        } else {
            percent >= min && percent <= max
        }
    }
}

/// Something with a press and a release that a hold (momentary control, mode shift) can be
/// tied to.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum HoldKey {
    Digital(DigitalKey),
    Zone(ZoneKey),
}

/// What happened on a button or hat direction, as [`PressTracker`] reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PressEvent {
    /// The raw press edge (holds start here).
    Down,
    /// The raw release edge (holds end here; `Release` triggers fire).
    Up,
    /// A press: immediate, or a short press once a long or double press is ruled out.
    Press,
    Long(u32),
    Double(u32),
}

/// An event triggers are matched against.
#[derive(Debug, Clone, PartialEq)]
pub enum Fired {
    Digital(DigitalKey, PressEvent),
    Encoder {
        encoder: String,
        direction: StepDirection,
    },
    Zone {
        zone: ZoneKey,
        entered: bool,
    },
}

impl Fired {
    /// The input a hold started by this event is tied to.
    pub fn hold_key(&self) -> Option<HoldKey> {
        match self {
            Fired::Digital(key, _) => Some(HoldKey::Digital(key.clone())),
            Fired::Zone { zone, .. } => Some(HoldKey::Zone(zone.clone())),
            Fired::Encoder { .. } => None,
        }
    }
}

impl TriggerInput {
    pub fn digital_key(&self) -> Option<DigitalKey> {
        match self {
            TriggerInput::Button { device, index } => Some(DigitalKey::Button {
                device: device.clone(),
                index: *index,
            }),
            TriggerInput::Hat {
                device,
                index,
                direction,
            } => Some(DigitalKey::Hat {
                device: device.clone(),
                index: *index,
                direction: *direction,
            }),
            _ => None,
        }
    }

    pub fn zone_key(&self) -> Option<ZoneKey> {
        match self {
            TriggerInput::AxisZone {
                device,
                index,
                min,
                max,
            } => Some(ZoneKey::new(device.clone(), *index, *min, *max)),
            _ => None,
        }
    }
}

impl Trigger {
    /// Whether this trigger fires on `fired`, the input alone (modifiers and conditions are
    /// checked by the engine). `hold`: the trigger starts a hold (momentary control, mode
    /// shift), so a plain press reacts to the raw press edge, never deferred.
    pub fn matches(&self, fired: &Fired, hold: bool) -> bool {
        match fired {
            Fired::Digital(key, event) => {
                self.input.digital_key().as_ref() == Some(key)
                    && match (self.press, event) {
                        (PressKind::Press, PressEvent::Down) => hold,
                        (PressKind::Press, PressEvent::Press) => !hold,
                        (PressKind::Release, PressEvent::Up) => true,
                        (PressKind::Long { ms }, PressEvent::Long(m)) => ms == *m,
                        (PressKind::Double { ms }, PressEvent::Double(m)) => ms == *m,
                        _ => false,
                    }
            }
            Fired::Encoder { encoder, direction } => {
                self.press == PressKind::Press
                    && matches!(&self.input, TriggerInput::EncoderStep { encoder: e, direction: d }
                        if e == encoder && d == direction)
            }
            Fired::Zone { zone, entered } => {
                self.input.zone_key().as_ref() == Some(zone)
                    && match self.press {
                        PressKind::Press => *entered,
                        PressKind::Release => !*entered,
                        _ => false,
                    }
            }
        }
    }

    /// Checks limits and fixes up the zone (bounds in order, within 0..=100). `hold`: the
    /// trigger starts a hold, so it needs an input with a release.
    pub fn validate(&mut self, hold: bool) -> Result<(), TriggerError> {
        if let TriggerInput::AxisZone { min, max, .. } = &mut self.input {
            if !min.is_finite() || !max.is_finite() {
                return Err(TriggerError::BadZone);
            }
            *min = min.clamp(0.0, 100.0);
            *max = max.clamp(0.0, 100.0);
            if *min > *max {
                std::mem::swap(min, max);
            }
        }
        match (&self.input, self.press) {
            (TriggerInput::EncoderStep { .. }, PressKind::Press) if hold => {
                return Err(TriggerError::NeedsRelease);
            }
            (TriggerInput::EncoderStep { .. }, PressKind::Press) => {}
            (TriggerInput::EncoderStep { .. }, _) => return Err(TriggerError::PressKind),
            (TriggerInput::AxisZone { .. }, PressKind::Long { .. } | PressKind::Double { .. }) => {
                return Err(TriggerError::PressKind);
            }
            (_, PressKind::Long { ms } | PressKind::Double { ms })
                if !(MIN_PRESS_MS..=MAX_PRESS_MS).contains(&ms) =>
            {
                return Err(TriggerError::PressTime);
            }
            _ => {}
        }
        if hold && self.press == PressKind::Release {
            return Err(TriggerError::NeedsRelease);
        }
        let mut seen = Vec::new();
        self.modifiers.retain(|m| {
            let keep = !seen.contains(m);
            seen.push(m.clone());
            keep
        });
        if let TriggerInput::Button { device, index } = &self.input {
            self.modifiers
                .retain(|m| !(m.device == *device && m.index == *index));
        }
        let mut seen = Vec::new();
        self.conditions.retain(|c| {
            let keep = !seen.contains(c);
            seen.push(c.clone());
            keep
        });
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TriggerError {
    #[error("encoder notches and axis zones cannot use long or double presses")]
    PressKind,
    #[error("long and double press times must be between {MIN_PRESS_MS} and {MAX_PRESS_MS} ms")]
    PressTime,
    #[error("an axis zone needs numbers for its bounds")]
    BadZone,
    #[error(
        "held controls (momentary, mode shift) need a button, hat or axis zone pressed, not a release or encoder"
    )]
    NeedsRelease,
}

/// Long and double press times in use for one button or hat direction (in the active
/// profile).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PressConfig {
    pub long: Vec<u32>,
    pub double: Vec<u32>,
}

impl PressConfig {
    /// From the press kinds of all triggers on the input.
    pub fn from_kinds<'a>(kinds: impl Iterator<Item = &'a PressKind>) -> Self {
        let mut cfg = Self::default();
        for kind in kinds {
            match *kind {
                PressKind::Long { ms } => cfg.long.push(ms),
                PressKind::Double { ms } => cfg.double.push(ms),
                _ => {}
            }
        }
        for v in [&mut cfg.long, &mut cfg.double] {
            v.sort_unstable();
            v.dedup();
        }
        cfg
    }

    /// Plain presses wait for the release (short press), so they do not fire together with
    /// a long or double press.
    pub fn deferred(&self) -> bool {
        !self.long.is_empty() || !self.double.is_empty()
    }
}

/// Press, release, long and double presses of one button or hat direction. Times are the
/// backend's nanoseconds.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PressTracker {
    down: bool,
    /// Long presses still to come in this press: (deadline, ms).
    long_pending: Vec<(u64, u32)>,
    /// This press already counted as long or double: no short press.
    consumed: bool,
    /// Start of a first press that a second one could make a double press.
    first_down: Option<u64>,
    /// A short press waiting for the double press window to pass.
    short_due: Option<u64>,
}

impl PressTracker {
    pub fn is_idle(&self) -> bool {
        *self == Self::default()
    }

    pub fn down(&mut self, t: u64, cfg: &PressConfig) -> Vec<PressEvent> {
        let mut out = vec![PressEvent::Down];
        if self.down {
            return out;
        }
        self.down = true;
        if let Some(first) = self.first_down.take() {
            let gap = t.saturating_sub(first);
            let doubles: Vec<u32> = cfg
                .double
                .iter()
                .copied()
                .filter(|&ms| u64::from(ms) * MS >= gap)
                .collect();
            if !doubles.is_empty() {
                self.short_due = None;
                self.consumed = true;
                self.long_pending.clear();
                out.extend(doubles.into_iter().map(PressEvent::Double));
                return out;
            }
            // Too late for a double press: the earlier short press is due now.
            if self.short_due.take().is_some() {
                out.push(PressEvent::Press);
            }
        }
        self.consumed = false;
        if !cfg.deferred() {
            out.push(PressEvent::Press);
        }
        if !cfg.double.is_empty() {
            self.first_down = Some(t);
        }
        self.long_pending = cfg
            .long
            .iter()
            .map(|&ms| (t + u64::from(ms) * MS, ms))
            .collect();
        out
    }

    pub fn up(&mut self, t: u64, cfg: &PressConfig) -> Vec<PressEvent> {
        if !self.down {
            return vec![PressEvent::Up];
        }
        self.down = false;
        self.long_pending.clear();
        let mut out = Vec::new();
        if std::mem::take(&mut self.consumed) {
            self.first_down = None;
        } else if cfg.deferred() {
            match (self.first_down, cfg.double.last()) {
                (Some(first), Some(&window)) => {
                    let due = first + u64::from(window) * MS;
                    if t >= due {
                        self.first_down = None;
                        out.push(PressEvent::Press);
                    } else {
                        self.short_due = Some(due);
                    }
                }
                _ => out.push(PressEvent::Press),
            }
        }
        out.push(PressEvent::Up);
        out
    }

    /// Events whose time has come at `now`.
    pub fn due(&mut self, now: u64) -> Vec<PressEvent> {
        let mut out = Vec::new();
        self.long_pending.sort_unstable();
        while let Some(&(at, ms)) = self.long_pending.first() {
            if at > now {
                break;
            }
            self.long_pending.remove(0);
            self.consumed = true;
            self.first_down = None;
            out.push(PressEvent::Long(ms));
        }
        if self.short_due.is_some_and(|at| at <= now) {
            self.short_due = None;
            self.first_down = None;
            out.push(PressEvent::Press);
        }
        out
    }

    pub fn deadline(&self) -> Option<u64> {
        self.long_pending
            .iter()
            .map(|&(at, _)| at)
            .chain(self.short_due)
            .min()
    }
}

/// Position of an axis value (-1..=1) in percent of its range.
pub fn axis_percent(value: f32) -> f32 {
    ((value.clamp(-1.0, 1.0) + 1.0) * 50.0).clamp(0.0, 100.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use PressEvent::*;

    const T: u64 = MS;

    fn cfg(long: &[u32], double: &[u32]) -> PressConfig {
        PressConfig {
            long: long.to_vec(),
            double: double.to_vec(),
        }
    }

    #[test]
    fn plain_press_fires_on_the_press_edge() {
        let c = cfg(&[], &[]);
        let mut p = PressTracker::default();
        assert_eq!(p.down(0, &c), [Down, Press]);
        assert_eq!(p.deadline(), None);
        assert_eq!(p.up(80 * T, &c), [Up]);
        assert!(p.is_idle());
    }

    #[test]
    fn with_a_long_press_a_short_press_fires_on_release() {
        let c = cfg(&[500], &[]);
        let mut p = PressTracker::default();
        assert_eq!(p.down(0, &c), [Down]);
        assert_eq!(p.deadline(), Some(500 * T));
        assert!(p.due(499 * T).is_empty());
        assert_eq!(p.up(200 * T, &c), [Press, Up]);
        assert_eq!(p.deadline(), None);
        // Held long enough: the long press fires at the deadline, no short press after.
        assert_eq!(p.down(1000 * T, &c), [Down]);
        assert_eq!(p.due(1500 * T), [Long(500)]);
        assert_eq!(p.up(2000 * T, &c), [Up]);
        assert!(p.is_idle());
    }

    #[test]
    fn several_long_thresholds_fire_in_order() {
        let c = cfg(&[500, 1500], &[]);
        let mut p = PressTracker::default();
        p.down(0, &c);
        assert_eq!(p.due(600 * T), [Long(500)]);
        assert_eq!(p.deadline(), Some(1500 * T));
        assert_eq!(p.due(1500 * T), [Long(1500)]);
        assert_eq!(p.up(1600 * T, &c), [Up]);
    }

    #[test]
    fn double_press_and_delayed_short_press() {
        let c = cfg(&[], &[300]);
        let mut p = PressTracker::default();
        assert_eq!(p.down(0, &c), [Down]);
        assert_eq!(p.up(80 * T, &c), [Up]);
        // The short press waits for the window.
        assert_eq!(p.deadline(), Some(300 * T));
        assert_eq!(p.down(250 * T, &c), [Down, Double(300)]);
        assert_eq!(p.up(320 * T, &c), [Up]);
        assert_eq!(p.deadline(), None);
        assert!(p.due(10_000 * T).is_empty());
        // A single press: short press when the window has passed.
        p.down(1000 * T, &c);
        p.up(1100 * T, &c);
        assert!(p.due(1299 * T).is_empty());
        assert_eq!(p.due(1300 * T), [Press]);
        // Released after the window: the short press fires right on release.
        p.down(2000 * T, &c);
        assert_eq!(p.up(2400 * T, &c), [Press, Up]);
        assert!(p.is_idle());
    }

    #[test]
    fn double_windows_count_from_the_first_press() {
        let c = cfg(&[], &[200, 400]);
        let mut p = PressTracker::default();
        p.down(0, &c);
        p.up(50 * T, &c);
        assert_eq!(p.down(300 * T, &c), [Down, Double(400)]);
        p.up(350 * T, &c);
        p.down(1000 * T, &c);
        p.up(1050 * T, &c);
        assert_eq!(p.down(1150 * T, &c), [Down, Double(200), Double(400)]);
    }

    #[test]
    fn long_press_does_not_start_a_double_press() {
        let c = cfg(&[500], &[300]);
        let mut p = PressTracker::default();
        p.down(0, &c);
        // Long press wins at 500 ms (the double window starting at 0 is over anyway).
        assert_eq!(p.due(500 * T), [Long(500)]);
        assert_eq!(p.up(600 * T, &c), [Up]);
        assert_eq!(p.down(700 * T, &c), [Down]);
        assert_eq!(p.up(750 * T, &c), [Up]);
        assert_eq!(p.due(1000 * T), [Press]);
    }

    #[test]
    fn matching() {
        let dev = DeviceId("d".into());
        let key = DigitalKey::Button {
            device: dev.clone(),
            index: 3,
        };
        let mut t: Trigger = TriggerInput::Button {
            device: dev.clone(),
            index: 3,
        }
        .into();
        let fired = |e| Fired::Digital(key.clone(), e);
        assert!(t.matches(&fired(Press), false));
        assert!(!t.matches(&fired(Down), false));
        // Holds react to the raw press, never deferred.
        assert!(t.matches(&fired(Down), true));
        assert!(!t.matches(&fired(Press), true));
        t.press = PressKind::Long { ms: 500 };
        assert!(t.matches(&fired(Long(500)), false));
        assert!(!t.matches(&fired(Long(800)), false));
        t.press = PressKind::Release;
        assert!(t.matches(&fired(Up), false));
        let other = Fired::Digital(
            DigitalKey::Button {
                device: dev.clone(),
                index: 4,
            },
            Up,
        );
        assert!(!t.matches(&other, false));

        let zone: Trigger = TriggerInput::AxisZone {
            device: dev.clone(),
            index: 2,
            min: 80.0,
            max: 100.0,
        }
        .into();
        let zk = ZoneKey::new(dev.clone(), 2, 80.0, 100.0);
        assert!(zone.matches(
            &Fired::Zone {
                zone: zk.clone(),
                entered: true
            },
            false
        ));
        assert!(!zone.matches(
            &Fired::Zone {
                zone: zk,
                entered: false
            },
            false
        ));
        let enc: Trigger = TriggerInput::EncoderStep {
            encoder: "enc-1".into(),
            direction: StepDirection::Increment,
        }
        .into();
        assert!(enc.matches(
            &Fired::Encoder {
                encoder: "enc-1".into(),
                direction: StepDirection::Increment
            },
            false
        ));
    }

    #[test]
    fn zones_have_hysteresis() {
        let z = ZoneKey::new(DeviceId("d".into()), 0, 80.0, 100.0);
        assert!(!z.inside(79.0, false));
        assert!(z.inside(80.0, false));
        assert!(z.inside(78.5, true));
        assert!(!z.inside(77.9, true));
        assert_eq!(axis_percent(-1.0), 0.0);
        assert_eq!(axis_percent(1.0), 100.0);
        assert_eq!(axis_percent(0.0), 50.0);
    }

    #[test]
    fn validation() {
        let dev = DeviceId("d".into());
        let mut zone: Trigger = TriggerInput::AxisZone {
            device: dev.clone(),
            index: 0,
            min: 120.0,
            max: 40.0,
        }
        .into();
        zone.validate(false).unwrap();
        assert_eq!(
            zone.input,
            TriggerInput::AxisZone {
                device: dev.clone(),
                index: 0,
                min: 40.0,
                max: 100.0
            }
        );
        zone.press = PressKind::Long { ms: 500 };
        assert_eq!(zone.validate(false), Err(TriggerError::PressKind));
        let mut b: Trigger = TriggerInput::Button {
            device: dev.clone(),
            index: 1,
        }
        .into();
        b.press = PressKind::Double { ms: 10 };
        assert_eq!(b.validate(false), Err(TriggerError::PressTime));
        b.press = PressKind::Release;
        assert_eq!(b.validate(true), Err(TriggerError::NeedsRelease));
        b.press = PressKind::Press;
        let m = ButtonRef {
            device: dev.clone(),
            index: 5,
        };
        b.modifiers = vec![
            m.clone(),
            m.clone(),
            ButtonRef {
                device: dev.clone(),
                index: 1,
            },
        ];
        b.validate(true).unwrap();
        assert_eq!(b.modifiers, [m]);
        let mut e: Trigger = TriggerInput::EncoderStep {
            encoder: "enc-1".into(),
            direction: StepDirection::Decrement,
        }
        .into();
        assert_eq!(e.validate(true), Err(TriggerError::NeedsRelease));
        e.validate(false).unwrap();
    }

    #[test]
    fn json_shape() {
        // Triggers from before press kinds, modifiers and conditions load as plain presses.
        let old: Trigger =
            serde_json::from_str(r#"{"type":"button","device":"d","index":3}"#).unwrap();
        assert_eq!(old.press, PressKind::Press);
        assert!(old.modifiers.is_empty() && old.conditions.is_empty());
        let t = Trigger {
            input: TriggerInput::Hat {
                device: DeviceId("d".into()),
                index: 0,
                direction: HatDirection::Up,
            },
            press: PressKind::Long { ms: 700 },
            modifiers: vec![ButtonRef {
                device: DeviceId("e".into()),
                index: 2,
            }],
            conditions: vec![Condition {
                mode: "vc-3".into(),
                state: "st-2".into(),
            }],
        };
        let json = serde_json::to_value(&t).unwrap();
        assert_eq!(
            json,
            serde_json::json!({"type":"hat","device":"d","index":0,"direction":"up",
                "press":{"kind":"long","ms":700},
                "modifiers":[{"device":"e","index":2}],
                "conditions":[{"mode":"vc-3","state":"st-2"}]})
        );
        assert_eq!(serde_json::from_value::<Trigger>(json).unwrap(), t);
        let zone: Trigger = serde_json::from_str(
            r#"{"type":"axisZone","device":"d","index":2,"min":80,"max":100,"press":{"kind":"release"}}"#,
        )
        .unwrap();
        assert_eq!(zone.press, PressKind::Release);
    }
}

//! Diagnostics event log: what the app saw and did, in order, for the "Event log" tab.
//!
//! Producers (the engine, the action executor, the connectors, the log bridge) push entries
//! into a [`Diagnostics`] handle. It holds a bounded ring buffer; recording is a short,
//! constant-time critical section, so it never holds up the engine, and the oldest entries are
//! dropped when the buffer is full. The app reads new entries in batches for the UI.

use crate::state::HatState;
use serde::Serialize;
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

/// Entries kept in memory.
pub const CAPACITY: usize = 5000;

/// An axis is logged when it moved this far from the last logged value. Axes run -1.0..=1.0,
/// so 0.04 is 2 % of the full travel.
pub const AXIS_MIN_CHANGE: f32 = 0.04;
/// And at most once per this interval per axis (10 entries per second).
pub const AXIS_MIN_INTERVAL_NS: u64 = 100_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Category {
    /// Button, hat and axis changes and encoder detents.
    Input,
    /// A trigger matched a virtual control.
    Trigger,
    /// A virtual control changed its active state.
    Control,
    /// Actions: queued, executed, skipped, failed.
    Action,
    /// Connector status changes.
    Connector,
    /// Warnings and errors from the app log.
    Log,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Level {
    Info,
    Warn,
    Error,
}

/// What a producer reports; [`Diagnostics::record`] adds the sequence number and timestamp.
#[derive(Debug, Clone, PartialEq)]
pub struct Record {
    pub category: Category,
    pub level: Level,
    /// Stable id and display name of the device involved.
    pub device: Option<(String, String)>,
    pub text: String,
    pub detail: Option<String>,
}

impl Record {
    pub fn new(category: Category, text: impl Into<String>) -> Self {
        Self {
            category,
            level: Level::Info,
            device: None,
            text: text.into(),
            detail: None,
        }
    }

    pub fn level(mut self, level: Level) -> Self {
        self.level = level;
        self
    }

    pub fn device(mut self, id: impl Into<String>, name: impl Into<String>) -> Self {
        self.device = Some((id.into(), name.into()));
        self
    }

    pub fn detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    /// Increases by one per entry, never reused, so the UI can ask for "everything after".
    pub seq: u64,
    /// Milliseconds since the Unix epoch.
    pub time_ms: u64,
    pub category: Category,
    pub level: Level,
    pub device: Option<String>,
    pub device_name: Option<String>,
    pub text: String,
    pub detail: Option<String>,
}

/// Bounded buffer, oldest entries drop first.
#[derive(Debug)]
pub struct Ring {
    entries: VecDeque<Entry>,
    capacity: usize,
    next_seq: u64,
}

impl Ring {
    pub fn new(capacity: usize) -> Self {
        Self {
            entries: VecDeque::new(),
            capacity: capacity.max(1),
            next_seq: 1,
        }
    }

    pub fn push(&mut self, time_ms: u64, record: Record) {
        if self.entries.len() == self.capacity {
            self.entries.pop_front();
        }
        let (device, device_name) = match record.device {
            Some((id, name)) => (Some(id), Some(name)),
            None => (None, None),
        };
        self.entries.push_back(Entry {
            seq: self.next_seq,
            time_ms,
            category: record.category,
            level: record.level,
            device,
            device_name,
            text: record.text,
            detail: record.detail,
        });
        self.next_seq += 1;
    }

    /// Entries with a sequence number above `seq`, oldest first.
    pub fn since(&self, seq: u64) -> Vec<Entry> {
        // Sequence numbers are consecutive, so the wanted entries are a suffix.
        let skip = self.entries.partition_point(|e| e.seq <= seq);
        self.entries.iter().skip(skip).cloned().collect()
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Cheap, cloneable handle to the shared event log.
#[derive(Debug, Clone)]
pub struct Diagnostics {
    ring: Arc<Mutex<Ring>>,
}

impl Default for Diagnostics {
    fn default() -> Self {
        Self::new(CAPACITY)
    }
}

impl Diagnostics {
    pub fn new(capacity: usize) -> Self {
        Self {
            ring: Arc::new(Mutex::new(Ring::new(capacity))),
        }
    }

    fn ring(&self) -> MutexGuard<'_, Ring> {
        self.ring.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn record(&self, record: Record) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX));
        self.ring().push(now, record);
    }

    pub fn since(&self, seq: u64) -> Vec<Entry> {
        self.ring().since(seq)
    }

    pub fn all(&self) -> Vec<Entry> {
        self.since(0)
    }

    pub fn clear(&self) {
        self.ring().clear();
    }
}

/// Decides which axis moves are worth an entry: a change of at least [`AXIS_MIN_CHANGE`] since
/// the last logged value, and at most one entry per [`AXIS_MIN_INTERVAL_NS`] per axis.
#[derive(Debug, Default)]
pub struct AxisThrottle {
    last: HashMap<(String, u16), (f32, u64)>,
}

impl AxisThrottle {
    /// `time_ns` is the event's monotonic time. Returns the previously logged value when the
    /// move should be logged (`Some(None)`: first value of this axis).
    pub fn admit(
        &mut self,
        device: &str,
        axis: u16,
        value: f32,
        time_ns: u64,
    ) -> Option<Option<f32>> {
        let key = (device.to_owned(), axis);
        match self.last.get(&key) {
            None => {
                self.last.insert(key, (value, time_ns));
                Some(None)
            }
            Some(&(previous, at)) => {
                if (value - previous).abs() < AXIS_MIN_CHANGE
                    || time_ns.saturating_sub(at) < AXIS_MIN_INTERVAL_NS
                {
                    return None;
                }
                self.last.insert(key, (value, time_ns));
                Some(Some(previous))
            }
        }
    }

    /// Forget an axis, e.g. when its device reconnects.
    pub fn reset_device(&mut self, device: &str) {
        self.last.retain(|(d, _), _| d != device);
    }
}

// --- Formatting ------------------------------------------------------------------------------
//
// Buttons, axes and hats are shown 1-based, like Windows and the rest of the UI do.

pub fn button_text(index: u16, pressed: bool) -> String {
    format!(
        "Button {} {}",
        u32::from(index) + 1,
        if pressed { "pressed" } else { "released" }
    )
}

pub fn axis_text(index: u16, value: f32) -> String {
    format!("Axis {}: {:+.1} %", u32::from(index) + 1, value * 100.0)
}

pub fn axis_detail(value: f32, previous: Option<f32>) -> String {
    match previous {
        Some(p) => format!("value {value:+.4} (range -1..1), last logged {p:+.4}"),
        None => format!("value {value:+.4} (range -1..1), first value logged"),
    }
}

pub fn hat_direction(state: HatState) -> &'static str {
    match state.0 & 0x0f {
        0 => "centered",
        HatState::UP => "up",
        HatState::RIGHT => "right",
        HatState::DOWN => "down",
        HatState::LEFT => "left",
        m if m == HatState::UP | HatState::RIGHT => "up-right",
        m if m == HatState::DOWN | HatState::RIGHT => "down-right",
        m if m == HatState::DOWN | HatState::LEFT => "down-left",
        m if m == HatState::UP | HatState::LEFT => "up-left",
        _ => "invalid",
    }
}

pub fn hat_text(index: u16, previous: HatState, state: HatState) -> String {
    format!(
        "Hat {}: {} → {}",
        u32::from(index) + 1,
        hat_direction(previous),
        hat_direction(state)
    )
}

pub fn encoder_text(name: &str, increment: bool) -> String {
    format!(
        "Encoder {name}: detent {}",
        if increment { "+" } else { "−" }
    )
}

/// A short text for a state change, e.g. "Landing gear: up → down".
pub fn state_change_text(control: &str, from: Option<&str>, to: Option<&str>) -> String {
    format!(
        "{control}: {} → {}",
        from.unwrap_or("(none)"),
        to.unwrap_or("(none)")
    )
}

pub fn duration_text(ms: f64) -> String {
    if ms >= 1000.0 {
        format!("{:.2} s", ms / 1000.0)
    } else if ms >= 10.0 {
        format!("{ms:.0} ms")
    } else {
        format!("{ms:.1} ms")
    }
}

/// What happened to an action, for [`action_record`].
#[derive(Debug, Clone, PartialEq)]
pub enum ActionEvent {
    Queued,
    /// A macro ran to the end.
    Executed {
        ms: f64,
    },
    /// Handed to a connector, which runs it on its own thread.
    Handed {
        connector: String,
    },
    Skipped {
        reason: String,
    },
    Failed {
        error: String,
    },
}

/// `origin` is where the action came from ("Comms / B (activate)"), `description` is
/// `Action::describe`.
pub fn action_record(event: &ActionEvent, origin: &str, description: &str) -> Record {
    let record = |text: String, level| {
        Record::new(Category::Action, text)
            .level(level)
            .detail(format!("{origin}: {description}"))
    };
    match event {
        ActionEvent::Queued => record(format!("Queued: {description}"), Level::Info),
        ActionEvent::Executed { ms } => record(
            format!("Executed in {}: {description}", duration_text(*ms)),
            Level::Info,
        ),
        ActionEvent::Handed { connector } => {
            record(format!("Sent to {connector}: {description}"), Level::Info)
        }
        ActionEvent::Skipped { reason } => {
            record(format!("Skipped ({reason}): {description}"), Level::Warn)
        }
        ActionEvent::Failed { error } => {
            record(format!("Failed ({error}): {description}"), Level::Error)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(text: &str) -> Record {
        Record::new(Category::Input, text)
    }

    #[test]
    fn ring_drops_the_oldest_entries_when_full() {
        let mut ring = Ring::new(3);
        for i in 0..5 {
            ring.push(i, rec(&format!("e{i}")));
        }
        let all = ring.since(0);
        assert_eq!(ring.len(), 3);
        assert_eq!(
            all.iter().map(|e| e.text.as_str()).collect::<Vec<_>>(),
            ["e2", "e3", "e4"]
        );
        assert_eq!(all[0].seq, 3);
    }

    #[test]
    fn since_returns_only_newer_entries_even_after_drops_and_clear() {
        let mut ring = Ring::new(3);
        for i in 0..4 {
            ring.push(i, rec("x"));
        }
        // Held entries: seq 2, 3, 4.
        assert_eq!(ring.since(3).len(), 1);
        assert_eq!(ring.since(4).len(), 0);
        assert_eq!(ring.since(0).len(), 3);
        ring.clear();
        assert!(ring.is_empty());
        ring.push(9, rec("after clear"));
        // Sequence numbers keep counting, so a reader's cursor stays valid.
        assert_eq!(ring.since(4).len(), 1);
        assert_eq!(ring.since(4)[0].seq, 5);
    }

    #[test]
    fn handle_records_with_timestamp_and_device() {
        let d = Diagnostics::new(10);
        let other = d.clone();
        other.record(
            Record::new(Category::Action, "queued")
                .level(Level::Warn)
                .device("231d:0200#1", "Stick")
                .detail("more"),
        );
        let all = d.all();
        assert_eq!(all.len(), 1);
        assert!(all[0].time_ms > 1_600_000_000_000);
        assert_eq!(all[0].device.as_deref(), Some("231d:0200#1"));
        assert_eq!(all[0].device_name.as_deref(), Some("Stick"));
        assert_eq!(all[0].level, Level::Warn);
        d.clear();
        assert!(d.all().is_empty());
    }

    #[test]
    fn entries_serialise_for_the_ui() {
        let d = Diagnostics::new(2);
        d.record(rec("hello").detail("d"));
        let json = serde_json::to_value(&d.all()[0]).unwrap();
        assert_eq!(json["category"], "input");
        assert_eq!(json["level"], "info");
        assert_eq!(json["text"], "hello");
        assert_eq!(json["detail"], "d");
        assert!(json["timeMs"].is_u64());
        assert!(json["deviceName"].is_null());
    }

    const MS: u64 = 1_000_000;

    #[test]
    fn axis_first_value_is_logged_then_small_moves_are_not() {
        let mut t = AxisThrottle::default();
        assert_eq!(t.admit("d", 0, 0.0, 0), Some(None));
        // 1 % change: ignored, however late.
        assert_eq!(t.admit("d", 0, 0.01, 500 * MS), None);
        // Still measured from the last *logged* value, not the last seen one.
        assert_eq!(t.admit("d", 0, 0.03, 600 * MS), None);
        assert_eq!(t.admit("d", 0, 0.05, 700 * MS), Some(Some(0.0)));
    }

    #[test]
    fn axis_is_limited_to_ten_entries_per_second() {
        let mut t = AxisThrottle::default();
        let mut logged = 0;
        // A fast sweep: a new value every millisecond for one second.
        for i in 0..1000u64 {
            let value = -1.0 + 2.0 * (i as f32) / 1000.0;
            if t.admit("d", 0, value, i * MS).is_some() {
                logged += 1;
            }
        }
        assert!((9..=10).contains(&logged), "logged {logged}");
    }

    #[test]
    fn axes_and_devices_are_throttled_independently() {
        let mut t = AxisThrottle::default();
        assert!(t.admit("a", 0, 0.0, 0).is_some());
        assert!(t.admit("a", 1, 0.0, 0).is_some());
        assert!(t.admit("b", 0, 0.0, 0).is_some());
        // Same instant, big change, but inside the interval of that axis.
        assert!(t.admit("a", 0, 0.5, 10 * MS).is_none());
        assert!(t.admit("a", 1, 0.5, 200 * MS).is_some());
        t.reset_device("a");
        assert_eq!(t.admit("a", 0, 0.5, 210 * MS), Some(None));
    }

    #[test]
    fn formats_input_events_one_based() {
        assert_eq!(button_text(0, true), "Button 1 pressed");
        assert_eq!(button_text(4, false), "Button 5 released");
        assert_eq!(axis_text(1, 0.4535), "Axis 2: +45.3 %");
        assert_eq!(axis_text(0, -1.0), "Axis 1: -100.0 %");
        assert_eq!(
            hat_text(
                0,
                HatState::CENTERED,
                HatState(HatState::UP | HatState::RIGHT)
            ),
            "Hat 1: centered → up-right"
        );
        assert_eq!(hat_direction(HatState(HatState::LEFT)), "left");
        assert_eq!(
            hat_direction(HatState(HatState::UP | HatState::DOWN)),
            "invalid"
        );
        assert_eq!(encoder_text("Radio", true), "Encoder Radio: detent +");
        assert_eq!(encoder_text("Radio", false), "Encoder Radio: detent −");
    }

    #[test]
    fn formats_state_changes_and_durations() {
        assert_eq!(
            state_change_text("Gear", Some("up"), Some("down")),
            "Gear: up → down"
        );
        assert_eq!(
            state_change_text("Gear", None, Some("up")),
            "Gear: (none) → up"
        );
        assert_eq!(duration_text(0.42), "0.4 ms");
        assert_eq!(duration_text(87.6), "88 ms");
        assert_eq!(duration_text(1530.0), "1.53 s");
        assert!(axis_detail(0.5, None).contains("first value"));
        assert!(axis_detail(0.5, Some(0.25)).contains("last logged +0.2500"));
    }

    #[test]
    fn formats_action_events() {
        let origin = "Comms / B (activate)";
        let desc = "keys: tap F1";
        let queued = action_record(&ActionEvent::Queued, origin, desc);
        assert_eq!(queued.text, "Queued: keys: tap F1");
        assert_eq!(
            queued.detail.as_deref(),
            Some("Comms / B (activate): keys: tap F1")
        );
        assert_eq!(queued.category, Category::Action);

        let done = action_record(&ActionEvent::Executed { ms: 12.4 }, origin, desc);
        assert_eq!(done.text, "Executed in 12 ms: keys: tap F1");
        let sent = action_record(
            &ActionEvent::Handed {
                connector: "Soundpad".into(),
            },
            origin,
            desc,
        );
        assert_eq!(sent.text, "Sent to Soundpad: keys: tap F1");

        let skipped = action_record(
            &ActionEvent::Skipped {
                reason: "connection disabled".into(),
            },
            origin,
            desc,
        );
        assert_eq!(skipped.level, Level::Warn);
        assert!(skipped.text.starts_with("Skipped (connection disabled)"));

        let failed = action_record(
            &ActionEvent::Failed {
                error: "SendInput blocked".into(),
            },
            origin,
            desc,
        );
        assert_eq!(failed.level, Level::Error);
        assert!(failed.text.contains("SendInput blocked"));
    }
}

//! Rotary encoders that report each detent as a short button pulse.
//!
//! Many devices report an incremental encoder as two buttons: one pulses once per detent when
//! turned one way, the other when turned the other way. Nothing in the HID report says so, so
//! the [`EncoderDetector`] infers it from behaviour alone, without vendor knowledge:
//!
//! * a pulse is a press shorter than [`DetectorConfig::max_pulse_ms`];
//! * an encoder button fires pulses in quick runs and is never held long;
//! * the two buttons of one encoder are on the same device and never pressed together.
//!
//! The thresholds are deliberately generous defaults. Every button's measured pulse lengths are
//! kept in [`PulseStats`] so the user can see the real values of their hardware.

use crate::device::DeviceId;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};

/// A pair of buttons used as one rotary encoder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EncoderBinding {
    pub id: String,
    pub name: String,
    pub device: DeviceId,
    /// Button that pulses when turned towards smaller values.
    pub decrement: u16,
    /// Button that pulses when turned towards larger values.
    pub increment: u16,
}

impl EncoderBinding {
    pub fn uses(&self, device: &DeviceId, button: u16) -> bool {
        &self.device == device && (self.decrement == button || self.increment == button)
    }

    /// +1, -1 or 0 for a button press on this encoder.
    pub fn direction(&self, device: &DeviceId, button: u16) -> i64 {
        if &self.device != device {
            0
        } else if button == self.increment {
            1
        } else if button == self.decrement {
            -1
        } else {
            0
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DetectorConfig {
    /// Presses shorter than this count as pulses.
    pub max_pulse_ms: f64,
    /// Pulses closer together than this belong to one run (one turn of the knob).
    pub max_run_gap_ms: f64,
    /// Pulses in one run before a button counts as encoder-like.
    pub min_run_length: u32,
}

impl Default for DetectorConfig {
    fn default() -> Self {
        Self {
            max_pulse_ms: 200.0,
            max_run_gap_ms: 600.0,
            min_run_length: 3,
        }
    }
}

/// Measured behaviour of one button, kept for every button that was ever pressed.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PulseStats {
    pub presses: u32,
    pub pulses: u32,
    pub min_pulse_ms: Option<f64>,
    pub max_pulse_ms: Option<f64>,
    pub avg_pulse_ms: Option<f64>,
    pub longest_run: u32,
    /// Pressed while another button of the same device was down.
    pub overlaps: u32,
    /// Ever held longer than a pulse.
    pub held: bool,
    #[serde(skip)]
    pulse_sum_ms: f64,
    #[serde(skip)]
    pressed_at_ns: Option<u64>,
    #[serde(skip)]
    last_pulse_start_ns: Option<u64>,
    #[serde(skip)]
    current_run: u32,
}

impl PulseStats {
    fn encoder_like(&self, cfg: &DetectorConfig) -> bool {
        !self.held && self.longest_run >= cfg.min_run_length
    }
}

/// A detected, not yet confirmed encoder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EncoderSuggestion {
    pub device: DeviceId,
    pub decrement: u16,
    pub increment: u16,
}

impl EncoderSuggestion {
    fn key(&self) -> (DeviceId, u16, u16) {
        (self.device.clone(), self.decrement, self.increment)
    }
}

#[derive(Debug, Default)]
pub struct EncoderDetector {
    pub config: DetectorConfig,
    stats: BTreeMap<(DeviceId, u16), PulseStats>,
    /// Buttons currently down, per device.
    down: BTreeMap<DeviceId, HashSet<u16>>,
    /// Pairs that ever were pressed at the same time.
    overlapping_pairs: HashSet<(DeviceId, u16, u16)>,
}

impl EncoderDetector {
    pub fn new(config: DetectorConfig) -> Self {
        Self {
            config,
            ..Default::default()
        }
    }

    /// Feeds one button edge. Timestamps are monotonic nanoseconds.
    pub fn observe(&mut self, device: &DeviceId, button: u16, pressed: bool, time_ns: u64) {
        let down = self.down.entry(device.clone()).or_default();
        let stats = self.stats.entry((device.clone(), button)).or_default();
        if pressed {
            stats.presses += 1;
            stats.pressed_at_ns = Some(time_ns);
            let others: Vec<u16> = down.iter().copied().filter(|&b| b != button).collect();
            if !others.is_empty() {
                stats.overlaps += 1;
            }
            for other in others {
                let (a, b) = (other.min(button), other.max(button));
                self.overlapping_pairs.insert((device.clone(), a, b));
            }
            down.insert(button);
            return;
        }

        down.remove(&button);
        let Some(start) = stats.pressed_at_ns.take() else {
            return;
        };
        let duration_ms = time_ns.saturating_sub(start) as f64 / 1e6;
        if duration_ms > self.config.max_pulse_ms {
            stats.held = true;
            stats.current_run = 0;
            return;
        }
        stats.pulses += 1;
        stats.pulse_sum_ms += duration_ms;
        stats.min_pulse_ms = Some(
            stats
                .min_pulse_ms
                .map_or(duration_ms, |m| m.min(duration_ms)),
        );
        stats.max_pulse_ms = Some(
            stats
                .max_pulse_ms
                .map_or(duration_ms, |m| m.max(duration_ms)),
        );
        stats.avg_pulse_ms = Some(stats.pulse_sum_ms / f64::from(stats.pulses));
        let in_run = stats.last_pulse_start_ns.is_some_and(|prev| {
            start.saturating_sub(prev) as f64 / 1e6 <= self.config.max_run_gap_ms
        });
        stats.current_run = if in_run { stats.current_run + 1 } else { 1 };
        stats.longest_run = stats.longest_run.max(stats.current_run);
        stats.last_pulse_start_ns = Some(start);
    }

    pub fn stats(&self, device: &DeviceId, button: u16) -> Option<&PulseStats> {
        self.stats.get(&(device.clone(), button))
    }

    pub fn all_stats(&self) -> impl Iterator<Item = (&DeviceId, u16, &PulseStats)> {
        self.stats.iter().map(|((d, b), s)| (d, *b, s))
    }

    /// Buttons that behave like one half of an encoder, not yet paired.
    pub fn half_detected(&self, bound: &[EncoderBinding]) -> Vec<(DeviceId, u16)> {
        let paired: HashSet<(DeviceId, u16)> = self
            .suggestions(bound, &HashSet::new())
            .into_iter()
            .flat_map(|s| [(s.device.clone(), s.decrement), (s.device, s.increment)])
            .collect();
        self.encoder_like_buttons(bound)
            .into_iter()
            .filter(|k| !paired.contains(k))
            .collect()
    }

    /// Pairs of encoder-like buttons. Within a device, the closest button indices are paired,
    /// lower index as decrement; the user can swap directions.
    pub fn suggestions(
        &self,
        bound: &[EncoderBinding],
        dismissed: &HashSet<(DeviceId, u16, u16)>,
    ) -> Vec<EncoderSuggestion> {
        let mut by_device: BTreeMap<DeviceId, Vec<u16>> = BTreeMap::new();
        for (device, button) in self.encoder_like_buttons(bound) {
            by_device.entry(device).or_default().push(button);
        }

        let mut out = Vec::new();
        for (device, mut buttons) in by_device {
            buttons.sort_unstable();
            let mut used: HashSet<u16> = HashSet::new();
            // Greedy: repeatedly pair the closest remaining buttons.
            loop {
                let free: Vec<u16> = buttons
                    .iter()
                    .copied()
                    .filter(|b| !used.contains(b))
                    .collect();
                let mut best: Option<(u16, u16)> = None;
                for (i, &a) in free.iter().enumerate() {
                    for &b in &free[i + 1..] {
                        if self.overlapping_pairs.contains(&(device.clone(), a, b)) {
                            continue;
                        }
                        if best.is_none_or(|(x, y)| (b - a, a) < (y - x, x)) {
                            best = Some((a, b));
                        }
                    }
                }
                let Some((a, b)) = best else { break };
                used.insert(a);
                used.insert(b);
                let suggestion = EncoderSuggestion {
                    device: device.clone(),
                    decrement: a,
                    increment: b,
                };
                if !dismissed.contains(&suggestion.key()) {
                    out.push(suggestion);
                }
            }
        }
        out
    }

    /// Forgets measurements, e.g. after the user changed thresholds.
    pub fn reset(&mut self) {
        self.stats.clear();
        self.overlapping_pairs.clear();
    }

    fn encoder_like_buttons(&self, bound: &[EncoderBinding]) -> Vec<(DeviceId, u16)> {
        self.stats
            .iter()
            .filter(|((d, b), s)| {
                s.encoder_like(&self.config) && !bound.iter().any(|e| e.uses(d, *b))
            })
            .map(|((d, b), _)| (d.clone(), *b))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MS: u64 = 1_000_000;

    fn dev() -> DeviceId {
        DeviceId("dev".into())
    }

    /// Turns the knob: `count` pulses of `pulse_ms` on `button`, every `period_ms`.
    fn turn(
        det: &mut EncoderDetector,
        button: u16,
        count: u64,
        start_ms: u64,
        period_ms: u64,
        pulse_ms: u64,
    ) {
        for i in 0..count {
            let t = (start_ms + i * period_ms) * MS;
            det.observe(&dev(), button, true, t);
            det.observe(&dev(), button, false, t + pulse_ms * MS);
        }
    }

    #[test]
    fn two_pulsing_buttons_are_suggested_as_encoder() {
        let mut det = EncoderDetector::default();
        turn(&mut det, 12, 5, 0, 80, 30);
        turn(&mut det, 13, 4, 2_000, 80, 30);
        let s = det.suggestions(&[], &HashSet::new());
        assert_eq!(
            s,
            [EncoderSuggestion {
                device: dev(),
                decrement: 12,
                increment: 13
            }]
        );

        let stats = det.stats(&dev(), 12).unwrap();
        assert_eq!(stats.pulses, 5);
        assert_eq!(stats.longest_run, 5);
        assert_eq!(stats.min_pulse_ms, Some(30.0));
    }

    #[test]
    fn single_direction_is_reported_as_half() {
        let mut det = EncoderDetector::default();
        turn(&mut det, 7, 4, 0, 100, 20);
        assert!(det.suggestions(&[], &HashSet::new()).is_empty());
        assert_eq!(det.half_detected(&[]), [(dev(), 7)]);
    }

    #[test]
    fn slow_human_taps_are_not_a_run() {
        let mut det = EncoderDetector::default();
        turn(&mut det, 1, 5, 0, 1_500, 60);
        turn(&mut det, 2, 5, 10_000, 1_500, 60);
        assert!(det.suggestions(&[], &HashSet::new()).is_empty());
    }

    #[test]
    fn a_held_button_is_never_an_encoder() {
        let mut det = EncoderDetector::default();
        turn(&mut det, 1, 5, 0, 80, 30);
        turn(&mut det, 2, 5, 1_000, 80, 30);
        det.observe(&dev(), 2, true, 5_000 * MS);
        det.observe(&dev(), 2, false, 6_000 * MS);
        assert!(det.stats(&dev(), 2).unwrap().held);
        assert!(det.suggestions(&[], &HashSet::new()).is_empty());
    }

    #[test]
    fn buttons_pressed_together_are_not_paired() {
        let mut det = EncoderDetector::default();
        turn(&mut det, 1, 5, 0, 80, 30);
        turn(&mut det, 2, 5, 1_000, 80, 30);
        det.observe(&dev(), 1, true, 3_000 * MS);
        det.observe(&dev(), 2, true, 3_010 * MS);
        det.observe(&dev(), 2, false, 3_020 * MS);
        det.observe(&dev(), 1, false, 3_030 * MS);
        assert!(det.suggestions(&[], &HashSet::new()).is_empty());
    }

    #[test]
    fn closest_buttons_pair_up_and_bound_or_dismissed_are_skipped() {
        let mut det = EncoderDetector::default();
        for (i, b) in [20u16, 21, 30, 31].into_iter().enumerate() {
            turn(&mut det, b, 4, i as u64 * 1_000, 80, 25);
        }
        let all = det.suggestions(&[], &HashSet::new());
        assert_eq!(all.len(), 2);
        assert_eq!((all[0].decrement, all[0].increment), (20, 21));
        assert_eq!((all[1].decrement, all[1].increment), (30, 31));

        let bound = [EncoderBinding {
            id: "e".into(),
            name: "Radio".into(),
            device: dev(),
            decrement: 20,
            increment: 21,
        }];
        let dismissed = HashSet::from([(dev(), 30, 31)]);
        assert!(det.suggestions(&bound, &dismissed).is_empty());
    }

    #[test]
    fn direction_follows_binding() {
        let e = EncoderBinding {
            id: "e".into(),
            name: "n".into(),
            device: dev(),
            decrement: 4,
            increment: 5,
        };
        assert_eq!(e.direction(&dev(), 5), 1);
        assert_eq!(e.direction(&dev(), 4), -1);
        assert_eq!(e.direction(&dev(), 6), 0);
        assert_eq!(e.direction(&DeviceId("other".into()), 5), 0);
    }
}

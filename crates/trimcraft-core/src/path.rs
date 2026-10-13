//! Mouse paths: thinning and simplifying recorded movement, the playback schedule, and the
//! mapping from monitor-relative positions to `SendInput`'s absolute coordinates.
//!
//! Positions are stored as fractions of a monitor's width and height (like overlay widget
//! positions), so a macro survives resolution changes. At playback they are turned into
//! pixels of the monitor, then into the 0..=65535 range `SendInput` expects for
//! `MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK`, which spans the whole virtual desktop.

use crate::overlay::{MonitorRef, find_monitor};

/// A position at a time: pixels or mickeys, and milliseconds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sample {
    pub x: f64,
    pub y: f64,
    pub t: f64,
}

impl Sample {
    pub fn new(x: f64, y: f64, t: f64) -> Self {
        Self { x, y, t }
    }
}

/// Drops samples that follow the last kept one by less than `min_interval_ms` (high-rate mice
/// report up to 8000 times a second). The first and the last sample are always kept.
pub fn cap_rate(samples: &[Sample], min_interval_ms: f64) -> Vec<Sample> {
    let mut out: Vec<Sample> = Vec::with_capacity(samples.len());
    for (i, s) in samples.iter().enumerate() {
        let last = i + 1 == samples.len();
        match out.last() {
            Some(prev) if !last && s.t - prev.t < min_interval_ms => {}
            _ => out.push(*s),
        }
    }
    out
}

/// Ramer–Douglas–Peucker on position and time: a sample is dropped when it lies within
/// `tolerance` of where the simplified path is *at that moment* (synchronised Euclidean
/// distance). So straight, even moves collapse to their ends, while turns, changes of speed
/// and pauses keep their waypoints. First and last samples are always kept.
pub fn simplify(samples: &[Sample], tolerance: f64) -> Vec<Sample> {
    if samples.len() <= 2 {
        return samples.to_vec();
    }
    let mut keep = vec![false; samples.len()];
    keep[0] = true;
    keep[samples.len() - 1] = true;
    let mut stack = vec![(0, samples.len() - 1)];
    while let Some((a, b)) = stack.pop() {
        let (mut worst, mut worst_d) = (0, 0.0);
        for i in a + 1..b {
            let d = synchronised_distance(&samples[a], &samples[b], &samples[i]);
            if d > worst_d {
                (worst, worst_d) = (i, d);
            }
        }
        if worst_d > tolerance {
            keep[worst] = true;
            stack.push((a, worst));
            stack.push((worst, b));
        }
    }
    samples
        .iter()
        .zip(keep)
        .filter_map(|(s, k)| k.then_some(*s))
        .collect()
}

/// Distance between `p` and the point on segment `a`–`b` at `p`'s time.
fn synchronised_distance(a: &Sample, b: &Sample, p: &Sample) -> f64 {
    let span = b.t - a.t;
    let f = if span > 0.0 {
        ((p.t - a.t) / span).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let x = a.x + (b.x - a.x) * f;
    let y = a.y + (b.y - a.y) * f;
    (p.x - x).hypot(p.y - y)
}

/// Positions to send while playing `waypoints` at `speed` (2.0 is twice as fast): every
/// waypoint, plus linear steps in between so no gap is longer than `tick_ms`. Times are
/// milliseconds from the first waypoint, already scaled by the speed.
pub fn schedule(waypoints: &[Sample], speed: f64, tick_ms: f64) -> Vec<Sample> {
    let speed = if speed > 0.0 { speed } else { 1.0 };
    let Some(first) = waypoints.first() else {
        return Vec::new();
    };
    let mut out = vec![Sample::new(first.x, first.y, 0.0)];
    for pair in waypoints.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let start = (a.t - first.t) / speed;
        let duration = ((b.t - a.t) / speed).max(0.0);
        let steps = (duration / tick_ms).ceil().max(1.0) as usize;
        for k in 1..=steps {
            let f = k as f64 / steps as f64;
            out.push(Sample::new(
                a.x + (b.x - a.x) * f,
                a.y + (b.y - a.y) * f,
                start + duration * f,
            ));
        }
    }
    out
}

/// A rectangle in physical pixels of the virtual desktop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl Rect {
    fn of(m: &MonitorRef) -> Self {
        Self {
            x: m.x,
            y: m.y,
            width: m.width,
            height: m.height,
        }
    }

    fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x
            && y >= self.y
            && i64::from(x) < i64::from(self.x) + i64::from(self.width)
            && i64::from(y) < i64::from(self.y) + i64::from(self.height)
    }
}

/// The connected monitors, as needed to record and play absolute mouse positions.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Screens {
    pub monitors: Vec<MonitorRef>,
    pub primary: Option<usize>,
}

impl Screens {
    fn primary_or_first(&self) -> Option<&MonitorRef> {
        self.primary
            .and_then(|i| self.monitors.get(i))
            .or(self.monitors.first())
    }

    /// The monitor a macro plays on: the saved one if it is connected (found like the overlay
    /// monitor), otherwise the primary monitor.
    pub fn target(&self, saved: Option<&MonitorRef>) -> Option<&MonitorRef> {
        saved
            .and_then(|s| find_monitor(s, &self.monitors))
            .and_then(|i| self.monitors.get(i))
            .or_else(|| self.primary_or_first())
    }

    /// The monitor showing pixel `x`,`y`, or the primary one if none does.
    pub fn monitor_at(&self, x: i32, y: i32) -> Option<&MonitorRef> {
        self.monitors
            .iter()
            .find(|m| Rect::of(m).contains(x, y))
            .or_else(|| self.primary_or_first())
    }

    /// Bounding box of all monitors (Windows' virtual screen).
    pub fn desktop(&self) -> Option<Rect> {
        let mut it = self.monitors.iter().map(Rect::of);
        let first = it.next()?;
        let (mut x0, mut y0) = (i64::from(first.x), i64::from(first.y));
        let (mut x1, mut y1) = (x0 + i64::from(first.width), y0 + i64::from(first.height));
        for r in it {
            x0 = x0.min(i64::from(r.x));
            y0 = y0.min(i64::from(r.y));
            x1 = x1.max(i64::from(r.x) + i64::from(r.width));
            y1 = y1.max(i64::from(r.y) + i64::from(r.height));
        }
        Some(Rect {
            x: x0 as i32,
            y: y0 as i32,
            width: (x1 - x0) as u32,
            height: (y1 - y0) as u32,
        })
    }

    /// Pixel position as `SendInput` absolute coordinates over the virtual desktop
    /// (0..=65535 on both axes). Windows maps `n` back to pixel `n * size / 65536`, so the
    /// smallest `n` that lands on the pixel is used. Positions off the desktop are clamped.
    pub fn to_absolute(&self, px: f64, py: f64) -> Option<(i32, i32)> {
        let d = self.desktop()?;
        let axis = |p: f64, origin: i32, size: u32| -> i32 {
            if size == 0 {
                return 0;
            }
            let size = i64::from(size);
            let offset = (p.round() as i64 - i64::from(origin)).clamp(0, size - 1);
            ((offset * 65536 + size - 1) / size).min(65535) as i32
        };
        Some((axis(px, d.x, d.width), axis(py, d.y, d.height)))
    }
}

/// Pixel position as fractions of the monitor's width and height.
pub fn to_fraction(m: &MonitorRef, px: f64, py: f64) -> (f32, f32) {
    let f = |p: f64, origin: i32, size: u32| {
        if size == 0 {
            0.0
        } else {
            ((p - f64::from(origin)) / f64::from(size)) as f32
        }
    };
    (f(px, m.x, m.width), f(py, m.y, m.height))
}

/// Fractions of the monitor back to a pixel position.
pub fn from_fraction(m: &MonitorRef, fx: f32, fy: f32) -> (f64, f64) {
    (
        f64::from(m.x) + f64::from(fx) * f64::from(m.width),
        f64::from(m.y) + f64::from(fy) * f64::from(m.height),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(x: f64, y: f64, t: f64) -> Sample {
        Sample::new(x, y, t)
    }

    #[test]
    fn rate_cap_keeps_ends_and_spacing() {
        let samples: Vec<Sample> = (0..=20).map(|i| s(i as f64, 0.0, i as f64)).collect();
        let capped = cap_rate(&samples, 8.0);
        let times: Vec<f64> = capped.iter().map(|p| p.t).collect();
        assert_eq!(times, [0.0, 8.0, 16.0, 20.0]);
        assert_eq!(cap_rate(&samples[..1], 8.0).len(), 1);
        assert!(cap_rate(&[], 8.0).is_empty());
    }

    #[test]
    fn straight_even_move_collapses_to_its_ends() {
        let samples: Vec<Sample> = (0..=100)
            .map(|i| s(i as f64 * 3.0, i as f64, i as f64 * 2.0))
            .collect();
        let simple = simplify(&samples, 1.0);
        assert_eq!(simple, [samples[0], samples[100]]);
    }

    #[test]
    fn corners_are_kept() {
        // Right 100 px, then down 100 px, at constant speed.
        let mut samples: Vec<Sample> = (0..=10)
            .map(|i| s(i as f64 * 10.0, 0.0, i as f64))
            .collect();
        samples.extend((1..=10).map(|i| s(100.0, i as f64 * 10.0, 10.0 + i as f64)));
        let simple = simplify(&samples, 2.0);
        assert_eq!(
            simple,
            [s(0.0, 0.0, 0.0), s(100.0, 0.0, 10.0), s(100.0, 100.0, 20.0)]
        );
    }

    #[test]
    fn pauses_are_kept_because_time_counts() {
        // Straight line, but the cursor waits 500 ms halfway: geometrically redundant, yet
        // dropping the middle point would change the timing.
        let samples = [
            s(0.0, 0.0, 0.0),
            s(50.0, 0.0, 50.0),
            s(50.0, 0.0, 550.0),
            s(100.0, 0.0, 600.0),
        ];
        let simple = simplify(&samples, 2.0);
        assert_eq!(simple, samples);
    }

    #[test]
    fn tolerance_controls_detail() {
        let wobble: Vec<Sample> = (0..=40)
            .map(|i| {
                s(
                    i as f64 * 5.0,
                    if i % 2 == 0 { 0.0 } else { 3.0 },
                    i as f64 * 10.0,
                )
            })
            .collect();
        assert_eq!(simplify(&wobble, 5.0).len(), 2);
        assert!(simplify(&wobble, 1.0).len() > 20);
    }

    #[test]
    fn schedule_fills_gaps_and_scales_time() {
        let points = [
            s(0.0, 0.0, 100.0),
            s(100.0, 0.0, 132.0),
            s(100.0, 50.0, 132.0),
        ];
        let plan = schedule(&points, 1.0, 8.0);
        let times: Vec<f64> = plan.iter().map(|p| p.t).collect();
        assert_eq!(times, [0.0, 8.0, 16.0, 24.0, 32.0, 32.0]);
        assert_eq!(plan[2], s(50.0, 0.0, 16.0));
        assert_eq!(plan.last(), Some(&s(100.0, 50.0, 32.0)));
        // Twice as fast: half the time, fewer steps.
        let fast = schedule(&points, 2.0, 8.0);
        assert_eq!(
            fast.iter().map(|p| p.t).collect::<Vec<_>>(),
            [0.0, 8.0, 16.0, 16.0]
        );
        // Half speed.
        assert_eq!(schedule(&points, 0.5, 8.0)[8].t, 64.0);
        assert!(schedule(&[], 1.0, 8.0).is_empty());
    }

    fn mon(name: &str, x: i32, y: i32, width: u32, height: u32) -> MonitorRef {
        MonitorRef {
            name: Some(name.into()),
            x,
            y,
            width,
            height,
        }
    }

    /// The two-monitor setup Trimcraft is developed against: an ultrawide and a portrait one.
    fn screens() -> Screens {
        Screens {
            monitors: vec![
                mon(r"\\.\DISPLAY1", 0, 0, 3440, 1440),
                mon(r"\\.\DISPLAY2", 3440, -1000, 1440, 3440),
            ],
            primary: Some(0),
        }
    }

    #[test]
    fn desktop_is_the_union_of_monitors() {
        assert_eq!(
            screens().desktop(),
            Some(Rect {
                x: 0,
                y: -1000,
                width: 4880,
                height: 3440
            })
        );
        assert_eq!(Screens::default().desktop(), None);
    }

    #[test]
    fn absolute_coordinates_land_on_the_pixel() {
        let sc = screens();
        let d = sc.desktop().unwrap();
        // Windows maps n back to n * size / 65536 (truncated).
        let back = |n: i32, origin: i32, size: u32| {
            origin + (i64::from(n) * i64::from(size) / 65536) as i32
        };
        for &(px, py) in &[
            (0, -1000),
            (0, 0),
            (1719, 719),
            (3439, 1439),
            (3440, 0),
            (4879, 2439),
        ] {
            let (nx, ny) = sc.to_absolute(f64::from(px), f64::from(py)).unwrap();
            assert!((0..=65535).contains(&nx) && (0..=65535).contains(&ny));
            assert_eq!(back(nx, d.x, d.width), px, "x for {px}");
            assert_eq!(back(ny, d.y, d.height), py, "y for {py}");
        }
        // Off the desktop: clamped to its edge pixels.
        assert_eq!(
            sc.to_absolute(-50.0, 99_999.0),
            Some((0, sc.to_absolute(0.0, 2439.0).unwrap().1))
        );
        // A single 1920x1080 monitor.
        let one = Screens {
            monitors: vec![mon("A", 0, 0, 1920, 1080)],
            primary: Some(0),
        };
        for px in [0, 1, 959, 960, 1918, 1919] {
            let (nx, _) = one.to_absolute(f64::from(px), 0.0).unwrap();
            assert_eq!(back(nx, 0, 1920), px);
        }
    }

    #[test]
    fn fractions_round_trip_per_monitor() {
        let sc = screens();
        let portrait = &sc.monitors[1];
        let (fx, fy) = to_fraction(portrait, 3440.0 + 720.0, -1000.0 + 860.0);
        assert_eq!((fx, fy), (0.5, 0.25));
        assert_eq!(from_fraction(portrait, fx, fy), (4160.0, -140.0));
    }

    #[test]
    fn monitors_are_found_for_points_and_saved_refs() {
        let sc = screens();
        assert_eq!(sc.monitor_at(100, 100).unwrap().x, 0);
        assert_eq!(sc.monitor_at(4000, 2000).unwrap().x, 3440);
        // Outside every monitor: the primary one.
        assert_eq!(sc.monitor_at(-5, -5).unwrap().x, 0);
        let saved = mon(r"\\.\DISPLAY2", 3440, -1000, 1440, 3440);
        assert_eq!(sc.target(Some(&saved)).unwrap().x, 3440);
        // Not connected any more: the primary monitor.
        let gone = mon(r"\\.\DISPLAY9", -1920, 0, 1920, 1080);
        assert_eq!(sc.target(Some(&gone)).unwrap().x, 0);
        assert_eq!(sc.target(None).unwrap().x, 0);
        assert_eq!(Screens::default().target(None), None);
    }
}

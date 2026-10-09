//! Synthetic devices for developing without hardware: two identical throttles without serial
//! numbers, a stick, a panel whose rotary encoder is turned now and then, and a vJoy device.

use crate::InputBackend;
use detent_core::{BackendEvent, Fingerprint, HatState, RawDevice};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

#[derive(Debug, Default)]
pub struct MockBackend;

const TICK: Duration = Duration::from_millis(10);

/// `shape` is (axes, buttons, hats).
fn device(
    instance: u32,
    (vid, pid): (u16, u16),
    serial: Option<&str>,
    path: &str,
    name: &str,
    (axes, buttons, hats): (usize, usize, usize),
) -> RawDevice {
    RawDevice {
        instance,
        fingerprint: Fingerprint {
            vendor_id: vid,
            product_id: pid,
            serial: serial.map(Into::into),
            path: Some(path.into()),
            name: name.into(),
        },
        backend_guid: format!("mock-{vid:04x}{pid:04x}"),
        axes: vec![0.0; axes],
        buttons: vec![false; buttons],
        hats: vec![HatState::CENTERED; hats],
    }
}

impl InputBackend for MockBackend {
    fn name(&self) -> &'static str {
        "mock"
    }

    fn run(self: Box<Self>, events: Sender<BackendEvent>, stop: Arc<AtomicBool>) {
        let devices = vec![
            device(
                1,
                (0xf00d, 0x0001),
                Some("MOCK-1"),
                "mock://usb/1",
                "Mock Stick",
                (4, 32, 2),
            ),
            device(
                2,
                (0xf00d, 0x0002),
                None,
                "mock://usb/2",
                "Mock Throttle",
                (3, 24, 1),
            ),
            device(
                3,
                (0xf00d, 0x0002),
                None,
                "mock://usb/3",
                "Mock Throttle",
                (3, 24, 1),
            ),
            device(
                4,
                (0xf00d, 0x0003),
                None,
                "mock://usb/4",
                "Mock Panel",
                (0, 16, 0),
            ),
            device(
                5,
                (0x1234, 0xbead),
                None,
                "mock://vjoy/1",
                "vJoy Device",
                (8, 32, 1),
            ),
        ];
        if events.send(BackendEvent::Initial(devices)).is_err() {
            return;
        }

        let start = Instant::now();
        let mut tick: u64 = 0;
        while !stop.load(Ordering::Relaxed) {
            std::thread::sleep(TICK);
            let t = start.elapsed().as_secs_f32();
            let now = u64::try_from(start.elapsed().as_nanos()).unwrap_or(u64::MAX);
            let mut out = vec![
                BackendEvent::Axis {
                    instance: 1,
                    index: 0,
                    value: (t * 0.9).sin() * 0.8,
                    time_ns: now,
                },
                BackendEvent::Axis {
                    instance: 1,
                    index: 1,
                    value: (t * 1.3).cos() * 0.6,
                    time_ns: now,
                },
                BackendEvent::Axis {
                    instance: 2,
                    index: 2,
                    value: (t * 0.2).sin(),
                    time_ns: now,
                },
                BackendEvent::Axis {
                    instance: 3,
                    index: 2,
                    value: (t * 0.25).cos(),
                    time_ns: now,
                },
            ];
            if tick % 100 == 50 {
                let pressed = (tick / 100) % 2 == 1;
                out.push(BackendEvent::Button {
                    instance: 1,
                    index: 0,
                    pressed,
                    time_ns: now,
                });
            }
            if tick % 150 == 75 {
                let hat = [
                    HatState::UP,
                    HatState::RIGHT,
                    HatState::DOWN,
                    HatState::LEFT,
                    0,
                ][(tick / 150 % 5) as usize];
                out.push(BackendEvent::Hat {
                    instance: 1,
                    index: 0,
                    state: HatState(hat),
                    time_ns: now,
                });
            }
            // Encoder on the panel: every 4 s a run of 5 detents, alternating direction.
            // Pulses last 30 ms, 90 ms apart.
            let phase = tick % 400;
            if phase < 45 && phase % 9 < 2 {
                let button = if (tick / 400).is_multiple_of(2) {
                    11
                } else {
                    10
                };
                out.push(BackendEvent::Button {
                    instance: 4,
                    index: button,
                    pressed: phase.is_multiple_of(9),
                    time_ns: now,
                });
            }
            for event in out {
                if events.send(event).is_err() {
                    return;
                }
            }
            tick += 1;
        }
    }
}

//! Prints every device and input event, to check a backend against real hardware.
//!
//! `cargo run -p trimcraft-input --example probe -- 30` listens for 30 seconds.
//! `TRIMCRAFT_INPUT=mock` uses the synthetic devices instead.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};
use trimcraft_core::{BackendEvent, DeviceRegistry, Engine, Setup};

fn main() {
    let seconds: u64 = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(10);
    let backend = trimcraft_input::spawn(trimcraft_input::default_backend());
    println!("backend: {}, listening {seconds}s", backend.name);

    let mut engine = Engine::new(
        DeviceRegistry::default(),
        Setup::default(),
        &BTreeMap::new(),
    );
    let until = Instant::now() + Duration::from_secs(seconds);
    while Instant::now() < until {
        let Ok(event) = backend.events.recv_timeout(Duration::from_millis(100)) else {
            continue;
        };
        let describe = match &event {
            BackendEvent::Initial(devs) => Some(format!("initial: {} device(s)", devs.len())),
            BackendEvent::Added(d) => Some(format!("added: {}", d.fingerprint.name)),
            BackendEvent::Removed { instance } => Some(format!("removed: instance {instance}")),
            BackendEvent::Button {
                instance,
                index,
                pressed,
                time_ns,
            } => Some(format!(
                "button {index} {} (instance {instance}, t={:.1} ms)",
                if *pressed { "down" } else { "up" },
                *time_ns as f64 / 1e6
            )),
            BackendEvent::Hat {
                instance,
                index,
                state,
                ..
            } => Some(format!("hat {index} = {} (instance {instance})", state.0)),
            BackendEvent::Failed(msg) => Some(format!("FAILED: {msg}")),
            BackendEvent::Axis { .. } => None,
        };
        let devices_changed = matches!(
            event,
            BackendEvent::Initial(_) | BackendEvent::Added(_) | BackendEvent::Removed { .. }
        );
        engine.apply(event);
        if let Some(line) = describe {
            println!("{line}");
        }
        if devices_changed {
            for d in &engine.snapshot().live.devices {
                println!(
                    "  {} {:?} \"{}\" axes={} buttons={} hats={} serial={:?} path={:?} identity={:?} connected={}",
                    d.id,
                    d.kind,
                    d.name,
                    d.state.axes.len(),
                    d.state.buttons.len(),
                    d.state.hats.len(),
                    d.fingerprint.serial,
                    d.fingerprint.path,
                    d.confidence,
                    d.connected
                );
            }
        }
    }
    let snap = engine.snapshot();
    for s in snap.suggestions {
        println!(
            "encoder suggestion: {} buttons {} / {}",
            s.device, s.decrement, s.increment
        );
    }
}

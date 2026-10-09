//! SDL3 joystick backend. All SDL calls happen on the backend thread.
//!
//! Events are consumed one by one as SDL delivers them (button edges are never sampled), so a
//! short encoder pulse is seen as long as the OS reports it.

use crate::InputBackend;
use detent_core::{BackendEvent, Fingerprint, HatState, RawDevice};
use sdl3_sys::everything::*;
use std::collections::HashMap;
use std::ffi::{CStr, c_char};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;

#[derive(Debug, Default)]
pub struct SdlBackend;

/// How long one wait for events may block, so the stop flag is noticed promptly.
const WAIT_MS: i32 = 20;

impl InputBackend for SdlBackend {
    fn name(&self) -> &'static str {
        "sdl3"
    }

    fn run(self: Box<Self>, events: Sender<BackendEvent>, stop: Arc<AtomicBool>) {
        // SAFETY: SDL is initialised, used and shut down on this thread only.
        unsafe { run(events, stop) }
    }
}

unsafe fn run(events: Sender<BackendEvent>, stop: Arc<AtomicBool>) {
    unsafe {
        // The game, not Detent, has focus while playing.
        SDL_SetHint(SDL_HINT_JOYSTICK_ALLOW_BACKGROUND_EVENTS, c"1".as_ptr());
        // Windows: process device messages on SDL's own thread, we have no window loop here.
        SDL_SetHint(SDL_HINT_JOYSTICK_THREAD, c"1".as_ptr());

        if !SDL_Init(SDL_INIT_JOYSTICK) {
            let _ = events.send(BackendEvent::Failed(format!(
                "SDL_Init failed: {}",
                error()
            )));
            return;
        }
        log::info!("SDL {} joystick subsystem started", version());

        let mut open: HashMap<u32, *mut SDL_Joystick> = HashMap::new();
        let mut initial = Vec::new();
        let mut count = 0;
        let ids = SDL_GetJoysticks(&mut count);
        if !ids.is_null() {
            for i in 0..usize::try_from(count).unwrap_or(0) {
                let id = *ids.add(i);
                if let Some(raw) = open_device(id, &mut open) {
                    initial.push(raw);
                }
            }
            SDL_free(ids.cast());
        }
        if events.send(BackendEvent::Initial(initial)).is_err() {
            shutdown(&mut open);
            return;
        }

        let mut event = SDL_Event::default();
        while !stop.load(Ordering::Relaxed) {
            if !SDL_WaitEventTimeout(&mut event, WAIT_MS) {
                continue;
            }
            loop {
                if let Some(out) = translate(&event, &mut open)
                    && events.send(out).is_err()
                {
                    shutdown(&mut open);
                    return;
                }
                if !SDL_PollEvent(&mut event) {
                    break;
                }
            }
        }
        shutdown(&mut open);
    }
}

unsafe fn translate(
    event: &SDL_Event,
    open: &mut HashMap<u32, *mut SDL_Joystick>,
) -> Option<BackendEvent> {
    unsafe {
        let ty = SDL_EventType(event.r#type);
        match ty {
            SDL_EVENT_JOYSTICK_ADDED => {
                let id = event.jdevice.which;
                if open.contains_key(&id.0) {
                    return None; // already reported in the initial batch
                }
                open_device(id, open).map(BackendEvent::Added)
            }
            SDL_EVENT_JOYSTICK_REMOVED => {
                let id = event.jdevice.which.0;
                let joystick = open.remove(&id)?;
                SDL_CloseJoystick(joystick);
                Some(BackendEvent::Removed { instance: id })
            }
            SDL_EVENT_JOYSTICK_AXIS_MOTION => {
                let e = event.jaxis;
                Some(BackendEvent::Axis {
                    instance: e.which.0,
                    index: u16::from(e.axis),
                    value: normalise_axis(e.value),
                    time_ns: e.timestamp,
                })
            }
            SDL_EVENT_JOYSTICK_BUTTON_DOWN | SDL_EVENT_JOYSTICK_BUTTON_UP => {
                let e = event.jbutton;
                Some(BackendEvent::Button {
                    instance: e.which.0,
                    index: u16::from(e.button),
                    pressed: e.down,
                    time_ns: e.timestamp,
                })
            }
            SDL_EVENT_JOYSTICK_HAT_MOTION => {
                let e = event.jhat;
                Some(BackendEvent::Hat {
                    instance: e.which.0,
                    index: u16::from(e.hat),
                    state: HatState(e.value),
                    time_ns: e.timestamp,
                })
            }
            _ => None,
        }
    }
}

unsafe fn open_device(
    id: SDL_JoystickID,
    open: &mut HashMap<u32, *mut SDL_Joystick>,
) -> Option<RawDevice> {
    unsafe {
        let joystick = SDL_OpenJoystick(id);
        if joystick.is_null() {
            log::warn!("could not open joystick {}: {}", id.0, error());
            return None;
        }
        open.insert(id.0, joystick);

        let axes = usize::try_from(SDL_GetNumJoystickAxes(joystick)).unwrap_or(0);
        let buttons = usize::try_from(SDL_GetNumJoystickButtons(joystick)).unwrap_or(0);
        let hats = usize::try_from(SDL_GetNumJoystickHats(joystick)).unwrap_or(0);

        let mut guid = [0 as c_char; 33];
        SDL_GUIDToString(
            SDL_GetJoystickGUID(joystick),
            guid.as_mut_ptr(),
            guid.len() as i32,
        );

        let raw = RawDevice {
            instance: id.0,
            fingerprint: Fingerprint {
                vendor_id: SDL_GetJoystickVendor(joystick),
                product_id: SDL_GetJoystickProduct(joystick),
                serial: string(SDL_GetJoystickSerial(joystick)),
                path: string(SDL_GetJoystickPath(joystick)),
                name: string(SDL_GetJoystickName(joystick))
                    .unwrap_or_else(|| "Unnamed device".into()),
            },
            backend_guid: string(guid.as_ptr()).unwrap_or_default(),
            axes: (0..axes)
                .map(|i| normalise_axis(SDL_GetJoystickAxis(joystick, i as i32)))
                .collect(),
            buttons: (0..buttons)
                .map(|i| SDL_GetJoystickButton(joystick, i as i32))
                .collect(),
            hats: (0..hats)
                .map(|i| HatState(SDL_GetJoystickHat(joystick, i as i32)))
                .collect(),
        };
        log::info!(
            "opened {} ({:04x}:{:04x}) axes={axes} buttons={buttons} hats={hats} serial={:?} path={:?}",
            raw.fingerprint.name,
            raw.fingerprint.vendor_id,
            raw.fingerprint.product_id,
            raw.fingerprint.serial,
            raw.fingerprint.path,
        );
        Some(raw)
    }
}

unsafe fn shutdown(open: &mut HashMap<u32, *mut SDL_Joystick>) {
    unsafe {
        for (_, joystick) in open.drain() {
            SDL_CloseJoystick(joystick);
        }
        SDL_Quit();
    }
}

fn normalise_axis(value: i16) -> f32 {
    (f32::from(value) / 32767.0).clamp(-1.0, 1.0)
}

unsafe fn string(ptr: *const c_char) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    // SAFETY: SDL returns NUL-terminated strings valid until the next call on this thread.
    let s = unsafe { CStr::from_ptr(ptr) }
        .to_string_lossy()
        .trim()
        .to_owned();
    (!s.is_empty()).then_some(s)
}

fn error() -> String {
    // SAFETY: SDL_GetError always returns a valid string.
    unsafe { string(SDL_GetError()) }.unwrap_or_else(|| "unknown error".into())
}

fn version() -> String {
    let v = SDL_GetVersion();
    format!("{}.{}.{}", v / 1_000_000, v / 1_000 % 1_000, v % 1_000)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn axis_range_is_symmetric() {
        assert_eq!(normalise_axis(i16::MAX), 1.0);
        assert_eq!(normalise_axis(i16::MIN), -1.0);
        assert_eq!(normalise_axis(0), 0.0);
    }
}

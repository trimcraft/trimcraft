//! Runs actions (keyboard and mouse macros) on a worker thread of its own, so a macro with
//! delays never blocks the engine thread. Requests run strictly in the order they arrive.
//! Soundpad commands are handed on to the Soundpad thread, so a slow or missing Soundpad
//! never holds up the queue.
//!
//! On Windows input is synthesised with `SendInput`, keys as scan codes so games reading
//! DirectInput or RawInput see them. Other platforms only log what would be sent, so the app
//! still builds and runs for development.

use crate::soundpad::Soundpad;
use detent_core::{Action, ActionRequest, InputOp, Phase};
use std::sync::Arc;
use std::sync::mpsc::{Sender, channel};
use std::time::Duration;

pub struct Executor {
    queue: Sender<ActionRequest>,
}

impl Executor {
    pub fn spawn(soundpad: Arc<Soundpad>) -> Self {
        let (queue, requests) = channel::<ActionRequest>();
        std::thread::Builder::new()
            .name("detent-actions".into())
            .spawn(move || {
                for request in requests {
                    run(&request, &soundpad);
                }
            })
            .expect("spawn action thread");
        Self { queue }
    }

    pub fn submit(&self, requests: Vec<ActionRequest>) {
        for request in requests {
            if self.queue.send(request).is_err() {
                log::error!("action thread is gone; action dropped");
            }
        }
    }
}

fn run(request: &ActionRequest, soundpad: &Soundpad) {
    let phase = match request.phase {
        Phase::Activate => "activate",
        Phase::Deactivate => "deactivate",
    };
    log::info!(
        "action: {} / {} ({phase}): {}",
        request.control,
        request.state,
        request.action.describe()
    );
    if let Action::Soundpad { command } = &request.action {
        soundpad.submit(command.clone());
        return;
    }
    let (ops, unknown) = request.action.expand();
    for key in unknown {
        log::warn!("action: unknown key \"{key}\" skipped");
    }
    // Consecutive inputs go out in one call, so nothing else can interleave with them.
    let mut batch = Vec::new();
    for op in ops {
        if let InputOp::Sleep { ms } = op {
            platform::send(&batch);
            batch.clear();
            std::thread::sleep(Duration::from_millis(u64::from(ms)));
        } else {
            batch.push(op);
        }
    }
    platform::send(&batch);
}

#[cfg(windows)]
mod platform {
    use detent_core::{InputOp, MouseButton};
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBDINPUT, KEYEVENTF_EXTENDEDKEY,
        KEYEVENTF_KEYUP, KEYEVENTF_SCANCODE, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP,
        MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP, MOUSEEVENTF_MOVE, MOUSEEVENTF_RIGHTDOWN,
        MOUSEEVENTF_RIGHTUP, MOUSEEVENTF_WHEEL, MOUSEEVENTF_XDOWN, MOUSEEVENTF_XUP, MOUSEINPUT,
        SendInput,
    };

    const XBUTTON1: u32 = 1;
    const XBUTTON2: u32 = 2;
    const WHEEL_DELTA: i32 = 120;
    /// Marks input Detent synthesised ("DTNT"), so it can be told apart later.
    const EXTRA_INFO: usize = 0x4454_4E54;

    fn keyboard(scan: u16, extended: bool, down: bool) -> INPUT {
        let mut flags = KEYEVENTF_SCANCODE;
        if extended {
            flags |= KEYEVENTF_EXTENDEDKEY;
        }
        if !down {
            flags |= KEYEVENTF_KEYUP;
        }
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: 0,
                    wScan: scan,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: EXTRA_INFO,
                },
            },
        }
    }

    fn mouse(dx: i32, dy: i32, data: u32, flags: u32) -> INPUT {
        INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 {
                mi: MOUSEINPUT {
                    dx,
                    dy,
                    mouseData: data,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: EXTRA_INFO,
                },
            },
        }
    }

    fn to_input(op: &InputOp) -> Option<INPUT> {
        Some(match *op {
            InputOp::Key {
                scan,
                extended,
                down,
            } => keyboard(scan, extended, down),
            InputOp::MouseButton { button, down } => {
                let (flags, data) = match (button, down) {
                    (MouseButton::Left, true) => (MOUSEEVENTF_LEFTDOWN, 0),
                    (MouseButton::Left, false) => (MOUSEEVENTF_LEFTUP, 0),
                    (MouseButton::Right, true) => (MOUSEEVENTF_RIGHTDOWN, 0),
                    (MouseButton::Right, false) => (MOUSEEVENTF_RIGHTUP, 0),
                    (MouseButton::Middle, true) => (MOUSEEVENTF_MIDDLEDOWN, 0),
                    (MouseButton::Middle, false) => (MOUSEEVENTF_MIDDLEUP, 0),
                    (MouseButton::X1, true) => (MOUSEEVENTF_XDOWN, XBUTTON1),
                    (MouseButton::X1, false) => (MOUSEEVENTF_XUP, XBUTTON1),
                    (MouseButton::X2, true) => (MOUSEEVENTF_XDOWN, XBUTTON2),
                    (MouseButton::X2, false) => (MOUSEEVENTF_XUP, XBUTTON2),
                };
                mouse(0, 0, data, flags)
            }
            // mouseData is a DWORD holding a signed value.
            InputOp::Wheel { delta } => mouse(
                0,
                0,
                delta.saturating_mul(WHEEL_DELTA) as u32,
                MOUSEEVENTF_WHEEL,
            ),
            InputOp::Move { dx, dy } => mouse(dx, dy, 0, MOUSEEVENTF_MOVE),
            InputOp::Sleep { .. } => return None,
        })
    }

    pub fn send(ops: &[InputOp]) {
        let inputs: Vec<INPUT> = ops.iter().filter_map(to_input).collect();
        if inputs.is_empty() {
            return;
        }
        let count = u32::try_from(inputs.len()).unwrap_or(u32::MAX);
        let size = i32::try_from(std::mem::size_of::<INPUT>()).unwrap_or(i32::MAX);
        // SAFETY: `inputs` is a valid, initialised array of `count` INPUT structures that
        // outlives the call, and `size` is the size of one structure.
        let sent = unsafe { SendInput(count, inputs.as_ptr(), size) };
        if sent != count {
            // Typically UIPI: the foreground window belongs to a process with higher integrity
            // (a game running as administrator).
            log::warn!(
                "SendInput delivered {sent} of {count} inputs (error {})",
                std::io::Error::last_os_error()
            );
        }
    }
}

#[cfg(not(windows))]
mod platform {
    use detent_core::InputOp;

    pub fn send(ops: &[InputOp]) {
        if !ops.is_empty() {
            log::info!("action: not sent on this platform: {ops:?}");
        }
    }
}

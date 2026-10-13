//! Runs actions (keyboard and mouse macros) on a worker thread of its own, so a macro with
//! delays never blocks the engine thread. Requests run strictly in the order they arrive.
//! Soundpad and Discord commands are handed on to their connector threads, so a slow or
//! missing program never holds up the queue.
//!
//! Input is synthesised with `SendInput`, keys as scan codes so games reading DirectInput or
//! RawInput see them.

use crate::discord::Discord;
use crate::overlay;
use crate::soundpad::Soundpad;
use detent_core::diagnostics::action_record;
use detent_core::{Action, ActionEvent, ActionRequest, Diagnostics, InputOp, Phase};
use std::sync::Arc;
use std::sync::mpsc::{Sender, channel};
use std::time::{Duration, Instant};
use tauri::AppHandle;

pub struct Executor {
    queue: Sender<ActionRequest>,
    diagnostics: Diagnostics,
}

impl Executor {
    pub fn spawn(
        app: AppHandle,
        soundpad: Arc<Soundpad>,
        discord: Arc<Discord>,
        diagnostics: Diagnostics,
    ) -> Self {
        let log = diagnostics.clone();
        let (queue, requests) = channel::<ActionRequest>();
        std::thread::Builder::new()
            .name("detent-actions".into())
            .spawn(move || {
                for request in requests {
                    run(&app, &request, &soundpad, &discord, &log);
                }
            })
            .expect("spawn action thread");
        Self { queue, diagnostics }
    }

    pub fn submit(&self, requests: Vec<ActionRequest>) {
        for request in requests {
            self.diagnostics.record(action_record(
                &ActionEvent::Queued,
                &origin(&request),
                &request.action.describe(),
            ));
            if self.queue.send(request).is_err() {
                log::error!("action thread is gone; action dropped");
            }
        }
    }
}

/// Where an action came from, e.g. "Comms / B (activate)".
pub fn origin(request: &ActionRequest) -> String {
    let phase = match request.phase {
        Phase::Activate => "activate",
        Phase::Deactivate => "deactivate",
    };
    format!("{} / {} ({phase})", request.control, request.state)
}

fn run(
    app: &AppHandle,
    request: &ActionRequest,
    soundpad: &Soundpad,
    discord: &Discord,
    diagnostics: &Diagnostics,
) {
    let origin = origin(request);
    let description = request.action.describe();
    let record = |event: ActionEvent| {
        diagnostics.record(action_record(&event, &origin, &description));
    };
    log::info!("action: {origin}: {description}");
    if let Action::Soundpad { command } = &request.action {
        soundpad.submit(command.clone());
        record(ActionEvent::Handed {
            connector: "Soundpad".into(),
        });
        return;
    }
    if let Action::Discord { command } = &request.action {
        discord.submit(*command);
        record(ActionEvent::Handed {
            connector: "Discord".into(),
        });
        return;
    }
    // Absolute moves need the current monitor layout; it is queried per macro, so a changed
    // arrangement is picked up.
    let screens = if request.action.needs_screens() {
        overlay::screens(app)
    } else {
        None
    };
    let (ops, warnings) = request.action.expand(screens.as_ref());
    for warning in warnings {
        log::warn!("action: {warning}");
    }
    // Consecutive inputs go out in one call, so nothing else can interleave with them. Waits
    // are measured from the macro's start, so the time spent sending (and the sleep timer's
    // granularity) does not add up over a long recorded path.
    let start = Instant::now();
    let mut due = Duration::ZERO;
    let mut batch = Vec::new();
    let mut failure = None;
    let mut send = |batch: &[InputOp]| {
        if let Err(e) = platform::send(batch) {
            failure.get_or_insert(e);
        }
    };
    for op in ops {
        if let InputOp::Sleep { ms } = op {
            send(&batch);
            batch.clear();
            due += Duration::from_millis(u64::from(ms));
            if let Some(wait) = due.checked_sub(start.elapsed()) {
                std::thread::sleep(wait);
            }
        } else {
            batch.push(op);
        }
    }
    send(&batch);
    match failure {
        None => record(ActionEvent::Executed {
            ms: start.elapsed().as_secs_f64() * 1000.0,
        }),
        Some(error) => record(ActionEvent::Failed { error }),
    }
}

mod platform {
    use detent_core::{InputOp, MouseButton};
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBDINPUT, KEYEVENTF_EXTENDEDKEY,
        KEYEVENTF_KEYUP, KEYEVENTF_SCANCODE, MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_LEFTDOWN,
        MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP, MOUSEEVENTF_MOVE,
        MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP, MOUSEEVENTF_VIRTUALDESK, MOUSEEVENTF_WHEEL,
        MOUSEEVENTF_XDOWN, MOUSEEVENTF_XUP, MOUSEINPUT, SendInput,
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
            InputOp::MoveAbsolute { x, y } => mouse(
                x,
                y,
                0,
                MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK,
            ),
            InputOp::Sleep { .. } => return None,
        })
    }

    pub fn send(ops: &[InputOp]) -> Result<(), String> {
        let inputs: Vec<INPUT> = ops.iter().filter_map(to_input).collect();
        if inputs.is_empty() {
            return Ok(());
        }
        let count = u32::try_from(inputs.len()).unwrap_or(u32::MAX);
        let size = i32::try_from(std::mem::size_of::<INPUT>()).unwrap_or(i32::MAX);
        // SAFETY: `inputs` is a valid, initialised array of `count` INPUT structures that
        // outlives the call, and `size` is the size of one structure.
        let sent = unsafe { SendInput(count, inputs.as_ptr(), size) };
        if sent != count {
            // Typically UIPI: the foreground window belongs to a process with higher integrity
            // (a game running as administrator).
            let error = format!(
                "SendInput delivered {sent} of {count} inputs (error {})",
                std::io::Error::last_os_error()
            );
            log::warn!("{error}");
            return Err(error);
        }
        Ok(())
    }
}

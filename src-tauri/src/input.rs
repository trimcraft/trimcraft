//! Synthesised keyboard and mouse input (`SendInput`), shared by the action executor and the
//! macro player. Keys go out as scan codes, so games reading DirectInput or RawInput see them.

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
        // Media keys: by virtual-key code, flagged extended like a keyboard's media keys.
        InputOp::VirtualKey { vk, down } => INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: vk,
                    wScan: 0,
                    dwFlags: KEYEVENTF_EXTENDEDKEY | if down { 0 } else { KEYEVENTF_KEYUP },
                    time: 0,
                    dwExtraInfo: EXTRA_INFO,
                },
            },
        },
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

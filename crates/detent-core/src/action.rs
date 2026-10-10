//! Actions: what Detent does when a virtual control changes, e.g. press a key.
//!
//! The core only describes actions and expands them into low-level [`InputOp`]s. Executing
//! them (Windows `SendInput`) happens in the app, on its own thread, so the engine never
//! blocks and stays free of OS calls.
//!
//! Keys are stored by name (see [`KEYS`]) and sent as keyboard scan codes, not virtual-key
//! codes: games that read DirectInput or RawInput only see scan codes.

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Hold time of a key tap or mouse click when none is given. Long enough that games which
/// sample input once per frame still see the press.
pub const DEFAULT_HOLD_MS: u32 = 50;
/// Upper bound for one delay or hold, so a typo cannot stall the action queue for hours.
pub const MAX_DELAY_MS: u32 = 60_000;
/// Upper bound on steps per macro.
pub const MAX_STEPS: usize = 200;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "type"
)]
pub enum Action {
    KeyboardMacro { steps: Vec<KeyStep> },
    MouseMacro { steps: Vec<MouseStep> },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "type"
)]
pub enum KeyStep {
    KeyDown {
        key: String,
    },
    KeyUp {
        key: String,
    },
    /// Down, hold, up.
    KeyTap {
        key: String,
        #[serde(default)]
        hold_ms: Option<u32>,
    },
    Delay {
        ms: u32,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MouseButton {
    Left,
    Right,
    Middle,
    X1,
    X2,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "type"
)]
pub enum MouseStep {
    ButtonDown {
        button: MouseButton,
    },
    ButtonUp {
        button: MouseButton,
    },
    /// Down, hold, up.
    Click {
        button: MouseButton,
        #[serde(default)]
        hold_ms: Option<u32>,
    },
    /// Wheel detents; positive scrolls up (away from the user).
    Wheel {
        delta: i32,
    },
    /// Relative move in mickeys; Windows pointer acceleration may scale it.
    MoveRelative {
        dx: i32,
        dy: i32,
    },
    Delay {
        ms: u32,
    },
}

/// One low-level input operation, ready for the platform executor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputOp {
    Key {
        scan: u16,
        extended: bool,
        down: bool,
    },
    MouseButton {
        button: MouseButton,
        down: bool,
    },
    Wheel {
        delta: i32,
    },
    Move {
        dx: i32,
        dy: i32,
    },
    Sleep {
        ms: u32,
    },
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ActionError {
    #[error("unknown key \"{0}\"")]
    UnknownKey(String),
    #[error("delays and hold times must be at most {MAX_DELAY_MS} ms")]
    TooLong,
    #[error("a macro can have at most {MAX_STEPS} steps")]
    TooManySteps,
}

/// A key Detent can send: stable name and its set-1 scan code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyInfo {
    pub name: &'static str,
    pub scan: u16,
    /// Needs the `E0` prefix (`KEYEVENTF_EXTENDEDKEY`).
    pub extended: bool,
}

const fn k(name: &'static str, scan: u16) -> KeyInfo {
    KeyInfo {
        name,
        scan,
        extended: false,
    }
}

const fn e(name: &'static str, scan: u16) -> KeyInfo {
    KeyInfo {
        name,
        scan,
        extended: true,
    }
}

/// All keys that can be used in keyboard macros, in picker order. Names are part of the
/// setup file format: never rename one.
pub const KEYS: &[KeyInfo] = &[
    k("A", 0x1E),
    k("B", 0x30),
    k("C", 0x2E),
    k("D", 0x20),
    k("E", 0x12),
    k("F", 0x21),
    k("G", 0x22),
    k("H", 0x23),
    k("I", 0x17),
    k("J", 0x24),
    k("K", 0x25),
    k("L", 0x26),
    k("M", 0x32),
    k("N", 0x31),
    k("O", 0x18),
    k("P", 0x19),
    k("Q", 0x10),
    k("R", 0x13),
    k("S", 0x1F),
    k("T", 0x14),
    k("U", 0x16),
    k("V", 0x2F),
    k("W", 0x11),
    k("X", 0x2D),
    k("Y", 0x15),
    k("Z", 0x2C),
    k("1", 0x02),
    k("2", 0x03),
    k("3", 0x04),
    k("4", 0x05),
    k("5", 0x06),
    k("6", 0x07),
    k("7", 0x08),
    k("8", 0x09),
    k("9", 0x0A),
    k("0", 0x0B),
    k("F1", 0x3B),
    k("F2", 0x3C),
    k("F3", 0x3D),
    k("F4", 0x3E),
    k("F5", 0x3F),
    k("F6", 0x40),
    k("F7", 0x41),
    k("F8", 0x42),
    k("F9", 0x43),
    k("F10", 0x44),
    k("F11", 0x57),
    k("F12", 0x58),
    k("F13", 0x64),
    k("F14", 0x65),
    k("F15", 0x66),
    k("F16", 0x67),
    k("F17", 0x68),
    k("F18", 0x69),
    k("F19", 0x6A),
    k("F20", 0x6B),
    k("F21", 0x6C),
    k("F22", 0x6D),
    k("F23", 0x6E),
    k("F24", 0x76),
    k("LeftShift", 0x2A),
    k("RightShift", 0x36),
    k("LeftCtrl", 0x1D),
    e("RightCtrl", 0x1D),
    k("LeftAlt", 0x38),
    e("RightAlt", 0x38),
    e("LeftWin", 0x5B),
    e("RightWin", 0x5C),
    e("Menu", 0x5D),
    k("Escape", 0x01),
    k("Tab", 0x0F),
    k("CapsLock", 0x3A),
    k("Space", 0x39),
    k("Enter", 0x1C),
    k("Backspace", 0x0E),
    e("Insert", 0x52),
    e("Delete", 0x53),
    e("Home", 0x47),
    e("End", 0x4F),
    e("PageUp", 0x49),
    e("PageDown", 0x51),
    e("Up", 0x48),
    e("Down", 0x50),
    e("Left", 0x4B),
    e("Right", 0x4D),
    e("PrintScreen", 0x37),
    k("ScrollLock", 0x46),
    e("NumLock", 0x45),
    k("Numpad0", 0x52),
    k("Numpad1", 0x4F),
    k("Numpad2", 0x50),
    k("Numpad3", 0x51),
    k("Numpad4", 0x4B),
    k("Numpad5", 0x4C),
    k("Numpad6", 0x4D),
    k("Numpad7", 0x47),
    k("Numpad8", 0x48),
    k("Numpad9", 0x49),
    k("NumpadAdd", 0x4E),
    k("NumpadSubtract", 0x4A),
    k("NumpadMultiply", 0x37),
    e("NumpadDivide", 0x35),
    k("NumpadDecimal", 0x53),
    e("NumpadEnter", 0x1C),
    k("Minus", 0x0C),
    k("Equals", 0x0D),
    k("LeftBracket", 0x1A),
    k("RightBracket", 0x1B),
    k("Semicolon", 0x27),
    k("Apostrophe", 0x28),
    k("Grave", 0x29),
    k("Backslash", 0x2B),
    k("Comma", 0x33),
    k("Period", 0x34),
    k("Slash", 0x35),
    k("IntlBackslash", 0x56),
];

/// Looks a key up by name, ignoring case.
pub fn key_info(name: &str) -> Option<&'static KeyInfo> {
    KEYS.iter()
        .find(|k| k.name.eq_ignore_ascii_case(name.trim()))
}

fn check_ms(ms: u32) -> Result<(), ActionError> {
    if ms > MAX_DELAY_MS {
        Err(ActionError::TooLong)
    } else {
        Ok(())
    }
}

fn canonical_key(name: &mut String) -> Result<(), ActionError> {
    let info = key_info(name).ok_or_else(|| ActionError::UnknownKey(name.clone()))?;
    *name = info.name.to_owned();
    Ok(())
}

impl Action {
    /// Checks keys and limits, and rewrites key names to their canonical spelling.
    pub fn normalize(&mut self) -> Result<(), ActionError> {
        match self {
            Action::KeyboardMacro { steps } => {
                if steps.len() > MAX_STEPS {
                    return Err(ActionError::TooManySteps);
                }
                for step in steps {
                    match step {
                        KeyStep::KeyDown { key } | KeyStep::KeyUp { key } => canonical_key(key)?,
                        KeyStep::KeyTap { key, hold_ms } => {
                            canonical_key(key)?;
                            check_ms(hold_ms.unwrap_or(0))?;
                        }
                        KeyStep::Delay { ms } => check_ms(*ms)?,
                    }
                }
            }
            Action::MouseMacro { steps } => {
                if steps.len() > MAX_STEPS {
                    return Err(ActionError::TooManySteps);
                }
                for step in steps {
                    match step {
                        MouseStep::Click { hold_ms, .. } => check_ms(hold_ms.unwrap_or(0))?,
                        MouseStep::Delay { ms } => check_ms(*ms)?,
                        _ => {}
                    }
                }
            }
        }
        Ok(())
    }

    /// Expands the action into input operations, in order. Unknown keys (only possible in a
    /// hand-edited setup file) are skipped and reported in the second value.
    pub fn expand(&self) -> (Vec<InputOp>, Vec<String>) {
        let mut ops = Vec::new();
        let mut unknown = Vec::new();
        let mut key = |name: &str, down: bool, ops: &mut Vec<InputOp>| match key_info(name) {
            Some(info) => ops.push(InputOp::Key {
                scan: info.scan,
                extended: info.extended,
                down,
            }),
            None => {
                if !unknown.iter().any(|u| u == name) {
                    unknown.push(name.to_owned());
                }
            }
        };
        match self {
            Action::KeyboardMacro { steps } => {
                for step in steps {
                    match step {
                        KeyStep::KeyDown { key: k } => key(k, true, &mut ops),
                        KeyStep::KeyUp { key: k } => key(k, false, &mut ops),
                        KeyStep::KeyTap { key: k, hold_ms } => {
                            if key_info(k).is_some() {
                                key(k, true, &mut ops);
                                ops.push(InputOp::Sleep {
                                    ms: hold_ms.unwrap_or(DEFAULT_HOLD_MS),
                                });
                            }
                            key(k, false, &mut ops);
                        }
                        KeyStep::Delay { ms } => ops.push(InputOp::Sleep { ms: *ms }),
                    }
                }
            }
            Action::MouseMacro { steps } => {
                for step in steps {
                    match *step {
                        MouseStep::ButtonDown { button } => {
                            ops.push(InputOp::MouseButton { button, down: true });
                        }
                        MouseStep::ButtonUp { button } => {
                            ops.push(InputOp::MouseButton {
                                button,
                                down: false,
                            });
                        }
                        MouseStep::Click { button, hold_ms } => {
                            ops.push(InputOp::MouseButton { button, down: true });
                            ops.push(InputOp::Sleep {
                                ms: hold_ms.unwrap_or(DEFAULT_HOLD_MS),
                            });
                            ops.push(InputOp::MouseButton {
                                button,
                                down: false,
                            });
                        }
                        MouseStep::Wheel { delta } => ops.push(InputOp::Wheel { delta }),
                        MouseStep::MoveRelative { dx, dy } => ops.push(InputOp::Move { dx, dy }),
                        MouseStep::Delay { ms } => ops.push(InputOp::Sleep { ms }),
                    }
                }
            }
        }
        (ops, unknown)
    }

    /// Short description for logs, e.g. "keys: tap LeftCtrl, down F1".
    pub fn describe(&self) -> String {
        let hold = |h: &Option<u32>| h.map(|ms| format!(" {ms}ms")).unwrap_or_default();
        let parts: Vec<String> = match self {
            Action::KeyboardMacro { steps } => steps
                .iter()
                .map(|s| match s {
                    KeyStep::KeyDown { key } => format!("down {key}"),
                    KeyStep::KeyUp { key } => format!("up {key}"),
                    KeyStep::KeyTap { key, hold_ms } => format!("tap {key}{}", hold(hold_ms)),
                    KeyStep::Delay { ms } => format!("wait {ms}ms"),
                })
                .collect(),
            Action::MouseMacro { steps } => steps
                .iter()
                .map(|s| match s {
                    MouseStep::ButtonDown { button } => format!("down {button:?}"),
                    MouseStep::ButtonUp { button } => format!("up {button:?}"),
                    MouseStep::Click { button, hold_ms } => {
                        format!("click {button:?}{}", hold(hold_ms))
                    }
                    MouseStep::Wheel { delta } => format!("wheel {delta}"),
                    MouseStep::MoveRelative { dx, dy } => format!("move {dx},{dy}"),
                    MouseStep::Delay { ms } => format!("wait {ms}ms"),
                })
                .collect(),
        };
        let kind = match self {
            Action::KeyboardMacro { .. } => "keys",
            Action::MouseMacro { .. } => "mouse",
        };
        format!("{kind}: {}", parts.join(", "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn tap(key: &str, hold_ms: Option<u32>) -> KeyStep {
        KeyStep::KeyTap {
            key: key.into(),
            hold_ms,
        }
    }

    #[test]
    fn key_names_are_unique_and_scan_codes_distinct() {
        let names: HashSet<String> = KEYS.iter().map(|k| k.name.to_lowercase()).collect();
        assert_eq!(names.len(), KEYS.len());
        let codes: HashSet<(u16, bool)> = KEYS.iter().map(|k| (k.scan, k.extended)).collect();
        assert_eq!(codes.len(), KEYS.len());
    }

    #[test]
    fn extended_keys_carry_the_flag() {
        assert_eq!(
            key_info("Up").map(|k| (k.scan, k.extended)),
            Some((0x48, true))
        );
        assert_eq!(
            key_info("Numpad8").map(|k| (k.scan, k.extended)),
            Some((0x48, false))
        );
        assert!(key_info("RightCtrl").unwrap().extended);
        assert!(!key_info("LeftCtrl").unwrap().extended);
    }

    #[test]
    fn keyboard_macro_expands_in_order() {
        let a = Action::KeyboardMacro {
            steps: vec![
                KeyStep::KeyDown {
                    key: "LeftCtrl".into(),
                },
                tap("F1", None),
                KeyStep::Delay { ms: 100 },
                KeyStep::KeyUp {
                    key: "LeftCtrl".into(),
                },
                tap("Up", Some(10)),
            ],
        };
        let (ops, unknown) = a.expand();
        assert!(unknown.is_empty());
        let key = |scan, extended, down| InputOp::Key {
            scan,
            extended,
            down,
        };
        assert_eq!(
            ops,
            [
                key(0x1D, false, true),
                key(0x3B, false, true),
                InputOp::Sleep {
                    ms: DEFAULT_HOLD_MS
                },
                key(0x3B, false, false),
                InputOp::Sleep { ms: 100 },
                key(0x1D, false, false),
                key(0x48, true, true),
                InputOp::Sleep { ms: 10 },
                key(0x48, true, false),
            ]
        );
    }

    #[test]
    fn mouse_macro_expands_in_order() {
        let a = Action::MouseMacro {
            steps: vec![
                MouseStep::Click {
                    button: MouseButton::X1,
                    hold_ms: Some(5),
                },
                MouseStep::Wheel { delta: -2 },
                MouseStep::MoveRelative { dx: 10, dy: -3 },
                MouseStep::ButtonDown {
                    button: MouseButton::Right,
                },
                MouseStep::Delay { ms: 7 },
                MouseStep::ButtonUp {
                    button: MouseButton::Right,
                },
            ],
        };
        let (ops, _) = a.expand();
        assert_eq!(
            ops,
            [
                InputOp::MouseButton {
                    button: MouseButton::X1,
                    down: true
                },
                InputOp::Sleep { ms: 5 },
                InputOp::MouseButton {
                    button: MouseButton::X1,
                    down: false
                },
                InputOp::Wheel { delta: -2 },
                InputOp::Move { dx: 10, dy: -3 },
                InputOp::MouseButton {
                    button: MouseButton::Right,
                    down: true
                },
                InputOp::Sleep { ms: 7 },
                InputOp::MouseButton {
                    button: MouseButton::Right,
                    down: false
                },
            ]
        );
    }

    #[test]
    fn unknown_keys_are_skipped_and_reported() {
        let a = Action::KeyboardMacro {
            steps: vec![tap("Hyper", None), tap("A", Some(1))],
        };
        let (ops, unknown) = a.expand();
        assert_eq!(unknown, ["Hyper"]);
        assert_eq!(ops.len(), 3);
    }

    #[test]
    fn normalize_canonicalises_and_validates() {
        let mut a = Action::KeyboardMacro {
            steps: vec![tap(" leftctrl ", None)],
        };
        a.normalize().unwrap();
        assert_eq!(
            a,
            Action::KeyboardMacro {
                steps: vec![tap("LeftCtrl", None)]
            }
        );

        let mut bad = Action::KeyboardMacro {
            steps: vec![tap("Nope", None)],
        };
        assert_eq!(bad.normalize(), Err(ActionError::UnknownKey("Nope".into())));
        let mut long = Action::MouseMacro {
            steps: vec![MouseStep::Delay {
                ms: MAX_DELAY_MS + 1,
            }],
        };
        assert_eq!(long.normalize(), Err(ActionError::TooLong));
    }

    #[test]
    fn json_shape_is_stable() {
        let a = Action::KeyboardMacro {
            steps: vec![tap("F1", Some(20)), KeyStep::Delay { ms: 5 }],
        };
        assert_eq!(
            serde_json::to_string(&a).unwrap(),
            r#"{"type":"keyboardMacro","steps":[{"type":"keyTap","key":"F1","holdMs":20},{"type":"delay","ms":5}]}"#
        );
        let m: Action = serde_json::from_str(
            r#"{"type":"mouseMacro","steps":[{"type":"click","button":"x2"},
                {"type":"moveRelative","dx":1,"dy":2},{"type":"wheel","delta":1}]}"#,
        )
        .unwrap();
        assert_eq!(m.describe(), "mouse: click X2, move 1,2, wheel 1");
    }
}

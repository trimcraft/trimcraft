//! The macro recorder: captures keyboard or mouse input for the macro being edited.
//!
//! A session runs on a thread of its own: a countdown (so the user can switch to the game),
//! then capture until the stop hotkey, the Stop button or Esc (cancel). On Windows capture
//! uses low-level hooks (`WH_KEYBOARD_LL`, `WH_MOUSE_LL`) and, for relative mouse movement,
//! Raw Input. Both observe input globally from Trimcraft's own process; nothing is injected into
//! any other process. Injected input (including Trimcraft's own playback) is ignored.
//!
//! The captured events go to `trimcraft_core::recording`, which turns them into steps; the result
//! waits in the engine until the editor takes it.

use crate::{Shared, overlay};
use std::sync::atomic::{AtomicU8, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::AppHandle;
use trimcraft_core::Movement;
use trimcraft_core::recording::{
    self, COUNTDOWN_MS, Finished, RecordKind, Recorded, RecorderPhase,
};

/// Values of [`Control::request`].
const RUN: u8 = 0;
const STOP: u8 = 1;
const CANCEL: u8 = 2;

/// How a running session is told to end.
struct Control {
    request: AtomicU8,
    /// The capture thread's id once its message loop runs (0 before).
    thread_id: AtomicU32,
}

#[derive(Default)]
pub struct Recorder {
    current: Mutex<Option<Arc<Control>>>,
}

/// How capture ended.
pub enum Ended {
    Stopped(Vec<Recorded>, recording::StopReason),
    Cancelled,
}

impl Recorder {
    /// Starts a session: countdown, then capture.
    pub fn start(
        &self,
        app: &AppHandle,
        shared: &Arc<Shared>,
        kind: RecordKind,
        movement: Movement,
    ) -> Result<(), String> {
        let session = {
            let mut engine = shared.engine();
            let session = engine
                .begin_recording(kind, movement)
                .map_err(|e| e.to_string())?;
            shared.commit(&mut engine);
            session
        };
        let control = Arc::new(Control {
            request: AtomicU8::new(RUN),
            thread_id: AtomicU32::new(0),
        });
        *self.current.lock().unwrap_or_else(|e| e.into_inner()) = Some(control.clone());
        overlay::refresh_visibility(app);
        log::info!("recording {kind:?} macro ({movement:?} movement): countdown");
        let app = app.clone();
        let shared = shared.clone();
        std::thread::Builder::new()
            .name("trimcraft-recorder".into())
            .spawn(move || run(&app, &shared, session, kind, movement, &control))
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Stops the running session and keeps what was recorded (the Stop button).
    pub fn stop(&self) {
        self.request(STOP);
    }

    /// Ends the running session without a result.
    pub fn cancel(&self) {
        self.request(CANCEL);
    }

    fn request(&self, what: u8) {
        if let Some(control) = self
            .current
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
        {
            control.request.store(what, Ordering::SeqCst);
            platform::wake(control.thread_id.load(Ordering::SeqCst));
        }
    }
}

fn run(
    app: &AppHandle,
    shared: &Arc<Shared>,
    session: u64,
    kind: RecordKind,
    movement: Movement,
    control: &Control,
) {
    let result = record(app, shared, session, kind, movement, control);
    // Forget this session's control before the engine allows the next session.
    {
        let mut current = shared
            .recorder
            .current
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if current
            .as_ref()
            .is_some_and(|c| std::ptr::eq(c.as_ref(), control))
        {
            *current = None;
        }
    }
    {
        let mut engine = shared.engine();
        match &result {
            Ok(Some((action, notes))) => {
                log::info!("recorded: {}", action.describe());
                for note in notes {
                    log::info!("recorder: {note}");
                }
            }
            Ok(None) => log::info!("recording cancelled"),
            Err(e) => log::warn!("recording failed: {e}"),
        }
        engine.finish_recording(session, result);
        shared.commit(&mut engine);
    }
    overlay::refresh_visibility(app);
}

type Recording = Option<(trimcraft_core::Action, Vec<String>)>;

fn record(
    app: &AppHandle,
    shared: &Arc<Shared>,
    session: u64,
    kind: RecordKind,
    movement: Movement,
    control: &Control,
) -> Result<Recording, String> {
    let until = Instant::now() + Duration::from_millis(COUNTDOWN_MS);
    while Instant::now() < until {
        if control.request.load(Ordering::SeqCst) != RUN {
            return Ok(None);
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let config = shared.engine().setup().recorder.clone();
    let screens = overlay::screens(app).unwrap_or_default();
    let on_started = || {
        let mut engine = shared.engine();
        engine.set_recording_phase(session, RecorderPhase::Recording);
        shared.commit(&mut engine);
        log::info!("recording started");
    };
    let hotkey = recording::Hotkey::parse(&config.stop_hotkey).ok();
    match platform::capture(kind, movement, hotkey, control, on_started)? {
        Ended::Cancelled => Ok(None),
        Ended::Stopped(events, stop) => Ok(Some(recording::finish(Finished {
            kind,
            movement,
            events,
            stop,
            config: &config,
            screens: &screens,
        }))),
    }
}

mod platform {
    use super::{CANCEL, Control, Ended, RUN};
    use std::sync::Mutex;
    use std::sync::atomic::Ordering;
    use std::time::Instant;
    use trimcraft_core::recording::{Hotkey, RawEvent, RecordKind, Recorded, StopReason};
    use trimcraft_core::{MouseButton, Movement};
    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::System::Threading::GetCurrentThreadId;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        GetAsyncKeyState, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
    };
    use windows_sys::Win32::UI::Input::{
        GetRawInputData, GetRegisteredRawInputDevices, HRAWINPUT, MOUSE_MOVE_ABSOLUTE, RAWINPUT,
        RAWINPUTDEVICE, RAWINPUTHEADER, RID_INPUT, RIDEV_INPUTSINK, RIDEV_REMOVE, RIM_TYPEMOUSE,
        RegisterRawInputDevices,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW,
        GetCursorPos, GetMessageW, HHOOK, HWND_MESSAGE, KBDLLHOOKSTRUCT, LLKHF_EXTENDED,
        LLKHF_INJECTED, LLMHF_INJECTED, MSG, MSLLHOOKSTRUCT, PM_NOREMOVE, PeekMessageW,
        PostQuitMessage, PostThreadMessageW, RegisterClassW, SetWindowsHookExW,
        UnhookWindowsHookEx, WH_KEYBOARD_LL, WH_MOUSE_LL, WM_INPUT, WM_KEYDOWN, WM_LBUTTONDOWN,
        WM_LBUTTONUP, WM_MBUTTONDOWN, WM_MBUTTONUP, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_QUIT,
        WM_RBUTTONDOWN, WM_RBUTTONUP, WM_SYSKEYDOWN, WM_USER, WM_XBUTTONDOWN, WM_XBUTTONUP,
        WNDCLASSW, XBUTTON1,
    };

    /// Scan code of Esc, which cancels.
    const ESC: u16 = 0x01;
    /// Raw Input usage of a mouse (generic desktop page).
    const USAGE_PAGE: u16 = 1;
    const USAGE_MOUSE: u16 = 2;

    enum HookEnd {
        Hotkey,
        Cancel,
    }

    /// What the hook procedures need; they are plain functions, so it is global. Only one
    /// session runs at a time.
    struct Session {
        start: Instant,
        kind: RecordKind,
        movement: Movement,
        hotkey: Option<Hotkey>,
        events: Vec<Recorded>,
        end: Option<HookEnd>,
    }

    static SESSION: Mutex<Option<Session>> = Mutex::new(None);

    fn with_session<T>(f: impl FnOnce(&mut Session) -> T) -> Option<T> {
        SESSION
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_mut()
            .map(f)
    }

    impl Session {
        fn push(&mut self, event: RawEvent) {
            let t_us = self.start.elapsed().as_micros() as u64;
            self.events.push(Recorded { t_us, event });
        }
    }

    fn held(vk: u16) -> bool {
        // SAFETY: no preconditions.
        (unsafe { GetAsyncKeyState(i32::from(vk)) } as u16) & 0x8000 != 0
    }

    fn hotkey_pressed(hk: &Hotkey, scan: u16, extended: bool) -> bool {
        scan == hk.key.scan
            && extended == hk.key.extended
            && (!hk.ctrl || held(VK_CONTROL))
            && (!hk.shift || held(VK_SHIFT))
            && (!hk.alt || held(VK_MENU))
            && (!hk.win || held(VK_LWIN) || held(VK_RWIN))
    }

    unsafe extern "system" fn keyboard_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if code >= 0 {
            // SAFETY: for WH_KEYBOARD_LL with code >= 0, lparam points to a KBDLLHOOKSTRUCT.
            let kb = unsafe { &*(lparam as *const KBDLLHOOKSTRUCT) };
            if kb.flags & LLKHF_INJECTED == 0 {
                let down = matches!(wparam as u32, WM_KEYDOWN | WM_SYSKEYDOWN);
                let scan = kb.scanCode as u16;
                let extended = kb.flags & LLKHF_EXTENDED != 0;
                let swallow = with_session(|s| {
                    if s.end.is_some() {
                        return false;
                    }
                    if scan == ESC && !extended {
                        if down {
                            s.end = Some(HookEnd::Cancel);
                            // SAFETY: posts WM_QUIT to this (the hook's) thread.
                            unsafe { PostQuitMessage(0) };
                        }
                        // Esc never reaches the game while recording.
                        return true;
                    }
                    if down
                        && s.hotkey
                            .is_some_and(|hk| hotkey_pressed(&hk, scan, extended))
                    {
                        s.end = Some(HookEnd::Hotkey);
                        // SAFETY: as above.
                        unsafe { PostQuitMessage(0) };
                        return true;
                    }
                    if s.kind.keys {
                        s.push(RawEvent::Key {
                            scan,
                            extended,
                            down,
                        });
                    }
                    false
                })
                .unwrap_or(false);
                if swallow {
                    return 1;
                }
            }
        }
        // SAFETY: passing the hook arguments on unchanged.
        unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) }
    }

    unsafe extern "system" fn mouse_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if code >= 0 {
            // SAFETY: for WH_MOUSE_LL with code >= 0, lparam points to an MSLLHOOKSTRUCT.
            let m = unsafe { &*(lparam as *const MSLLHOOKSTRUCT) };
            if m.flags & LLMHF_INJECTED == 0 {
                let msg = wparam as u32;
                let high = (m.mouseData >> 16) as u16;
                let button = |button| RawEvent::Button {
                    button,
                    down: matches!(
                        msg,
                        WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN | WM_XBUTTONDOWN
                    ),
                };
                let event = match msg {
                    WM_MOUSEMOVE => Some(RawEvent::Cursor {
                        x: m.pt.x,
                        y: m.pt.y,
                    }),
                    WM_LBUTTONDOWN | WM_LBUTTONUP => Some(button(MouseButton::Left)),
                    WM_RBUTTONDOWN | WM_RBUTTONUP => Some(button(MouseButton::Right)),
                    WM_MBUTTONDOWN | WM_MBUTTONUP => Some(button(MouseButton::Middle)),
                    WM_XBUTTONDOWN | WM_XBUTTONUP if high == XBUTTON1 => {
                        Some(button(MouseButton::X1))
                    }
                    WM_XBUTTONDOWN | WM_XBUTTONUP => Some(button(MouseButton::X2)),
                    // The high word is the signed wheel delta.
                    WM_MOUSEWHEEL => Some(RawEvent::Wheel {
                        delta: i32::from(high as i16),
                    }),
                    _ => None,
                };
                if let Some(event) = event {
                    with_session(|s| {
                        let wanted = match event {
                            RawEvent::Cursor { .. } => s.movement == Movement::Absolute,
                            _ => true,
                        };
                        if s.end.is_none() && wanted {
                            s.push(event);
                        }
                    });
                }
            }
        }
        // SAFETY: passing the hook arguments on unchanged.
        unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) }
    }

    unsafe extern "system" fn window_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        if msg == WM_INPUT {
            let mut raw = RAWINPUT::default();
            let mut size = std::mem::size_of::<RAWINPUT>() as u32;
            // SAFETY: `raw` is a buffer of `size` bytes; a mouse packet fits into RAWINPUT.
            let got = unsafe {
                GetRawInputData(
                    lparam as HRAWINPUT,
                    RID_INPUT,
                    (&mut raw as *mut RAWINPUT).cast(),
                    &mut size,
                    std::mem::size_of::<RAWINPUTHEADER>() as u32,
                )
            };
            // Injected input (SendInput, including Trimcraft's playback) has no device.
            if got != u32::MAX
                && raw.header.dwType == RIM_TYPEMOUSE
                && !raw.header.hDevice.is_null()
            {
                // SAFETY: dwType says the union holds a mouse packet.
                let mouse = unsafe { raw.data.mouse };
                let (dx, dy) = (mouse.lLastX, mouse.lLastY);
                if mouse.usFlags & MOUSE_MOVE_ABSOLUTE == 0 && (dx != 0 || dy != 0) {
                    with_session(|s| {
                        if s.end.is_none() && s.movement == Movement::Relative {
                            s.push(RawEvent::RawMove { dx, dy });
                        }
                    });
                }
            }
        }
        // SAFETY: default handling (WM_INPUT needs it for cleanup).
        unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
    }

    /// Raw Input for the mouse, sent to our window even while a game has focus. Registrations
    /// are per process; any previous one (e.g. the windowing library's) is restored afterwards.
    struct RawMouse {
        previous: Option<RAWINPUTDEVICE>,
    }

    impl RawMouse {
        fn register(hwnd: HWND) -> Result<Self, String> {
            let size = std::mem::size_of::<RAWINPUTDEVICE>() as u32;
            let mut count = 0u32;
            // SAFETY: a null buffer asks for the count only.
            unsafe { GetRegisteredRawInputDevices(std::ptr::null_mut(), &mut count, size) };
            let mut list = vec![RAWINPUTDEVICE::default(); count as usize];
            let mut previous = None;
            if count > 0 {
                // SAFETY: `list` has room for `count` entries.
                let n =
                    unsafe { GetRegisteredRawInputDevices(list.as_mut_ptr(), &mut count, size) };
                if n != u32::MAX {
                    previous = list
                        .iter()
                        .take(n as usize)
                        .find(|d| d.usUsagePage == USAGE_PAGE && d.usUsage == USAGE_MOUSE)
                        .copied();
                }
            }
            let device = RAWINPUTDEVICE {
                usUsagePage: USAGE_PAGE,
                usUsage: USAGE_MOUSE,
                dwFlags: RIDEV_INPUTSINK,
                hwndTarget: hwnd,
            };
            // SAFETY: one valid RAWINPUTDEVICE.
            if unsafe { RegisterRawInputDevices(&device, 1, size) } == 0 {
                return Err(format!(
                    "Raw Input could not be registered: {}",
                    std::io::Error::last_os_error()
                ));
            }
            Ok(Self { previous })
        }
    }

    impl Drop for RawMouse {
        fn drop(&mut self) {
            let device = self.previous.unwrap_or(RAWINPUTDEVICE {
                usUsagePage: USAGE_PAGE,
                usUsage: USAGE_MOUSE,
                dwFlags: RIDEV_REMOVE,
                hwndTarget: std::ptr::null_mut(),
            });
            // SAFETY: one valid RAWINPUTDEVICE.
            unsafe {
                RegisterRawInputDevices(&device, 1, std::mem::size_of::<RAWINPUTDEVICE>() as u32)
            };
        }
    }

    struct Hook(HHOOK);

    impl Drop for Hook {
        fn drop(&mut self) {
            // SAFETY: a hook installed by this thread, removed once.
            unsafe { UnhookWindowsHookEx(self.0) };
        }
    }

    struct Window(HWND);

    impl Drop for Window {
        fn drop(&mut self) {
            // SAFETY: a window created by this thread, destroyed once.
            unsafe { DestroyWindow(self.0) };
        }
    }

    fn hook(
        id: i32,
        proc: unsafe extern "system" fn(i32, WPARAM, LPARAM) -> LRESULT,
    ) -> Result<Hook, String> {
        // SAFETY: a valid hook procedure of this module; thread id 0 = global low-level hook.
        let h = unsafe { SetWindowsHookExW(id, Some(proc), GetModuleHandleW(std::ptr::null()), 0) };
        if h.is_null() {
            Err(format!(
                "input hook could not be installed: {}",
                std::io::Error::last_os_error()
            ))
        } else {
            Ok(Hook(h))
        }
    }

    fn message_window() -> Result<Window, String> {
        let class: Vec<u16> = "TrimcraftRecorder\0".encode_utf16().collect();
        let wc = WNDCLASSW {
            lpfnWndProc: Some(window_proc),
            // SAFETY: no preconditions.
            hInstance: unsafe { GetModuleHandleW(std::ptr::null()) },
            lpszClassName: class.as_ptr(),
            ..Default::default()
        };
        // SAFETY: `wc` is valid; registering twice fails harmlessly (class already exists).
        unsafe { RegisterClassW(&wc) };
        // SAFETY: a message-only window of the class registered above.
        let hwnd = unsafe {
            CreateWindowExW(
                0,
                class.as_ptr(),
                class.as_ptr(),
                0,
                0,
                0,
                0,
                0,
                HWND_MESSAGE,
                std::ptr::null_mut(),
                wc.hInstance,
                std::ptr::null(),
            )
        };
        if hwnd.is_null() {
            Err(format!(
                "recorder window could not be created: {}",
                std::io::Error::last_os_error()
            ))
        } else {
            Ok(Window(hwnd))
        }
    }

    pub fn capture(
        kind: RecordKind,
        movement: Movement,
        hotkey: Option<Hotkey>,
        control: &Control,
        on_started: impl FnOnce(),
    ) -> Result<Ended, String> {
        let mut msg: MSG = unsafe { std::mem::zeroed() };
        // Creates this thread's message queue, so PostThreadMessage reaches it from now on.
        // SAFETY: `msg` is a valid MSG to write into.
        unsafe {
            PeekMessageW(
                &mut msg,
                std::ptr::null_mut(),
                WM_USER,
                WM_USER,
                PM_NOREMOVE,
            )
        };
        // SAFETY: no preconditions.
        control
            .thread_id
            .store(unsafe { GetCurrentThreadId() }, Ordering::SeqCst);

        let mut start_events = Vec::new();
        if kind.mouse && movement == Movement::Absolute {
            let mut p = POINT { x: 0, y: 0 };
            // SAFETY: `p` is a valid POINT to write into.
            if unsafe { GetCursorPos(&mut p) } != 0 {
                start_events.push(Recorded {
                    t_us: 0,
                    event: RawEvent::Cursor { x: p.x, y: p.y },
                });
            }
        }
        *SESSION.lock().unwrap_or_else(|e| e.into_inner()) = Some(Session {
            start: Instant::now(),
            kind,
            movement,
            hotkey,
            events: start_events,
            end: None,
        });

        let result = (|| {
            let _keyboard = hook(WH_KEYBOARD_LL, keyboard_proc)?;
            let _mouse = if kind.mouse {
                Some(hook(WH_MOUSE_LL, mouse_proc)?)
            } else {
                None
            };
            let window = message_window()?;
            let _raw = if kind.mouse && movement == Movement::Relative {
                Some(RawMouse::register(window.0)?)
            } else {
                None
            };
            on_started();
            // A stop requested before the queue existed would have been missed.
            if control.request.load(Ordering::SeqCst) == RUN {
                // SAFETY: `msg` is valid; the loop ends with WM_QUIT (or an error, -1).
                while unsafe { GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) } > 0 {
                    // SAFETY: a message just retrieved by GetMessageW.
                    unsafe { DispatchMessageW(&msg) };
                }
            }
            Ok::<_, String>(())
        })();
        control.thread_id.store(0, Ordering::SeqCst);
        let session = SESSION.lock().unwrap_or_else(|e| e.into_inner()).take();
        result?;
        let session = session.ok_or("recording state lost")?;
        Ok(
            match (session.end, control.request.load(Ordering::SeqCst)) {
                (Some(HookEnd::Cancel), _) | (None, CANCEL) => Ended::Cancelled,
                (Some(HookEnd::Hotkey), _) => Ended::Stopped(session.events, StopReason::Hotkey),
                // The Stop button.
                (None, _) => Ended::Stopped(session.events, StopReason::Ui),
            },
        )
    }

    /// Ends the capture thread's message loop.
    pub fn wake(thread_id: u32) {
        if thread_id != 0 {
            // SAFETY: posting a message to a thread id is safe even if the thread is gone.
            unsafe { PostThreadMessageW(thread_id, WM_QUIT, 0, 0) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Installs the hooks, the message window and Raw Input, then stops at once. Needs an
    /// interactive desktop: `cargo test -p trimcraft -- --ignored recorder`.
    #[test]
    #[ignore = "installs global input hooks"]
    fn capture_sets_up_and_tears_down() {
        for movement in [Movement::Absolute, Movement::Relative] {
            let control = Control {
                request: AtomicU8::new(STOP),
                thread_id: AtomicU32::new(0),
            };
            let mut started = false;
            let ended = platform::capture(RecordKind::MOUSE, movement, None, &control, || {
                started = true
            })
            .unwrap();
            assert!(started);
            assert_eq!(control.thread_id.load(Ordering::SeqCst), 0);
            match ended {
                // Absolute mode starts with the cursor position.
                Ended::Stopped(events, recording::StopReason::Ui) => {
                    assert_eq!(events.len(), usize::from(movement == Movement::Absolute));
                }
                _ => panic!("expected a stop"),
            }
        }
        let control = Control {
            request: AtomicU8::new(CANCEL),
            thread_id: AtomicU32::new(0),
        };
        let ended = platform::capture(
            RecordKind::KEYBOARD,
            Movement::Absolute,
            None,
            &control,
            || {},
        )
        .unwrap();
        assert!(matches!(ended, Ended::Cancelled));
    }
}

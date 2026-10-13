//! The Voicemeeter connector: gain and mute of strips and buses through the VoicemeeterRemote
//! API (see `detent_core::voicemeeter`).
//!
//! The API's DLL (`VoicemeeterRemote64.dll`) is loaded dynamically from the Voicemeeter
//! installation, found through its uninstall entry (or the default folder). Without an
//! installation the connector reports it as missing and its actions are not offered.
//!
//! While enabled, a monitor thread logs in to the API and checks every few seconds that
//! Voicemeeter runs; commands run on a worker thread of their own, so the action queue never
//! waits for Voicemeeter.

use crate::elevation::wide;
use detent_core::connection::{ConnectorState, ConnectorStatus, VoicemeeterSettings};
use detent_core::voicemeeter::{self, VmCommand, VmTarget};
use std::collections::HashMap;
use std::ffi::{CString, c_char};
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};

const POLL: Duration = Duration::from_secs(5);
const DLL: &str = "VoicemeeterRemote64.dll";
const UNINSTALL_KEY: &str = r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\VB:Voicemeeter {17359A74-1236-5467}";
const DEFAULT_DIR: &str = r"C:\Program Files (x86)\VB\Voicemeeter";
/// A value Detent set is used instead of reading it back for this long: Voicemeeter applies
/// parameters asynchronously, so a quick second toggle would read the old value.
const OWN_VALUE_FRESH: Duration = Duration::from_millis(500);

type Reporter = Box<dyn Fn(ConnectorState) + Send + Sync>;

/// What `GetProcAddress` returns.
type Proc = unsafe extern "system" fn() -> isize;
type Call = unsafe extern "system" fn() -> i32;
type GetKind = unsafe extern "system" fn(*mut i32) -> i32;
type GetFloat = unsafe extern "system" fn(*const c_char, *mut f32) -> i32;
type SetFloat = unsafe extern "system" fn(*const c_char, f32) -> i32;

/// The API functions; all return 0 on success.
struct Api {
    login: Call,
    logout: Call,
    kind: GetKind,
    dirty: Call,
    get: GetFloat,
    set: SetFloat,
}

/// Where the DLL is, if Voicemeeter is installed.
fn find_dll() -> Option<std::path::PathBuf> {
    let from_registry = registry_uninstall_string().and_then(|s| voicemeeter::install_dir(&s));
    [from_registry, Some(DEFAULT_DIR.to_owned())]
        .into_iter()
        .flatten()
        .map(|dir| std::path::Path::new(&dir).join(DLL))
        .find(|p| p.is_file())
}

fn registry_uninstall_string() -> Option<String> {
    use windows_sys::Win32::System::Registry::{HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ, RegGetValueW};
    let (key, value) = (wide(UNINSTALL_KEY), wide("UninstallString"));
    let mut buf = [0u16; 1024];
    let mut size = (buf.len() * 2) as u32;
    // SAFETY: NUL-terminated strings; `buf` holds `size` bytes.
    let status = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            key.as_ptr(),
            value.as_ptr(),
            RRF_RT_REG_SZ,
            std::ptr::null_mut(),
            buf.as_mut_ptr().cast(),
            &mut size,
        )
    };
    if status != 0 {
        return None;
    }
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    Some(String::from_utf16_lossy(&buf[..len]))
}

/// Loads the DLL once; it stays loaded for the life of the process.
fn api() -> Result<&'static Api, String> {
    static API: OnceLock<Result<Api, String>> = OnceLock::new();
    API.get_or_init(load).as_ref().map_err(Clone::clone)
}

fn load() -> Result<Api, String> {
    use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
    let path = find_dll().ok_or_else(|| "Voicemeeter is not installed".to_owned())?;
    let name = wide(path.as_os_str());
    // SAFETY: NUL-terminated path of the DLL Voicemeeter installed.
    let module = unsafe { LoadLibraryW(name.as_ptr()) };
    if module.is_null() {
        return Err(format!("cannot load {}", path.display()));
    }
    let symbol = |name: &[u8]| {
        // SAFETY: `module` is loaded; `name` is NUL-terminated.
        unsafe { GetProcAddress(module, name.as_ptr()) }.ok_or_else(|| {
            format!(
                "{DLL} has no {}",
                String::from_utf8_lossy(&name[..name.len() - 1])
            )
        })
    };
    // SAFETY: the signatures are those of VoicemeeterRemote.h (stdcall, which is the system
    // ABI on x64).
    unsafe {
        Ok(Api {
            login: std::mem::transmute::<Proc, Call>(symbol(b"VBVMR_Login\0")?),
            logout: std::mem::transmute::<Proc, Call>(symbol(b"VBVMR_Logout\0")?),
            kind: std::mem::transmute::<Proc, GetKind>(symbol(b"VBVMR_GetVoicemeeterType\0")?),
            dirty: std::mem::transmute::<Proc, Call>(symbol(b"VBVMR_IsParametersDirty\0")?),
            get: std::mem::transmute::<Proc, GetFloat>(symbol(b"VBVMR_GetParameterFloat\0")?),
            set: std::mem::transmute::<Proc, SetFloat>(symbol(b"VBVMR_SetParameterFloat\0")?),
        })
    }
}

#[derive(Default)]
struct State {
    settings: Mutex<VoicemeeterSettings>,
    status: Mutex<ConnectorState>,
    /// Logged in to the API (and the API mutex: one call at a time).
    session: Mutex<bool>,
    /// Values Detent set, with when.
    own: Mutex<HashMap<String, (f32, Instant)>>,
    wake: Mutex<bool>,
    woken: Condvar,
    reporter: OnceLock<Reporter>,
}

pub struct Voicemeeter {
    queue: Sender<(VmTarget, VmCommand)>,
    state: Arc<State>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Voicemeeter {
    pub fn spawn(settings: VoicemeeterSettings) -> Self {
        let (queue, commands) = channel::<(VmTarget, VmCommand)>();
        let state = Arc::new(State::default());
        *lock(&state.settings) = settings;
        let worker = state.clone();
        std::thread::Builder::new()
            .name("detent-voicemeeter".into())
            .spawn(move || {
                for (target, command) in commands {
                    if let Err(e) = worker.run(target, command) {
                        worker.warn(format!("Voicemeeter: {e}"));
                    }
                }
            })
            .expect("spawn voicemeeter thread");
        let monitor = state.clone();
        std::thread::Builder::new()
            .name("detent-voicemeeter-monitor".into())
            .spawn(move || monitor.monitor())
            .expect("spawn voicemeeter monitor");
        Self { queue, state }
    }

    pub fn on_status(&self, report: impl Fn(ConnectorState) + Send + Sync + 'static) {
        let _ = self.state.reporter.set(Box::new(report));
        self.state.report();
    }

    pub fn apply(&self, settings: VoicemeeterSettings) {
        *lock(&self.state.settings) = settings;
        self.state.wake();
    }

    pub fn submit(&self, target: VmTarget, command: VmCommand) {
        if self.queue.send((target, command)).is_err() {
            log::error!("voicemeeter thread is gone; command dropped");
        }
    }

    pub fn test(&self) -> Result<String, String> {
        let result = self.state.probe();
        self.state.wake();
        result
    }
}

impl State {
    fn report(&self) {
        if let Some(report) = self.reporter.get() {
            report(lock(&self.status).clone());
        }
    }

    fn update(&self, change: impl FnOnce(&mut ConnectorState)) {
        {
            let mut s = lock(&self.status);
            let before = s.clone();
            change(&mut s);
            if *s == before {
                return;
            }
            if s.status != before.status {
                log::info!("voicemeeter connector: {:?}", s.status);
            }
        }
        self.report();
    }

    fn wake(&self) {
        *lock(&self.wake) = true;
        self.woken.notify_all();
    }

    fn wait(&self, pause: Option<Duration>) {
        let mut woken = lock(&self.wake);
        let deadline = pause.map(|p| Instant::now() + p);
        while !*woken {
            woken = match deadline {
                None => self.woken.wait(woken).unwrap_or_else(|e| e.into_inner()),
                Some(d) => {
                    let left = d.saturating_duration_since(Instant::now());
                    if left.is_zero() {
                        break;
                    }
                    self.woken
                        .wait_timeout(woken, left)
                        .unwrap_or_else(|e| e.into_inner())
                        .0
                }
            };
        }
        *woken = false;
    }

    /// Logs in if needed and asks which edition runs.
    fn probe(&self) -> Result<String, String> {
        let result = (|| {
            let api = api()?;
            let mut session = lock(&self.session);
            if !*session {
                // SAFETY: API function from the loaded DLL; no arguments.
                let r = unsafe { (api.login)() };
                if r < 0 {
                    return Err(format!("cannot connect to the Voicemeeter API (error {r})"));
                }
                *session = true;
            }
            let mut kind = 0;
            // SAFETY: `kind` outlives the call.
            match unsafe { (api.kind)(&mut kind) } {
                0 => Ok(voicemeeter::edition_name(kind).to_owned()),
                _ => Err("Voicemeeter is not running".to_owned()),
            }
        })();
        match &result {
            Ok(edition) => self.update(|s| {
                s.status = ConnectorStatus::Connected;
                s.detail = Some(edition.clone());
            }),
            Err(message) => self.update(|s| {
                s.status = ConnectorStatus::Error {
                    message: message.clone(),
                };
                s.detail = None;
            }),
        }
        result.map(|edition| format!("Connected to {edition}"))
    }

    fn logout(&self) {
        let mut session = lock(&self.session);
        if *session {
            if let Ok(api) = api() {
                // SAFETY: API function from the loaded DLL; no arguments.
                unsafe { (api.logout)() };
            }
            *session = false;
        }
    }

    fn monitor(&self) {
        // Installed or not is known at once, also while disabled.
        let missing = api().err().filter(|e| e.contains("not installed"));
        if let Some(message) = &missing {
            log::info!("voicemeeter connector: {message}");
        }
        loop {
            if !lock(&self.settings).enabled {
                self.logout();
                {
                    let mut s = lock(&self.status);
                    *s = ConnectorState {
                        missing: missing.clone(),
                        ..ConnectorState::default()
                    };
                }
                self.report();
                self.wait(None);
                continue;
            }
            if lock(&self.status).status == ConnectorStatus::NotConfigured {
                self.update(|s| s.status = ConnectorStatus::Connecting);
            }
            let _ = self.probe();
            self.wait(Some(POLL));
        }
    }

    fn run(&self, target: VmTarget, command: VmCommand) -> Result<(), String> {
        let api = api()?;
        if !lock(&self.settings).enabled {
            return Err("the connection is disabled".into());
        }
        if !*lock(&self.session) {
            self.probe()?;
        }
        let _session = lock(&self.session);
        let name = target.parameter(command.field());
        let c_name = CString::new(name.clone()).map_err(|e| e.to_string())?;
        let current = if command.reads_current() {
            let own = lock(&self.own)
                .get(&name)
                .filter(|(_, at)| at.elapsed() < OWN_VALUE_FRESH)
                .map(|(v, _)| *v);
            match own {
                Some(v) => v,
                None => {
                    let mut value = 0.0f32;
                    // SAFETY: API functions from the loaded DLL; `c_name` is NUL-terminated
                    // and `value` outlives the call. The dirty check refreshes the values.
                    let r = unsafe {
                        (api.dirty)();
                        (api.get)(c_name.as_ptr(), &mut value)
                    };
                    if r != 0 {
                        return Err(format!(
                            "cannot read {name} (error {r}; is Voicemeeter running?)"
                        ));
                    }
                    value
                }
            }
        } else {
            0.0
        };
        let value = command.value(current);
        // SAFETY: as above.
        let r = unsafe { (api.set)(c_name.as_ptr(), value) };
        if r != 0 {
            return Err(format!(
                "cannot set {name} (error {r}; is Voicemeeter running?)"
            ));
        }
        lock(&self.own).insert(name.clone(), (value, Instant::now()));
        log::info!("voicemeeter: {name} = {value}");
        self.update(|s| s.warning = None);
        Ok(())
    }

    fn warn(&self, message: String) {
        log::warn!("{message}");
        self.update(|s| s.warning = Some(message));
        self.wake();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Needs Voicemeeter installed and running: `cargo test -p detent -- --ignored voicemeeter`.
    /// Changes nothing.
    #[test]
    #[ignore = "needs a running Voicemeeter"]
    fn live_voicemeeter_answers() {
        let vm = Voicemeeter::spawn(VoicemeeterSettings { enabled: true });
        println!("{}", vm.test().unwrap());
    }
}

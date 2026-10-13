//! The vJoy connector: writes Trimcraft's vJoy output through the vJoy driver.
//!
//! `vJoyInterface.dll` is loaded at run time from the vJoy installation (found through its
//! uninstall entry, else under Program Files), so Trimcraft starts without vJoy installed. All
//! driver calls go through [`VjoyApi`], which the tests replace with a fake.
//!
//! The engine resolves what it wants on the devices ([`VjoyState`]); this worker thread
//! compares that with what it wrote and sends only the difference. It acquires the devices
//! the active profile uses and relinquishes the others, after releasing their buttons and
//! centring their POVs, so nothing stays pressed: on a profile switch, on disable and on exit.
//! Every few seconds it looks at the devices again (configuration, other feeders).

use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::time::{Duration, Instant};
use trimcraft_core::connection::{ConnectorState, ConnectorStatus};
use trimcraft_core::diagnostics::{AxisThrottle, Category, Diagnostics, Level, Record};
use trimcraft_core::vjoy::{
    self, DeviceOwner, PovDirection, VjoyAxis, VjoyDeviceInfo, VjoyInfo, VjoyState, VjoyWrite,
};

/// How often the device list is read again.
const POLL: Duration = Duration::from_secs(5);

/// A device's status as the driver reports it (`GetVJDStatus`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RawStatus {
    Own,
    Free,
    Busy,
    Missing,
    Unknown,
}

/// The driver calls Trimcraft uses. Devices are 1–16, buttons and POVs 1-based, as vJoy counts.
pub trait VjoyApi: Send {
    /// The driver is installed and enabled.
    fn driver_enabled(&self) -> bool;
    fn version(&self) -> u16;
    fn status(&self, device: u8) -> RawStatus;
    fn buttons(&self, device: u8) -> u8;
    fn discrete_povs(&self, device: u8) -> u8;
    fn continuous_povs(&self, device: u8) -> u8;
    fn has_axis(&self, device: u8, axis: VjoyAxis) -> bool;
    /// Raw range of an axis; `None` if it cannot be read.
    fn axis_range(&self, device: u8, axis: VjoyAxis) -> Option<(i32, i32)>;
    fn acquire(&self, device: u8) -> bool;
    fn relinquish(&self, device: u8);
    fn set_button(&self, device: u8, button: u8, down: bool) -> bool;
    fn set_axis(&self, device: u8, axis: VjoyAxis, raw: i32) -> bool;
    /// `continuous`: the device has continuous POVs (8 directions), else 4-way ones.
    fn set_pov(
        &self,
        device: u8,
        pov: u8,
        direction: Option<PovDirection>,
        continuous: bool,
    ) -> bool;
}

type Loader = fn() -> Result<Box<dyn VjoyApi>, String>;
type Reporter = Box<dyn Fn(ConnectorState) + Send + Sync>;

enum Msg {
    Enabled(bool),
    Desired(VjoyState),
    Test(Sender<Result<String, String>>),
    Reporter(Reporter),
    Shutdown(Sender<()>),
}

pub struct Vjoy {
    queue: Sender<Msg>,
}

impl Vjoy {
    pub fn spawn(enabled: bool, diagnostics: Diagnostics) -> Self {
        Self::spawn_with(enabled, diagnostics, platform::load)
    }

    fn spawn_with(enabled: bool, diagnostics: Diagnostics, loader: Loader) -> Self {
        let (queue, messages) = channel();
        std::thread::Builder::new()
            .name("trimcraft-vjoy".into())
            .spawn(move || Worker::new(enabled, diagnostics, loader).run(messages))
            .expect("spawn vjoy thread");
        Self { queue }
    }

    /// Where status changes go (the engine).
    pub fn on_status(&self, report: impl Fn(ConnectorState) + Send + Sync + 'static) {
        let _ = self.queue.send(Msg::Reporter(Box::new(report)));
    }

    pub fn apply(&self, enabled: bool) {
        let _ = self.queue.send(Msg::Enabled(enabled));
    }

    /// What the engine wants on the devices now.
    pub fn submit(&self, desired: VjoyState) {
        let _ = self.queue.send(Msg::Desired(desired));
    }

    /// "Test connection": reads driver and devices now.
    pub fn test(&self) -> Result<String, String> {
        let (tx, rx) = channel();
        self.queue
            .send(Msg::Test(tx))
            .map_err(|_| "the vJoy thread is gone".to_owned())?;
        rx.recv_timeout(Duration::from_secs(5))
            .map_err(|_| "vJoy did not answer".to_owned())?
    }

    /// Releases everything and gives the devices back; waits a moment for it (on exit).
    pub fn shutdown(&self) {
        let (tx, rx) = channel();
        if self.queue.send(Msg::Shutdown(tx)).is_ok() {
            let _ = rx.recv_timeout(Duration::from_secs(2));
        }
    }
}

struct Worker {
    loader: Loader,
    api: Option<Box<dyn VjoyApi>>,
    load_error: Option<String>,
    enabled: bool,
    desired: VjoyState,
    written: VjoyState,
    info: VjoyInfo,
    /// Devices that could not be acquired, with the reason. Tried again on the next poll,
    /// test or settings change, not on every input event.
    failed: Vec<(u8, String)>,
    report: Option<Reporter>,
    diagnostics: Diagnostics,
    throttle: AxisThrottle,
    started: Instant,
}

impl Worker {
    fn new(enabled: bool, diagnostics: Diagnostics, loader: Loader) -> Self {
        Self {
            loader,
            api: None,
            load_error: None,
            enabled,
            desired: VjoyState::default(),
            written: VjoyState::default(),
            info: VjoyInfo::default(),
            failed: Vec::new(),
            report: None,
            diagnostics,
            throttle: AxisThrottle::default(),
            started: Instant::now(),
        }
    }

    fn run(mut self, messages: Receiver<Msg>) {
        self.refresh();
        loop {
            match messages.recv_timeout(POLL) {
                Ok(Msg::Enabled(enabled)) => {
                    self.enabled = enabled;
                    self.refresh();
                    self.sync(true);
                }
                Ok(Msg::Desired(desired)) => {
                    let retry = desired.devices != self.desired.devices;
                    self.desired = desired;
                    self.sync(retry);
                }
                Ok(Msg::Test(reply)) => {
                    self.load_error = None;
                    self.refresh();
                    self.sync(true);
                    let _ = reply.send(self.test_result());
                }
                Ok(Msg::Reporter(report)) => {
                    self.report = Some(report);
                    self.report();
                }
                Ok(Msg::Shutdown(done)) => {
                    self.enabled = false;
                    self.sync(false);
                    let _ = done.send(());
                    return;
                }
                Err(RecvTimeoutError::Timeout) => {
                    if self.enabled {
                        self.refresh();
                        self.sync(true);
                    }
                }
                Err(RecvTimeoutError::Disconnected) => {
                    self.enabled = false;
                    self.sync(false);
                    return;
                }
            }
        }
    }

    /// Loads the DLL if needed and reads driver version and devices.
    fn refresh(&mut self) {
        if self.api.is_none() && self.load_error.is_none() {
            match (self.loader)() {
                Ok(api) => self.api = Some(api),
                Err(e) => {
                    log::info!("vjoy: {e}");
                    self.load_error = Some(e);
                }
            }
        }
        self.info = match &self.api {
            Some(api) => read_info(api.as_ref()),
            None => VjoyInfo::default(),
        };
        self.report();
    }

    /// Writes the difference between what was written and what is wanted now. `retry`: try
    /// devices again that could not be acquired before.
    fn sync(&mut self, retry: bool) {
        let mut target = if self.enabled && self.info.version.is_some() {
            self.desired.clone()
        } else {
            VjoyState::default()
        };
        let Some(api) = &self.api else {
            self.written = VjoyState::default();
            self.report();
            return;
        };
        let mut failed: Vec<(u8, String)> = Vec::new();
        let wanted: Vec<u8> = target
            .devices
            .difference(&self.written.devices)
            .copied()
            .collect();
        for d in wanted {
            if !retry && let Some(f) = self.failed.iter().find(|(id, _)| *id == d) {
                failed.push(f.clone());
                continue;
            }
            let owner = self
                .info
                .devices
                .iter()
                .find(|i| i.id == d)
                .map(|i| i.owner);
            if api.acquire(d) {
                self.written.devices.insert(d);
                log::info!("vjoy: device {d} acquired");
                self.diagnostics
                    .record(Record::new(Category::Output, format!("vJoy {d} acquired")));
            } else {
                let why = match owner {
                    Some(DeviceOwner::Other) => "it is used by another program",
                    Some(DeviceOwner::Missing) | None => "it is not configured (Configure vJoy)",
                    _ => "the driver refused",
                };
                failed.push((d, format!("vJoy device {d} could not be acquired: {why}")));
            }
        }
        if failed != self.failed {
            for (_, f) in &failed {
                log::warn!("vjoy: {f}");
            }
        }
        target.devices.retain(|d| self.written.devices.contains(d));
        target.restrict_to_devices();
        for write in self.written.diff(&target) {
            self.write(&write);
            self.written.record(&write);
        }
        let changed_devices = self.failed != failed;
        self.failed = failed;
        // Ownership shows in the device list right away.
        if changed_devices || !self.info.devices.is_empty() {
            for dev in &mut self.info.devices {
                if self.written.devices.contains(&dev.id) {
                    dev.owner = DeviceOwner::Trimcraft;
                } else if dev.owner == DeviceOwner::Trimcraft {
                    dev.owner = DeviceOwner::Free;
                }
            }
        }
        self.report();
    }

    fn write(&mut self, write: &VjoyWrite) {
        let Some(api) = &self.api else { return };
        let ok = match *write {
            VjoyWrite::Acquire(_) => true,
            VjoyWrite::Relinquish(d) => {
                api.relinquish(d);
                log::info!("vjoy: device {d} released");
                self.diagnostics
                    .record(Record::new(Category::Output, format!("vJoy {d} released")));
                true
            }
            VjoyWrite::Button(b, down) => {
                self.diagnostics.record(Record::new(
                    Category::Output,
                    format!(
                        "{} {}",
                        vjoy::button_text(b),
                        if down { "pressed" } else { "released" }
                    ),
                ));
                api.set_button(b.device, b.button, down)
            }
            VjoyWrite::Axis(a, value) => {
                let key = format!("vjoy{}", a.device);
                let now = u64::try_from(self.started.elapsed().as_nanos()).unwrap_or(u64::MAX);
                if self
                    .throttle
                    .admit(&key, a.axis as u16, value, now)
                    .is_some()
                {
                    self.diagnostics.record(Record::new(
                        Category::Output,
                        format!("{} {:.0} %", vjoy::axis_text(a), (value + 1.0) * 50.0),
                    ));
                }
                let (min, max) = api.axis_range(a.device, a.axis).unwrap_or((0, 0x8000));
                let raw = min
                    + (((value.clamp(-1.0, 1.0) + 1.0) / 2.0) * (max - min) as f32).round() as i32;
                api.set_axis(a.device, a.axis, raw)
            }
            VjoyWrite::Pov(p, direction) => {
                self.diagnostics.record(Record::new(
                    Category::Output,
                    format!(
                        "{} {}",
                        vjoy::pov_text(p),
                        direction.map_or("centred".to_owned(), |d| format!("{d:?}").to_lowercase())
                    ),
                ));
                let continuous = api.continuous_povs(p.device) > 0;
                api.set_pov(p.device, p.pov, direction, continuous)
            }
        };
        if !ok {
            self.diagnostics.record(
                Record::new(Category::Output, format!("vJoy refused: {write:?}"))
                    .level(Level::Warn),
            );
        }
    }

    fn state(&self) -> ConnectorState {
        let status = if let Some(e) = &self.load_error {
            ConnectorStatus::Error { message: e.clone() }
        } else if self.info.version.is_none() {
            ConnectorStatus::Error {
                message: "the vJoy driver is not enabled (vJoy Configure, Enable vJoy)".into(),
            }
        } else {
            ConnectorStatus::Connected
        };
        let configured = self
            .info
            .devices
            .iter()
            .filter(|d| d.owner != DeviceOwner::Missing)
            .count();
        let detail = self.info.version.as_ref().map(|v| {
            let owned: Vec<String> = self.written.devices.iter().map(u8::to_string).collect();
            format!(
                "vJoy {v}, {configured} device{} configured{}",
                if configured == 1 { "" } else { "s" },
                if owned.is_empty() {
                    String::new()
                } else {
                    format!(", Trimcraft owns {}", owned.join(", "))
                }
            )
        });
        let mut problems: Vec<String> = self.failed.iter().map(|(_, f)| f.clone()).collect();
        if self.enabled {
            problems.extend(vjoy::capability_problems(&self.desired, &self.info.devices));
        }
        ConnectorState {
            status,
            detail,
            warning: (!problems.is_empty()).then(|| problems.join("; ")),
            vjoy: Some(self.info.clone()),
            ..Default::default()
        }
    }

    fn report(&self) {
        if let Some(report) = &self.report {
            report(self.state());
        }
    }

    fn test_result(&self) -> Result<String, String> {
        if let Some(e) = &self.load_error {
            return Err(e.clone());
        }
        let Some(version) = &self.info.version else {
            return Err("the vJoy driver is not enabled: open Configure vJoy and enable it".into());
        };
        let devices: Vec<String> = self
            .info
            .devices
            .iter()
            .filter(|d| d.owner != DeviceOwner::Missing)
            .map(|d| {
                format!(
                    "device {} ({} axes, {} buttons, {} POVs, {})",
                    d.id,
                    d.axes.len(),
                    d.buttons,
                    d.discrete_povs.max(d.continuous_povs),
                    match d.owner {
                        DeviceOwner::Free => "free",
                        DeviceOwner::Trimcraft => "owned by Trimcraft",
                        DeviceOwner::Other => "used by another program",
                        DeviceOwner::Missing => "not configured",
                    }
                )
            })
            .collect();
        if devices.is_empty() {
            return Err(format!(
                "vJoy {version} is installed, but no device is configured: add one in Configure vJoy"
            ));
        }
        Ok(format!("vJoy {version}: {}", devices.join("; ")))
    }
}

/// Driver version and devices 1–16.
fn read_info(api: &dyn VjoyApi) -> VjoyInfo {
    if !api.driver_enabled() {
        return VjoyInfo::default();
    }
    let devices = (1..=vjoy::MAX_DEVICES)
        .map(|id| {
            let owner = match api.status(id) {
                RawStatus::Own => DeviceOwner::Trimcraft,
                RawStatus::Free => DeviceOwner::Free,
                RawStatus::Busy => DeviceOwner::Other,
                RawStatus::Missing | RawStatus::Unknown => DeviceOwner::Missing,
            };
            let present = owner != DeviceOwner::Missing;
            VjoyDeviceInfo {
                id,
                owner,
                axes: if present {
                    VjoyAxis::ALL
                        .into_iter()
                        .filter(|&a| api.has_axis(id, a))
                        .collect()
                } else {
                    Vec::new()
                },
                buttons: if present { api.buttons(id) } else { 0 },
                discrete_povs: if present { api.discrete_povs(id) } else { 0 },
                continuous_povs: if present { api.continuous_povs(id) } else { 0 },
            }
        })
        .collect();
    VjoyInfo {
        version: Some(vjoy::version_text(api.version())),
        devices,
    }
}

mod platform {
    //! `vJoyInterface.dll`, loaded at run time.

    use super::{RawStatus, VjoyApi};
    use std::ffi::c_void;
    use std::path::PathBuf;
    use trimcraft_core::vjoy::{PovDirection, VjoyAxis, pov_discrete, pov_hundredths};
    use windows_sys::Win32::Foundation::{ERROR_SUCCESS, HMODULE};
    use windows_sys::Win32::System::LibraryLoader::{
        GetProcAddress, LOAD_WITH_ALTERED_SEARCH_PATH, LoadLibraryExW,
    };
    use windows_sys::Win32::System::Registry::{
        HKEY, HKEY_LOCAL_MACHINE, KEY_READ, RRF_RT_REG_SZ, RegCloseKey, RegEnumKeyExW,
        RegGetValueW, RegOpenKeyExW,
    };

    type Bool = i32;

    /// The functions Trimcraft calls, as `vJoyInterface.h` declares them.
    struct Fns {
        enabled: unsafe extern "C" fn() -> Bool,
        version: unsafe extern "C" fn() -> i16,
        status: unsafe extern "C" fn(u32) -> i32,
        acquire: unsafe extern "C" fn(u32) -> Bool,
        relinquish: unsafe extern "C" fn(u32),
        buttons: unsafe extern "C" fn(u32) -> i32,
        disc_povs: unsafe extern "C" fn(u32) -> i32,
        cont_povs: unsafe extern "C" fn(u32) -> i32,
        axis_exists: unsafe extern "C" fn(u32, u32) -> Bool,
        axis_min: unsafe extern "C" fn(u32, u32, *mut i32) -> Bool,
        axis_max: unsafe extern "C" fn(u32, u32, *mut i32) -> Bool,
        set_axis: unsafe extern "C" fn(i32, u32, u32) -> Bool,
        set_btn: unsafe extern "C" fn(Bool, u32, u8) -> Bool,
        set_disc_pov: unsafe extern "C" fn(i32, u32, u8) -> Bool,
        set_cont_pov: unsafe extern "C" fn(u32, u32, u8) -> Bool,
    }

    pub struct Dll {
        // The module stays loaded for the life of the process; the function pointers point
        // into it.
        _module: HMODULE,
        fns: Fns,
    }

    // SAFETY: the vJoy interface functions may be called from any thread; this handle is
    // only used from the vJoy worker thread anyway.
    unsafe impl Send for Dll {}

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(Some(0)).collect()
    }

    /// `InstallLocation` of vJoy's uninstall entry.
    fn install_location() -> Option<PathBuf> {
        const UNINSTALL: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall";
        let mut key: HKEY = std::ptr::null_mut();
        // SAFETY: plain registry calls; buffers outlive them; the key is closed below.
        unsafe {
            if RegOpenKeyExW(
                HKEY_LOCAL_MACHINE,
                wide(UNINSTALL).as_ptr(),
                0,
                KEY_READ,
                &mut key,
            ) != ERROR_SUCCESS
            {
                return None;
            }
            let mut found = None;
            for i in 0.. {
                let mut name = [0u16; 256];
                let mut len = name.len() as u32;
                if RegEnumKeyExW(
                    key,
                    i,
                    name.as_mut_ptr(),
                    &mut len,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                ) != ERROR_SUCCESS
                {
                    break;
                }
                let sub = &name[..len as usize];
                let read = |value: &str| -> Option<String> {
                    let mut buf = [0u16; 1024];
                    let mut size = (buf.len() * 2) as u32;
                    let mut path: Vec<u16> = sub.to_vec();
                    path.push(0);
                    let ok = RegGetValueW(
                        key,
                        path.as_ptr(),
                        wide(value).as_ptr(),
                        RRF_RT_REG_SZ,
                        std::ptr::null_mut(),
                        buf.as_mut_ptr() as *mut c_void,
                        &mut size,
                    ) == ERROR_SUCCESS;
                    ok.then(|| {
                        let n = (size as usize / 2).saturating_sub(1);
                        String::from_utf16_lossy(&buf[..n.min(buf.len())])
                    })
                };
                if read("DisplayName").is_some_and(|n| n.starts_with("vJoy"))
                    && let Some(dir) = read("InstallLocation")
                {
                    found = Some(PathBuf::from(dir.trim_end_matches('\0')));
                    break;
                }
            }
            RegCloseKey(key);
            found
        }
    }

    fn candidates() -> Vec<PathBuf> {
        let mut out = Vec::new();
        if let Some(dir) = install_location() {
            out.push(dir.join("x64").join("vJoyInterface.dll"));
        }
        for var in ["ProgramW6432", "ProgramFiles"] {
            if let Ok(dir) = std::env::var(var) {
                out.push(PathBuf::from(dir).join(r"vJoy\x64\vJoyInterface.dll"));
            }
        }
        out
    }

    pub fn load() -> Result<Box<dyn VjoyApi>, String> {
        let path = candidates()
            .into_iter()
            .find(|p| p.is_file())
            .ok_or_else(|| "vJoy is not installed (vJoyInterface.dll not found)".to_owned())?;
        let wide_path: Vec<u16> = path
            .as_os_str()
            .to_string_lossy()
            .encode_utf16()
            .chain(Some(0))
            .collect();
        // SAFETY: loads the vJoy interface DLL by full path; its dependencies come from its
        // own folder (altered search path).
        let module = unsafe {
            LoadLibraryExW(
                wide_path.as_ptr(),
                std::ptr::null_mut(),
                LOAD_WITH_ALTERED_SEARCH_PATH,
            )
        };
        if module.is_null() {
            return Err(format!("cannot load {}", path.display()));
        }
        macro_rules! func {
            ($name:literal, $t:ty) => {{
                // SAFETY: the symbol is looked up in the loaded module; a missing one is an
                // error, not a call.
                let f = unsafe { GetProcAddress(module, concat!($name, "\0").as_ptr()) };
                // SAFETY: the signature matches vJoyInterface.h for this symbol.
                unsafe {
                    std::mem::transmute::<unsafe extern "system" fn() -> isize, $t>(
                        f.ok_or_else(|| format!("{} is missing {}", path.display(), $name))?,
                    )
                }
            }};
        }
        let fns = Fns {
            enabled: func!("vJoyEnabled", unsafe extern "C" fn() -> Bool),
            version: func!("GetvJoyVersion", unsafe extern "C" fn() -> i16),
            status: func!("GetVJDStatus", unsafe extern "C" fn(u32) -> i32),
            acquire: func!("AcquireVJD", unsafe extern "C" fn(u32) -> Bool),
            relinquish: func!("RelinquishVJD", unsafe extern "C" fn(u32)),
            buttons: func!("GetVJDButtonNumber", unsafe extern "C" fn(u32) -> i32),
            disc_povs: func!("GetVJDDiscPovNumber", unsafe extern "C" fn(u32) -> i32),
            cont_povs: func!("GetVJDContPovNumber", unsafe extern "C" fn(u32) -> i32),
            axis_exists: func!("GetVJDAxisExist", unsafe extern "C" fn(u32, u32) -> Bool),
            axis_min: func!(
                "GetVJDAxisMin",
                unsafe extern "C" fn(u32, u32, *mut i32) -> Bool
            ),
            axis_max: func!(
                "GetVJDAxisMax",
                unsafe extern "C" fn(u32, u32, *mut i32) -> Bool
            ),
            set_axis: func!("SetAxis", unsafe extern "C" fn(i32, u32, u32) -> Bool),
            set_btn: func!("SetBtn", unsafe extern "C" fn(Bool, u32, u8) -> Bool),
            set_disc_pov: func!("SetDiscPov", unsafe extern "C" fn(i32, u32, u8) -> Bool),
            set_cont_pov: func!("SetContPov", unsafe extern "C" fn(u32, u32, u8) -> Bool),
        };
        log::info!("vjoy: loaded {}", path.display());
        Ok(Box::new(Dll {
            _module: module,
            fns,
        }))
    }

    fn count(n: i32) -> u8 {
        u8::try_from(n.max(0)).unwrap_or(u8::MAX)
    }

    // SAFETY (all methods): calls into vJoyInterface.dll with plain values; out-pointers point
    // to locals that outlive the call.
    impl VjoyApi for Dll {
        fn driver_enabled(&self) -> bool {
            unsafe { (self.fns.enabled)() != 0 }
        }
        fn version(&self) -> u16 {
            unsafe { (self.fns.version)() as u16 }
        }
        fn status(&self, device: u8) -> RawStatus {
            match unsafe { (self.fns.status)(u32::from(device)) } {
                0 => RawStatus::Own,
                1 => RawStatus::Free,
                2 => RawStatus::Busy,
                3 => RawStatus::Missing,
                _ => RawStatus::Unknown,
            }
        }
        fn buttons(&self, device: u8) -> u8 {
            count(unsafe { (self.fns.buttons)(u32::from(device)) })
        }
        fn discrete_povs(&self, device: u8) -> u8 {
            count(unsafe { (self.fns.disc_povs)(u32::from(device)) })
        }
        fn continuous_povs(&self, device: u8) -> u8 {
            count(unsafe { (self.fns.cont_povs)(u32::from(device)) })
        }
        fn has_axis(&self, device: u8, axis: VjoyAxis) -> bool {
            unsafe { (self.fns.axis_exists)(u32::from(device), axis.usage()) != 0 }
        }
        fn axis_range(&self, device: u8, axis: VjoyAxis) -> Option<(i32, i32)> {
            let (mut min, mut max) = (0i32, 0i32);
            let ok = unsafe {
                (self.fns.axis_min)(u32::from(device), axis.usage(), &mut min) != 0
                    && (self.fns.axis_max)(u32::from(device), axis.usage(), &mut max) != 0
            };
            (ok && max > min).then_some((min, max))
        }
        fn acquire(&self, device: u8) -> bool {
            unsafe { (self.fns.acquire)(u32::from(device)) != 0 }
        }
        fn relinquish(&self, device: u8) {
            unsafe { (self.fns.relinquish)(u32::from(device)) }
        }
        fn set_button(&self, device: u8, button: u8, down: bool) -> bool {
            unsafe { (self.fns.set_btn)(Bool::from(down), u32::from(device), button) != 0 }
        }
        fn set_axis(&self, device: u8, axis: VjoyAxis, raw: i32) -> bool {
            unsafe { (self.fns.set_axis)(raw, u32::from(device), axis.usage()) != 0 }
        }
        fn set_pov(
            &self,
            device: u8,
            pov: u8,
            direction: Option<PovDirection>,
            continuous: bool,
        ) -> bool {
            unsafe {
                if continuous {
                    let value = direction.map_or(u32::MAX, pov_hundredths);
                    (self.fns.set_cont_pov)(value, u32::from(device), pov) != 0
                } else {
                    let value = direction.map_or(-1, pov_discrete);
                    (self.fns.set_disc_pov)(value, u32::from(device), pov) != 0
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use trimcraft_core::trigger::HatDirection;
    use trimcraft_core::vjoy::{VjoyAxisRef, VjoyButton, VjoyPov};

    /// Records every driver call; device 1 is configured with X/Y/Z, 8 buttons, 1 POV;
    /// device 2 belongs to another feeder.
    #[derive(Clone, Default)]
    struct Fake {
        calls: Arc<Mutex<Vec<String>>>,
    }

    static CALLS: Mutex<Option<Arc<Mutex<Vec<String>>>>> = Mutex::new(None);

    impl Fake {
        fn log(&self, s: String) {
            self.calls.lock().unwrap().push(s);
        }
    }

    impl VjoyApi for Fake {
        fn driver_enabled(&self) -> bool {
            true
        }
        fn version(&self) -> u16 {
            0x219
        }
        fn status(&self, device: u8) -> RawStatus {
            match device {
                1 => RawStatus::Free,
                2 => RawStatus::Busy,
                _ => RawStatus::Missing,
            }
        }
        fn buttons(&self, _: u8) -> u8 {
            8
        }
        fn discrete_povs(&self, _: u8) -> u8 {
            0
        }
        fn continuous_povs(&self, _: u8) -> u8 {
            1
        }
        fn has_axis(&self, _: u8, axis: VjoyAxis) -> bool {
            matches!(axis, VjoyAxis::X | VjoyAxis::Y | VjoyAxis::Z)
        }
        fn axis_range(&self, _: u8, _: VjoyAxis) -> Option<(i32, i32)> {
            Some((0, 1000))
        }
        fn acquire(&self, device: u8) -> bool {
            self.log(format!("acquire {device}"));
            device == 1
        }
        fn relinquish(&self, device: u8) {
            self.log(format!("relinquish {device}"));
        }
        fn set_button(&self, device: u8, button: u8, down: bool) -> bool {
            self.log(format!("button {device}/{button} {down}"));
            true
        }
        fn set_axis(&self, device: u8, axis: VjoyAxis, raw: i32) -> bool {
            self.log(format!("axis {device}/{} {raw}", axis.name()));
            true
        }
        fn set_pov(&self, device: u8, pov: u8, d: Option<PovDirection>, c: bool) -> bool {
            self.log(format!("pov {device}/{pov} {d:?} {c}"));
            true
        }
    }

    fn fake_loader() -> Result<Box<dyn VjoyApi>, String> {
        let calls = CALLS.lock().unwrap().clone().unwrap();
        Ok(Box::new(Fake { calls }))
    }

    fn missing_loader() -> Result<Box<dyn VjoyApi>, String> {
        Err("vJoy is not installed (vJoyInterface.dll not found)".into())
    }

    fn worker() -> (Worker, Arc<Mutex<Vec<String>>>) {
        let calls = Arc::new(Mutex::new(Vec::new()));
        *CALLS.lock().unwrap() = Some(calls.clone());
        let mut w = Worker::new(true, Diagnostics::default(), fake_loader);
        w.refresh();
        (w, calls)
    }

    fn take(calls: &Arc<Mutex<Vec<String>>>) -> Vec<String> {
        std::mem::take(&mut *calls.lock().unwrap())
    }

    fn b(device: u8, button: u8) -> VjoyButton {
        VjoyButton { device, button }
    }

    // One test, as the fake shares a global with the loader.
    #[test]
    fn worker_acquires_writes_and_releases_everything() {
        let (mut w, calls) = worker();
        assert_eq!(w.info.version.as_deref(), Some("2.1.9"));
        assert_eq!(w.info.devices[0].axes.len(), 3);
        assert_eq!(w.info.devices[1].owner, DeviceOwner::Other);
        assert!(
            w.test_result()
                .unwrap()
                .contains("device 1 (3 axes, 8 buttons, 1 POVs, free)")
        );

        let mut s = VjoyState::default();
        s.devices.extend([1, 2]);
        s.buttons.extend([b(1, 3), b(2, 1)]);
        s.axes.insert(
            VjoyAxisRef {
                device: 1,
                axis: VjoyAxis::X,
            },
            0.5,
        );
        s.povs
            .insert(VjoyPov { device: 1, pov: 1 }, HatDirection::Right);
        w.desired = s;
        w.sync(true);
        assert_eq!(
            take(&calls),
            [
                "acquire 1",
                "acquire 2",
                "button 1/3 true",
                "axis 1/X 750",
                "pov 1/1 Some(Right) true"
            ]
        );
        // Device 2 belongs to someone else: reported, its outputs skipped.
        let state = w.state();
        assert!(
            state
                .warning
                .unwrap()
                .contains("vJoy device 2 could not be acquired")
        );
        assert_eq!(w.info.devices[0].owner, DeviceOwner::Trimcraft);

        // Another profile: button 3 goes, button 5 comes.
        let mut s = w.desired.clone();
        s.buttons = [b(1, 5)].into();
        s.povs.clear();
        w.desired = s;
        w.sync(false);
        assert_eq!(
            take(&calls),
            ["button 1/3 false", "button 1/5 true", "pov 1/1 None true"]
        );

        // Disabled (or exit): released before the device is given back.
        w.enabled = false;
        w.sync(false);
        assert_eq!(take(&calls), ["button 1/5 false", "relinquish 1"]);
        assert!(w.written == VjoyState::default());
        assert_eq!(w.info.devices[0].owner, DeviceOwner::Free);
    }

    #[test]
    fn missing_vjoy_is_reported() {
        let mut w = Worker::new(true, Diagnostics::default(), missing_loader);
        w.refresh();
        w.sync(true);
        assert!(w.test_result().unwrap_err().contains("not installed"));
        assert!(matches!(w.state().status, ConnectorStatus::Error { .. }));
    }

    #[test]
    #[ignore = "needs vJoy installed; reads the driver only (no device is acquired)"]
    fn reads_the_installed_driver() {
        let api = platform::load().unwrap();
        let info = read_info(api.as_ref());
        println!("{info:#?}");
        assert!(info.version.is_some());
    }
}

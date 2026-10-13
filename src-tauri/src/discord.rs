//! The Discord connector: keeps one connection to the Discord client's local RPC server (see
//! `trimcraft_core::discord` for the protocol and the login flow).
//!
//! One worker thread owns the connection and does everything in order: connecting and
//! reconnecting with a growing pause (backoff), the login, mute/deafen actions, "Test
//! connection", and tracking the live voice state. A reader thread per connection turns the
//! pipe into messages; actions, tests and settings arrive on the same channel, so the worker
//! never polls.
//!
//! Automatic (re)connects only authenticate with stored tokens. The consent popup in Discord
//! is only shown for "Test connection", never on its own.
//!
//! The connection stays open while the connector is enabled: Discord reverts voice settings
//! an app changed when that app disconnects.

use crate::secrets;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::io::{Read, Write};
use std::rc::Rc;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use trimcraft_core::connection::{
    ConnectorState, ConnectorStatus, CredentialState, DiscordSettings,
};
use trimcraft_core::discord::{
    self, Client, Credentials, DiscordCommand, DiscordError, Opcode, Outcome, Secret,
    TokenEndpoint, Tokens, Transport,
};

const BACKOFF_START: Duration = Duration::from_secs(1);
const BACKOFF_MAX: Duration = Duration::from_secs(30);
/// While connected and idle, the worker wakes this often anyway.
const IDLE: Duration = Duration::from_secs(60);

type Reporter = Box<dyn Fn(ConnectorState) + Send + Sync>;
type Reply = Sender<Result<String, String>>;

enum Msg {
    /// A frame from connection `conn`.
    Frame {
        conn: u64,
        op: Opcode,
        payload: String,
    },
    /// Connection `conn` ended.
    Closed {
        conn: u64,
        error: DiscordError,
    },
    Command(DiscordCommand),
    Test(Reply),
    Settings(DiscordSettings),
    /// Tokens were deleted: disconnect and start over.
    Forget,
}

pub struct Discord {
    inbox: Sender<Msg>,
    reporter: Arc<OnceLock<Reporter>>,
    /// The last state, re-sent when the reporter is set.
    last: Arc<Mutex<ConnectorState>>,
}

impl Discord {
    pub fn spawn(settings: DiscordSettings) -> Self {
        let (inbox, rx) = channel();
        let reporter: Arc<OnceLock<Reporter>> = Arc::default();
        let last: Arc<Mutex<ConnectorState>> = Arc::default();
        let (tx, worker_reporter, worker_last) = (inbox.clone(), reporter.clone(), last.clone());
        std::thread::Builder::new()
            .name("trimcraft-discord".into())
            .spawn(move || {
                Worker {
                    inbox: Rc::new(Inbox {
                        rx,
                        tx,
                        stash: RefCell::default(),
                    }),
                    settings,
                    conn: None,
                    next_conn: 0,
                    backoff: BACKOFF_START,
                    retry_at: Some(Instant::now()),
                    state: ConnectorState::default(),
                    reporter: worker_reporter,
                    last: worker_last,
                }
                .run();
            })
            .expect("spawn discord thread");
        Self {
            inbox,
            reporter,
            last,
        }
    }

    /// Where status changes go (the engine). Set once, after the app state exists.
    pub fn on_status(&self, report: impl Fn(ConnectorState) + Send + Sync + 'static) {
        let _ = self.reporter.set(Box::new(report));
        if let Some(r) = self.reporter.get() {
            r(lock(&self.last).clone());
        }
    }

    fn send(&self, msg: Msg) {
        if self.inbox.send(msg).is_err() {
            log::error!("discord thread is gone");
        }
    }

    pub fn apply(&self, settings: DiscordSettings) {
        self.send(Msg::Settings(settings));
    }

    pub fn submit(&self, command: DiscordCommand) {
        self.send(Msg::Command(command));
    }

    /// Tokens were removed from the store.
    pub fn forget(&self) {
        self.send(Msg::Forget);
    }

    /// "Test connection": connects (showing the consent popup if needed) and reads the voice
    /// state. Blocks until done, at most while the popup may be open.
    pub fn test(&self) -> Result<String, String> {
        let (tx, rx) = channel();
        self.send(Msg::Test(tx));
        rx.recv_timeout(discord::APPROVAL_TIMEOUT + Duration::from_secs(30))
            .map_err(|_| "Discord connector did not answer".to_owned())?
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// The worker's channel. Messages that are not frames arriving while the client waits for
/// Discord are kept in `stash` and handled afterwards, in order.
struct Inbox {
    rx: Receiver<Msg>,
    /// For reader threads.
    tx: Sender<Msg>,
    stash: RefCell<VecDeque<Msg>>,
}

impl Inbox {
    /// The next message: stashed ones first.
    fn recv(&self, timeout: Option<Duration>) -> Option<Msg> {
        if let Some(m) = self.stash.borrow_mut().pop_front() {
            return Some(m);
        }
        match timeout {
            None => self.rx.recv().ok(),
            Some(t) => match self.rx.recv_timeout(t) {
                Ok(m) => Some(m),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => {
                    // Cannot happen (the inbox holds a sender); don't spin if it does.
                    std::thread::sleep(t);
                    None
                }
            },
        }
    }
}

/// [`Transport`] over the IPC endpoint: writes go straight out, frames come through the
/// inbox from the reader thread.
struct Pipe {
    id: u64,
    writer: Box<dyn Write + Send>,
    shutdown: Arc<dyn Fn() + Send + Sync>,
    inbox: Rc<Inbox>,
}

impl Transport for Pipe {
    fn send(&mut self, frame: &[u8]) -> Result<(), DiscordError> {
        self.writer
            .write_all(frame)
            .and_then(|()| self.writer.flush())
            .map_err(|e| DiscordError::Disconnected(e.to_string()))
    }

    fn recv(&mut self, timeout: Duration) -> Result<Option<(Opcode, String)>, DiscordError> {
        let deadline = Instant::now() + timeout;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let msg = match self.inbox.rx.recv_timeout(left) {
                Ok(m) => m,
                Err(_) => return Ok(None),
            };
            match msg {
                Msg::Frame { conn, op, payload } if conn == self.id => {
                    return Ok(Some((op, payload)));
                }
                Msg::Closed { conn, error } if conn == self.id => return Err(error),
                // Left over from an earlier connection.
                Msg::Frame { .. } | Msg::Closed { .. } => {}
                other => {
                    self.inbox.stash.borrow_mut().push_back(other);
                    return Ok(None);
                }
            }
        }
    }
}

impl Drop for Pipe {
    fn drop(&mut self) {
        // Ends the reader thread.
        (self.shutdown)();
    }
}

struct Connection {
    client: Client<Pipe>,
    user: String,
}

struct Http;

impl TokenEndpoint for Http {
    fn post(&mut self, form: &[(&'static str, String)]) -> Result<(u16, String), DiscordError> {
        let pairs: Vec<(&str, &str)> = form.iter().map(|(k, v)| (*k, v.as_str())).collect();
        let agent = ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(20))
            .build();
        match agent.post(discord::TOKEN_URL).send_form(&pairs) {
            Ok(r) => {
                let status = r.status();
                let body = r
                    .into_string()
                    .map_err(|e| DiscordError::Http(format!("token request: {e}")))?;
                Ok((status, body))
            }
            Err(ureq::Error::Status(status, r)) => {
                Ok((status, r.into_string().unwrap_or_default()))
            }
            Err(ureq::Error::Transport(t)) => Err(DiscordError::Http(format!(
                "cannot reach discord.com ({})",
                t.kind()
            ))),
        }
    }
}

struct Worker {
    inbox: Rc<Inbox>,
    settings: DiscordSettings,
    conn: Option<Connection>,
    next_conn: u64,
    backoff: Duration,
    /// When to try connecting again; `None`: only when something changes.
    retry_at: Option<Instant>,
    state: ConnectorState,
    reporter: Arc<OnceLock<Reporter>>,
    last: Arc<Mutex<ConnectorState>>,
}

impl Worker {
    fn run(mut self) {
        self.state.credentials = Some(credential_state());
        self.report();
        loop {
            if self.conn.is_some() {
                let result = {
                    let conn = self.conn.as_mut().expect("connected");
                    conn.client.poll(IDLE)
                };
                match result {
                    Ok(()) => self.voice_changed(),
                    Err(e) => self.lost(e),
                }
                loop {
                    let msg = self.inbox.stash.borrow_mut().pop_front();
                    let Some(msg) = msg else { break };
                    self.handle(msg);
                }
                continue;
            }
            if !self.settings.enabled {
                self.set_status(ConnectorStatus::NotConfigured, None);
                if let Some(msg) = self.inbox.recv(None) {
                    self.handle(msg);
                }
                continue;
            }
            match self.retry_at {
                Some(at) if at <= Instant::now() => {
                    if let Err(e) = self.connect(false) {
                        self.failed(e);
                    }
                }
                Some(at) => {
                    let left = at.saturating_duration_since(Instant::now());
                    if let Some(msg) = self.inbox.recv(Some(left)) {
                        self.handle(msg);
                    }
                }
                None => {
                    if let Some(msg) = self.inbox.recv(None) {
                        self.handle(msg);
                    }
                }
            }
        }
    }

    fn handle(&mut self, msg: Msg) {
        match msg {
            // Frames only matter while the client reads them; these are stale.
            Msg::Frame { .. } | Msg::Closed { .. } => {}
            Msg::Command(command) => self.command(command),
            Msg::Test(reply) => {
                let result = self.test().map_err(|e| e.to_string());
                if let Err(e) = &result {
                    log::warn!("discord: test connection failed: {e}");
                }
                let _ = reply.send(result);
            }
            Msg::Settings(settings) => {
                let changed_app = settings.client_id != self.settings.client_id;
                if changed_app || !settings.enabled {
                    self.disconnect();
                }
                self.settings = settings;
                self.state.credentials = Some(credential_state());
                self.state.guide = None;
                self.retry_now();
                if !self.settings.enabled {
                    self.state.warning = None;
                }
                self.report();
            }
            Msg::Forget => {
                self.disconnect();
                self.state.credentials = Some(credential_state());
                self.state.guide = None;
                self.retry_now();
                self.report();
            }
        }
    }

    fn retry_now(&mut self) {
        self.backoff = BACKOFF_START;
        self.retry_at = Some(Instant::now());
    }

    fn credentials(&self) -> Result<Credentials, DiscordError> {
        let client_id = self.settings.client_id.trim().to_owned();
        let secret = match &self.settings.client_secret {
            Some(_) => secrets::get(discord::SECRET_TARGET)
                .map_err(DiscordError::Http)?
                .unwrap_or_default(),
            None => String::new(),
        };
        if client_id.is_empty() || secret.trim().is_empty() {
            return Err(DiscordError::NoCredentials);
        }
        Ok(Credentials {
            client_id,
            client_secret: Secret::new(secret),
        })
    }

    /// Opens the pipe, logs in and subscribes to voice settings. `interactive` allows the
    /// consent popup.
    fn connect(&mut self, interactive: bool) -> Result<(), DiscordError> {
        let creds = self.credentials()?;
        // While retrying, the last error stays visible.
        if interactive || !matches!(self.state.status, ConnectorStatus::Error { .. }) {
            self.set_status(ConnectorStatus::Connecting, None);
        }
        self.next_conn += 1;
        let id = self.next_conn;
        let (reader, writer, shutdown) = ipc::open()?;
        let tx = self.inbox.tx.clone();
        let reader_shutdown = shutdown.clone();
        std::thread::Builder::new()
            .name("trimcraft-discord-reader".into())
            .spawn(move || read_frames(id, reader, &tx, &*reader_shutdown))
            .map_err(|e| DiscordError::Disconnected(e.to_string()))?;
        let pipe = Pipe {
            id,
            writer,
            shutdown,
            inbox: self.inbox.clone(),
        };
        let mut client = Client::connect(pipe, &creds.client_id)?;
        let stored = match secrets::get(discord::TOKENS_TARGET) {
            Ok(Some(json)) => serde_json::from_str::<Tokens>(&json).ok(),
            Ok(None) => None,
            Err(e) => {
                log::warn!("discord: {e}");
                None
            }
        };
        let mut store_failed = None;
        let session = discord::login(
            &mut client,
            &mut Http,
            &creds,
            stored,
            now(),
            interactive,
            &mut |tokens| {
                let json = serde_json::to_string(tokens).expect("tokens serialise");
                if let Err(e) = secrets::set(discord::TOKENS_TARGET, &json) {
                    store_failed = Some(e);
                }
            },
            &mut || {
                log::info!("discord: asking for authorization in the Discord client");
                self.set_status(
                    ConnectorStatus::WaitingForApproval,
                    Some("Approve the request in your Discord client".into()),
                );
            },
        );
        self.state.credentials = Some(credential_state());
        if let Some(e) = store_failed {
            log::warn!("discord: {e}");
        }
        let session = session?;
        let voice = client.start_voice()?;
        client.take_changed();
        log::info!(
            "discord connector: connected as {}, {}",
            session.user,
            voice.describe()
        );
        self.backoff = BACKOFF_START;
        self.conn = Some(Connection {
            client,
            user: session.user,
        });
        self.state.voice = Some(voice);
        self.connected_status();
        Ok(())
    }

    fn connected_status(&mut self) {
        let detail = self
            .conn
            .as_ref()
            .map(|c| format!("Connected as {}", c.user));
        self.state.voice = self.conn.as_ref().and_then(|c| c.client.voice());
        self.state.guide = None;
        self.set_status(ConnectorStatus::Connected, detail);
    }

    fn voice_changed(&mut self) {
        if let Some(conn) = self.conn.as_mut()
            && conn.client.take_changed()
        {
            if let Some(v) = conn.client.voice() {
                log::info!("discord: voice state now {}", v.describe());
            }
            self.connected_status();
        }
    }

    fn disconnect(&mut self) {
        if self.conn.take().is_some() {
            log::info!("discord connector: disconnected");
        }
        self.state.voice = None;
    }

    /// The connection broke.
    fn lost(&mut self, e: DiscordError) {
        self.disconnect();
        self.failed(e);
    }

    /// Connecting failed: show it and plan the next attempt.
    fn failed(&mut self, e: DiscordError) {
        self.state.guide = e.guide_step();
        if e.retry() {
            self.retry_at = Some(Instant::now() + self.backoff);
            self.backoff = (self.backoff * 2).min(BACKOFF_MAX);
        } else {
            // Waits until the user changes something or presses Test connection.
            self.retry_at = None;
        }
        if lock(&self.last).status
            != (ConnectorStatus::Error {
                message: e.to_string(),
            })
        {
            log::warn!("discord connector: {e}");
        }
        self.set_status(
            ConnectorStatus::Error {
                message: e.to_string(),
            },
            None,
        );
    }

    fn test(&mut self) -> Result<String, DiscordError> {
        if self.conn.is_none() {
            let result = self.connect(true);
            if let Err(e) = result {
                if self.settings.enabled {
                    self.failed(e.clone());
                } else {
                    self.state.guide = e.guide_step();
                    self.set_status(ConnectorStatus::NotConfigured, None);
                }
                return Err(e);
            }
        }
        let conn = self.conn.as_mut().expect("connected");
        let user = conn.user.clone();
        let voice = match conn.client.refresh_voice() {
            Ok(v) => v,
            Err(e) => {
                self.lost(e.clone());
                return Err(e);
            }
        };
        let message = format!("Connected as {user}, {}", voice.describe());
        if self.settings.enabled {
            self.voice_changed();
            self.connected_status();
        } else {
            // Disabled: don't hold the voice settings.
            self.disconnect();
            self.set_status(ConnectorStatus::NotConfigured, None);
        }
        Ok(message)
    }

    fn command(&mut self, command: DiscordCommand) {
        if !self.settings.enabled {
            log::warn!(
                "discord: {} skipped, the connection is disabled",
                command.describe()
            );
            return;
        }
        let Some(conn) = self.conn.as_mut() else {
            self.warn(format!(
                "Discord: {} skipped, not connected",
                command.describe()
            ));
            // Perhaps Discord is back: try now.
            if self.retry_at.is_some() {
                self.retry_now();
            }
            return;
        };
        match conn.client.run(command) {
            Ok(Outcome::Skipped(v)) => {
                log::info!(
                    "discord: {} skipped, already so ({})",
                    command.describe(),
                    v.describe()
                );
                self.set_warning(None);
            }
            Ok(Outcome::Changed(v)) => {
                log::info!("discord: {} ({})", command.describe(), v.describe());
                self.set_warning(None);
                self.voice_changed();
            }
            Err(e @ (DiscordError::Disconnected(_) | DiscordError::Timeout(_))) => {
                self.warn(format!("Discord: {} failed: {e}", command.describe()));
                self.lost(e);
            }
            Err(e) => self.warn(format!("Discord: {} failed: {e}", command.describe())),
        }
    }

    fn warn(&mut self, message: String) {
        log::warn!("{message}");
        self.set_warning(Some(message));
    }

    fn set_warning(&mut self, warning: Option<String>) {
        if self.state.warning != warning {
            self.state.warning = warning;
            self.report();
        }
    }

    fn set_status(&mut self, status: ConnectorStatus, detail: Option<String>) {
        if status != ConnectorStatus::Connected {
            self.state.voice = None;
        }
        if self.state.status != status && !matches!(status, ConnectorStatus::Error { .. }) {
            log::info!("discord connector: {status:?}");
        }
        self.state.status = status;
        self.state.detail = detail;
        self.report();
    }

    fn report(&self) {
        {
            let mut last = lock(&self.last);
            if *last == self.state {
                return;
            }
            *last = self.state.clone();
        }
        if let Some(report) = self.reporter.get() {
            report(self.state.clone());
        }
    }
}

fn credential_state() -> CredentialState {
    CredentialState {
        secret_saved: secrets::exists(discord::SECRET_TARGET),
        authorized: secrets::exists(discord::TOKENS_TARGET),
    }
}

/// Reader thread: frames to the inbox until the pipe closes or is shut down.
fn read_frames(conn: u64, mut reader: Box<dyn Read + Send>, tx: &Sender<Msg>, shutdown: &dyn Fn()) {
    loop {
        match discord::read_frame(&mut reader) {
            Ok((op, payload)) => {
                if tx.send(Msg::Frame { conn, op, payload }).is_err() {
                    break;
                }
            }
            Err(error) => {
                let _ = tx.send(Msg::Closed { conn, error });
                break;
            }
        }
    }
    shutdown();
}

/// The IPC endpoint: reader, writer and a function that ends both (unblocking the reader).
type Endpoint = (
    Box<dyn Read + Send>,
    Box<dyn Write + Send>,
    Arc<dyn Fn() + Send + Sync>,
);

mod ipc {
    //! Named pipe opened for overlapped I/O, so the reader thread can wait for data while the
    //! worker writes (on a handle opened for synchronous I/O, Windows serialises the two).
    use super::Endpoint;
    use std::io::{Read, Write};
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::{AsRawHandle, OwnedHandle};
    use std::sync::Arc;
    use trimcraft_core::discord::{DiscordError, PIPE_COUNT, pipe_name};
    use windows_sys::Win32::Foundation::{
        CloseHandle, ERROR_FILE_NOT_FOUND, ERROR_IO_PENDING, ERROR_PIPE_BUSY, GetLastError,
    };
    use windows_sys::Win32::Storage::FileSystem::{FILE_FLAG_OVERLAPPED, ReadFile, WriteFile};
    use windows_sys::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};
    use windows_sys::Win32::System::Threading::CreateEventW;

    struct Handle(OwnedHandle);

    impl Handle {
        /// One overlapped read or write, waited for until it completes.
        fn io(&self, buf: *mut u8, len: usize, write: bool) -> std::io::Result<usize> {
            let h = self.0.as_raw_handle();
            let len = u32::try_from(len).unwrap_or(u32::MAX);
            // SAFETY: plain arguments; a manual-reset event, initially not signalled.
            let event = unsafe { CreateEventW(std::ptr::null(), 1, 0, std::ptr::null()) };
            if event.is_null() {
                return Err(std::io::Error::last_os_error());
            }
            let mut ov = OVERLAPPED {
                hEvent: event,
                ..Default::default()
            };
            // SAFETY: `buf` (len bytes) and `ov` outlive the operation: GetOverlappedResult
            // below waits for it to complete (or be cancelled) before returning.
            let started = unsafe {
                if write {
                    WriteFile(h, buf, len, std::ptr::null_mut(), &mut ov)
                } else {
                    ReadFile(h, buf, len, std::ptr::null_mut(), &mut ov)
                }
            };
            // SAFETY: no preconditions.
            let error = unsafe { GetLastError() };
            let result = if started == 0 && error != ERROR_IO_PENDING {
                Err(std::io::Error::from_raw_os_error(error as i32))
            } else {
                let mut n = 0;
                // SAFETY: `ov` belongs to the operation just started on `h`; waits for it.
                if unsafe { GetOverlappedResult(h, &ov, &mut n, 1) } == 0 {
                    Err(std::io::Error::last_os_error())
                } else {
                    Ok(n as usize)
                }
            };
            // SAFETY: the event was created above and the operation no longer uses it.
            unsafe { CloseHandle(event) };
            result
        }

        fn cancel(&self) {
            // SAFETY: valid handle; a null OVERLAPPED cancels all I/O of this process on it.
            unsafe { CancelIoEx(self.0.as_raw_handle(), std::ptr::null()) };
        }
    }

    struct Half(Arc<Handle>);

    impl Read for Half {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            self.0.io(buf.as_mut_ptr(), buf.len(), false)
        }
    }

    impl Write for Half {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            // WriteFile does not write through the pointer.
            self.0.io(buf.as_ptr().cast_mut(), buf.len(), true)
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    pub fn open() -> Result<Endpoint, DiscordError> {
        for n in 0..PIPE_COUNT {
            let file = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .custom_flags(FILE_FLAG_OVERLAPPED)
                .open(pipe_name(n));
            match file {
                Ok(file) => {
                    let handle = Arc::new(Handle(OwnedHandle::from(file)));
                    let cancel = handle.clone();
                    return Ok((
                        Box::new(Half(handle.clone())),
                        Box::new(Half(handle)),
                        Arc::new(move || cancel.cancel()),
                    ));
                }
                Err(e)
                    if e.raw_os_error() == Some(ERROR_FILE_NOT_FOUND as i32)
                        || e.raw_os_error() == Some(ERROR_PIPE_BUSY as i32) => {}
                Err(e) => {
                    return Err(DiscordError::Disconnected(format!(
                        "cannot open {}: {e}",
                        pipe_name(n)
                    )));
                }
            }
        }
        Err(DiscordError::NotRunning)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Needs a running Discord and saved, authorized credentials:
    /// `cargo test -p trimcraft -- --ignored discord`. Changes nothing.
    #[test]
    #[ignore = "needs a running Discord and an authorized Trimcraft"]
    fn live_discord_answers() {
        let settings = DiscordSettings {
            enabled: true,
            client_id: std::env::var("TRIMCRAFT_DISCORD_CLIENT_ID").unwrap_or_default(),
            client_secret: Some(DiscordSettings::secret_ref()),
        };
        let discord = Discord::spawn(settings);
        let (tx, rx) = channel();
        discord.on_status(move |s| {
            let _ = tx.send(s);
        });
        let connected = rx
            .iter()
            .find(|s| {
                matches!(
                    s.status,
                    ConnectorStatus::Connected | ConnectorStatus::Error { .. }
                )
            })
            .unwrap();
        assert_eq!(
            connected.status,
            ConnectorStatus::Connected,
            "{connected:?}"
        );
        println!("{}", discord.test().unwrap());
    }
}

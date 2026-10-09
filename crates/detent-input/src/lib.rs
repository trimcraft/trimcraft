//! Input backends. A backend runs on its own thread and sends [`BackendEvent`]s over a
//! channel; it owns every OS handle it opens, so nothing platform-specific leaks out.

use detent_core::BackendEvent;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::JoinHandle;

pub mod mock;
#[cfg(feature = "sdl")]
pub mod sdl;

pub trait InputBackend: Send + 'static {
    fn name(&self) -> &'static str;

    /// Runs until `stop` is set. Must send [`BackendEvent::Initial`] once before any other
    /// device event, so identities of devices present at startup resolve together.
    fn run(self: Box<Self>, events: Sender<BackendEvent>, stop: Arc<AtomicBool>);
}

/// A running backend thread. Dropping it stops the thread.
pub struct RunningBackend {
    pub name: &'static str,
    pub events: Receiver<BackendEvent>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Drop for RunningBackend {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

pub fn spawn(backend: Box<dyn InputBackend>) -> RunningBackend {
    let (tx, rx) = mpsc::channel();
    let stop = Arc::new(AtomicBool::new(false));
    let name = backend.name();
    let thread_stop = stop.clone();
    let thread = std::thread::Builder::new()
        .name(format!("detent-input-{name}"))
        .spawn(move || backend.run(tx, thread_stop))
        .expect("spawn input thread");
    RunningBackend {
        name,
        events: rx,
        stop,
        thread: Some(thread),
    }
}

/// The backend selected by the `DETENT_INPUT` environment variable: `mock` for synthetic
/// devices, anything else (or unset) for the real one.
pub fn default_backend() -> Box<dyn InputBackend> {
    match std::env::var("DETENT_INPUT").as_deref() {
        Ok("mock") => Box::new(mock::MockBackend),
        _ => real_backend(),
    }
}

#[cfg(feature = "sdl")]
fn real_backend() -> Box<dyn InputBackend> {
    Box::new(sdl::SdlBackend)
}

#[cfg(not(feature = "sdl"))]
fn real_backend() -> Box<dyn InputBackend> {
    Box::new(mock::MockBackend)
}

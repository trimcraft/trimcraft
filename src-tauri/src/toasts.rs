//! Overlay toasts: keeps the queue of what is on screen, sends it to the overlay window and,
//! when the overlay was hidden with the hotkey, shows the (click-through, non-activating)
//! window only while toasts are up.
//!
//! The engine thread only queues requests and wakes this thread; showing and hiding the window
//! happens here, so nothing waits for the window system while the engine lock is held.

use crate::overlay;
use detent_core::{Toast, ToastQueue, ToastRequest};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};

/// How long the window stays after the last toast ran out, so the fade-out can finish.
const FADE_GRACE: Duration = Duration::from_millis(450);

#[derive(Default)]
struct State {
    queue: ToastQueue,
    /// New toasts arrived: send the list, make sure the window is shown.
    fresh: bool,
    /// Hide the window (unless the overlay is shown) at this time.
    hide_at: Option<Instant>,
}

pub struct Toasts {
    state: Mutex<State>,
    wake: Condvar,
    epoch: Instant,
}

impl Toasts {
    pub fn spawn(app: AppHandle) -> Arc<Self> {
        let toasts = Arc::new(Self {
            state: Mutex::new(State::default()),
            wake: Condvar::new(),
            epoch: Instant::now(),
        });
        let worker = Arc::clone(&toasts);
        let spawned = std::thread::Builder::new()
            .name("detent-toasts".into())
            .spawn(move || worker.run(&app));
        if let Err(e) = spawned {
            log::error!("could not start the toast thread: {e}");
        }
        toasts
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn now_ms(&self) -> u64 {
        u64::try_from(self.epoch.elapsed().as_millis()).unwrap_or(u64::MAX)
    }

    /// Queues toasts, each shown for `duration_ms`. Never blocks.
    pub fn push(&self, requests: Vec<ToastRequest>, duration_ms: u32) {
        if requests.is_empty() {
            return;
        }
        let now = self.now_ms();
        let mut state = self.lock();
        for request in requests {
            state.queue.push(request, now, duration_ms);
        }
        state.fresh = true;
        state.hide_at = None;
        drop(state);
        self.wake.notify_one();
    }

    /// What is on screen now, newest first.
    pub fn list(&self) -> Vec<Toast> {
        self.lock().queue.list()
    }

    fn run(&self, app: &AppHandle) {
        let mut state = self.lock();
        loop {
            let now = self.now_ms();
            let expired = state.queue.expire(now);
            let fresh = std::mem::take(&mut state.fresh);
            if fresh && !state.queue.is_empty() {
                // The window must be up before the first toast is drawn.
                drop(state);
                overlay::toasts_begin(app);
                state = self.lock();
            }
            if fresh || expired {
                let list = state.queue.list();
                if list.is_empty() && state.hide_at.is_none() {
                    state.hide_at = Some(Instant::now() + FADE_GRACE);
                }
                drop(state);
                if let Err(e) = app.emit("detent://toasts", &list) {
                    log::debug!("could not send toasts: {e}");
                }
                state = self.lock();
            }
            if state.hide_at.is_some_and(|t| t <= Instant::now()) {
                state.hide_at = None;
                if state.queue.is_empty() {
                    drop(state);
                    overlay::toasts_end(app);
                    state = self.lock();
                }
            }
            // Sleep until something is due; a push wakes this up earlier.
            let next_toast = state
                .queue
                .next_expiry()
                .map(|at| Duration::from_millis(at.saturating_sub(self.now_ms())));
            let next_hide = state
                .hide_at
                .map(|t| t.saturating_duration_since(Instant::now()));
            let wait = match (next_toast, next_hide) {
                (Some(a), Some(b)) => Some(a.min(b)),
                (a, b) => a.or(b),
            };
            state = match wait {
                Some(wait) => {
                    self.wake
                        .wait_timeout(state, wait.max(Duration::from_millis(1)))
                        .unwrap_or_else(|e| e.into_inner())
                        .0
                }
                None => {
                    if state.fresh {
                        state
                    } else {
                        self.wake.wait(state).unwrap_or_else(|e| e.into_inner())
                    }
                }
            };
        }
    }
}

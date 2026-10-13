//! Overlay toasts: short messages when a virtual control changes ("COM1 · 121.500") or the
//! profile switches.
//!
//! The engine only says what changed ([`ToastRequest`]); the app keeps the [`ToastQueue`] of
//! what is on screen and sends it to the overlay. The queue is plain logic with the clock
//! passed in, so it is unit-tested here.

use serde::Serialize;

/// Toasts on screen at once; a new one pushes the oldest out.
pub const MAX_VISIBLE: usize = 3;
pub const DEFAULT_DURATION_MS: u32 = 2000;
pub const MIN_DURATION_MS: u32 = 500;
pub const MAX_DURATION_MS: u32 = 30_000;

/// What the engine reports: `key` identifies the source (a control id, or `profile`), so a
/// repeated change of the same control updates its toast instead of adding one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToastRequest {
    pub key: String,
    /// E.g. the control's name. A title ending in ":" is followed by a space, any other by " · ".
    pub title: String,
    /// The new state or value.
    pub text: String,
}

/// A toast as the overlay shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Toast {
    /// Stays the same while the toast is updated, so the overlay does not fade it again.
    pub id: u64,
    pub key: String,
    pub title: String,
    pub text: String,
    /// Counts the updates of this toast.
    pub version: u32,
}

#[derive(Debug, Clone)]
struct Entry {
    toast: Toast,
    expires_at_ms: u64,
}

/// The toasts on screen, newest first.
#[derive(Debug, Default)]
pub struct ToastQueue {
    items: Vec<Entry>,
    next_id: u64,
}

impl ToastQueue {
    /// Shows `request` for `duration_ms` from `now_ms`. A toast with the same key is updated
    /// (new text, new deadline) and moves to the top. Returns whether the visible list changed.
    pub fn push(&mut self, request: ToastRequest, now_ms: u64, duration_ms: u32) -> bool {
        let expires_at_ms = now_ms + u64::from(duration_ms.clamp(MIN_DURATION_MS, MAX_DURATION_MS));
        let toast = match self.items.iter().position(|e| e.toast.key == request.key) {
            Some(i) => {
                let mut old = self.items.remove(i).toast;
                old.title = request.title;
                old.text = request.text;
                old.version += 1;
                old
            }
            None => {
                self.next_id += 1;
                Toast {
                    id: self.next_id,
                    key: request.key,
                    title: request.title,
                    text: request.text,
                    version: 0,
                }
            }
        };
        self.items.insert(
            0,
            Entry {
                toast,
                expires_at_ms,
            },
        );
        self.items.truncate(MAX_VISIBLE);
        true
    }

    /// Removes what has run out by `now_ms`. Returns whether anything went.
    pub fn expire(&mut self, now_ms: u64) -> bool {
        let before = self.items.len();
        self.items.retain(|e| e.expires_at_ms > now_ms);
        self.items.len() != before
    }

    /// When the next toast runs out.
    pub fn next_expiry(&self) -> Option<u64> {
        self.items.iter().map(|e| e.expires_at_ms).min()
    }

    pub fn clear(&mut self) -> bool {
        let had = !self.items.is_empty();
        self.items.clear();
        had
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Newest first.
    pub fn list(&self) -> Vec<Toast> {
        self.items.iter().map(|e| e.toast.clone()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(key: &str, text: &str) -> ToastRequest {
        ToastRequest {
            key: key.into(),
            title: key.to_uppercase(),
            text: text.into(),
        }
    }

    fn texts(q: &ToastQueue) -> Vec<String> {
        q.list().into_iter().map(|t| t.text).collect()
    }

    #[test]
    fn newest_is_on_top() {
        let mut q = ToastQueue::default();
        q.push(req("a", "1"), 0, 2000);
        q.push(req("b", "2"), 10, 2000);
        assert_eq!(texts(&q), ["2", "1"]);
    }

    #[test]
    fn at_most_three_the_oldest_is_pushed_out() {
        let mut q = ToastQueue::default();
        for (i, k) in ["a", "b", "c", "d"].iter().enumerate() {
            q.push(req(k, &i.to_string()), i as u64, 2000);
        }
        assert_eq!(texts(&q), ["3", "2", "1"]);
    }

    #[test]
    fn the_same_control_updates_its_toast_in_place() {
        let mut q = ToastQueue::default();
        q.push(req("com1", "121.500"), 0, 2000);
        q.push(req("radio", "Kanal 2"), 100, 2000);
        let id = q.list()[1].id;
        q.push(req("com1", "121.505"), 200, 2000);
        let list = q.list();
        // Still two toasts, the updated one on top, same id, new text, counted update.
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].text, "121.505");
        assert_eq!(list[0].id, id);
        assert_eq!(list[0].version, 1);
        assert_eq!(list[1].text, "Kanal 2");
        assert_eq!(list[1].version, 0);
    }

    #[test]
    fn an_update_restarts_the_clock() {
        let mut q = ToastQueue::default();
        q.push(req("a", "1"), 0, 2000);
        q.push(req("a", "2"), 1500, 2000);
        assert!(!q.expire(2500));
        assert_eq!(q.next_expiry(), Some(3500));
        assert!(q.expire(3500));
        assert!(q.is_empty());
    }

    #[test]
    fn toasts_expire_independently() {
        let mut q = ToastQueue::default();
        q.push(req("a", "1"), 0, 1000);
        q.push(req("b", "2"), 500, 1000);
        assert_eq!(q.next_expiry(), Some(1000));
        assert!(!q.expire(999));
        assert!(q.expire(1000));
        assert_eq!(texts(&q), ["2"]);
        assert!(q.expire(1500));
        assert!(!q.expire(2000));
        assert_eq!(q.next_expiry(), None);
    }

    #[test]
    fn durations_are_kept_within_bounds_and_ids_never_repeat() {
        let mut q = ToastQueue::default();
        q.push(req("a", "1"), 0, 1);
        assert_eq!(q.next_expiry(), Some(u64::from(MIN_DURATION_MS)));
        q.push(req("b", "2"), 0, u32::MAX);
        assert!(q.expire(u64::from(MIN_DURATION_MS)));
        assert_eq!(q.next_expiry(), Some(u64::from(MAX_DURATION_MS)));
        let first = q.list()[0].id;
        q.clear();
        q.push(req("b", "again"), 0, 1000);
        assert!(q.list()[0].id > first);
    }

    #[test]
    fn serialises_for_the_overlay() {
        let mut q = ToastQueue::default();
        q.push(req("a", "x"), 0, 1000);
        let json = serde_json::to_value(q.list()).unwrap();
        assert_eq!(json[0]["key"], "a");
        assert_eq!(json[0]["title"], "A");
        assert_eq!(json[0]["version"], 0);
        assert!(json[0]["id"].is_u64());
    }
}

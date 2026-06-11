//! Transient popup notifications ("toasts").
//!
//! The data model is intentionally free of any rendering or async-timer
//! concerns: every method that depends on the clock takes `now` as a parameter
//! so the timing rules (auto-dismiss, coalescing, redraw scheduling) are pure
//! functions and can be unit-tested deterministically.

use std::borrow::Cow;
use std::time::{Duration, Instant};

use helix_core::diagnostic::Severity;

use crate::graphics::Rect;

#[derive(Debug, Clone)]
pub struct Notification {
    pub id: u32,
    pub text: Cow<'static, str>,
    pub severity: Severity,
    pub created_at: Instant,
    /// When the notification should disappear. `None` means sticky (dismissed
    /// only by the user).
    pub expires_at: Option<Instant>,
    /// How many identical messages have been coalesced into this one.
    pub count: u32,
}

impl Notification {
    fn is_expired(&self, now: Instant) -> bool {
        self.expires_at.is_some_and(|expires_at| now >= expires_at)
    }
}

#[derive(Debug, Default)]
pub struct Notifications {
    /// Oldest first; the newest notification is rendered at the top of the stack.
    items: Vec<Notification>,
    next_id: u32,
    /// Screen rectangles of the toasts as last rendered, used for mouse hit-testing.
    last_rects: Vec<(u32, Rect)>,
}

impl Notifications {
    /// Push a new notification, or coalesce it into the newest one if the text
    /// and severity are identical. `timeout` of `None` makes it sticky.
    pub fn push(
        &mut self,
        text: Cow<'static, str>,
        severity: Severity,
        timeout: Option<Duration>,
        now: Instant,
    ) -> u32 {
        let expires_at = timeout.map(|timeout| now + timeout);

        if let Some(last) = self.items.last_mut() {
            if last.severity == severity && last.text == text {
                last.count += 1;
                last.created_at = now;
                last.expires_at = expires_at;
                return last.id;
            }
        }

        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);
        self.items.push(Notification {
            id,
            text,
            severity,
            created_at: now,
            expires_at,
            count: 1,
        });
        id
    }

    /// Drop notifications whose auto-dismiss deadline has passed.
    pub fn prune(&mut self, now: Instant) {
        self.items.retain(|notification| !notification.is_expired(now));
    }

    /// Dismiss a single notification by id. Returns whether one was removed.
    pub fn dismiss(&mut self, id: u32) -> bool {
        let len = self.items.len();
        self.items.retain(|notification| notification.id != id);
        self.items.len() != len
    }

    /// Dismiss the most recently shown notification.
    pub fn dismiss_top(&mut self) -> bool {
        self.items.pop().is_some()
    }

    /// Dismiss every notification.
    pub fn dismiss_all(&mut self) {
        self.items.clear();
    }

    /// Dismiss the topmost notification whose last-rendered rectangle contains
    /// the given screen cell. Returns whether one was removed.
    pub fn dismiss_at(&mut self, column: u16, row: u16) -> bool {
        let hit = self.last_rects.iter().find_map(|(id, rect)| {
            let within = column >= rect.x
                && column < rect.right()
                && row >= rect.y
                && row < rect.bottom();
            within.then_some(*id)
        });
        match hit {
            Some(id) => self.dismiss(id),
            None => false,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Notifications in stacking order, newest first.
    pub fn iter(&self) -> impl DoubleEndedIterator<Item = &Notification> {
        self.items.iter().rev()
    }

    /// Whether any notification is still counting down toward auto-dismiss. While
    /// true the renderer keeps scheduling frames so toasts disappear on time;
    /// sticky-only stacks need no further redraws.
    pub fn wants_redraw(&self) -> bool {
        self.items
            .iter()
            .any(|notification| notification.expires_at.is_some())
    }

    /// The soonest auto-dismiss deadline across all notifications, if any. The
    /// renderer schedules a single wake-up at this instant so toasts disappear
    /// on time without continuous polling.
    pub fn next_expiry(&self) -> Option<Instant> {
        self.items
            .iter()
            .filter_map(|notification| notification.expires_at)
            .min()
    }

    /// Record the rectangles the toasts were rendered into, for mouse hit-testing.
    pub fn set_last_rects(&mut self, rects: Vec<(u32, Rect)>) {
        self.last_rects = rects;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECOND: Duration = Duration::from_secs(1);

    fn text(s: &'static str) -> Cow<'static, str> {
        Cow::Borrowed(s)
    }

    #[test]
    fn push_sets_expiry_from_timeout() {
        let now = Instant::now();
        let mut n = Notifications::default();
        let id = n.push(text("hi"), Severity::Info, Some(3 * SECOND), now);

        let item = n.iter().next().unwrap();
        assert_eq!(item.id, id);
        assert_eq!(item.expires_at, Some(now + 3 * SECOND));
        assert_eq!(item.count, 1);
    }

    #[test]
    fn none_timeout_is_sticky() {
        let now = Instant::now();
        let mut n = Notifications::default();
        n.push(text("boom"), Severity::Error, None, now);

        assert!(n.iter().next().unwrap().expires_at.is_none());
        // Far in the future, a sticky notification is still present.
        n.prune(now + 3600 * SECOND);
        assert_eq!(n.len(), 1);
        assert!(n.wants_redraw() == false);
    }

    #[test]
    fn identical_messages_coalesce() {
        let now = Instant::now();
        let mut n = Notifications::default();
        let first = n.push(text("saved"), Severity::Info, Some(SECOND), now);
        let later = now + SECOND / 2;
        let second = n.push(text("saved"), Severity::Info, Some(SECOND), later);

        assert_eq!(first, second);
        assert_eq!(n.len(), 1);
        let item = n.iter().next().unwrap();
        assert_eq!(item.count, 2);
        // Timer is reset to the latest push.
        assert_eq!(item.created_at, later);
        assert_eq!(item.expires_at, Some(later + SECOND));
    }

    #[test]
    fn different_severity_does_not_coalesce() {
        let now = Instant::now();
        let mut n = Notifications::default();
        n.push(text("x"), Severity::Info, Some(SECOND), now);
        n.push(text("x"), Severity::Warning, Some(SECOND), now);
        assert_eq!(n.len(), 2);
    }

    #[test]
    fn prune_removes_only_expired() {
        let now = Instant::now();
        let mut n = Notifications::default();
        n.push(text("short"), Severity::Info, Some(SECOND), now);
        n.push(text("long"), Severity::Info, Some(5 * SECOND), now);
        n.push(text("sticky"), Severity::Error, None, now);

        n.prune(now + 2 * SECOND);

        assert_eq!(n.len(), 2);
        let remaining: Vec<_> = n.iter().map(|item| item.text.as_ref()).collect();
        assert_eq!(remaining, vec!["sticky", "long"]);
    }

    #[test]
    fn newest_is_first() {
        let now = Instant::now();
        let mut n = Notifications::default();
        n.push(text("one"), Severity::Info, Some(SECOND), now);
        n.push(text("two"), Severity::Info, Some(SECOND), now);
        let order: Vec<_> = n.iter().map(|item| item.text.as_ref()).collect();
        assert_eq!(order, vec!["two", "one"]);
    }

    #[test]
    fn dismiss_variants() {
        let now = Instant::now();
        let mut n = Notifications::default();
        let a = n.push(text("a"), Severity::Info, Some(SECOND), now);
        n.push(text("b"), Severity::Info, Some(SECOND), now);
        n.push(text("c"), Severity::Info, Some(SECOND), now);

        assert!(n.dismiss(a));
        assert!(!n.dismiss(a));
        assert_eq!(n.len(), 2);

        assert!(n.dismiss_top());
        assert_eq!(n.iter().map(|i| i.text.as_ref()).collect::<Vec<_>>(), vec!["b"]);

        n.dismiss_all();
        assert!(n.is_empty());
    }

    #[test]
    fn dismiss_at_hits_rendered_rect() {
        let now = Instant::now();
        let mut n = Notifications::default();
        let id = n.push(text("click me"), Severity::Info, Some(SECOND), now);
        n.set_last_rects(vec![(id, Rect::new(10, 1, 20, 4))]);

        assert!(!n.dismiss_at(5, 1)); // left of the box
        assert!(!n.dismiss_at(15, 6)); // below the box
        assert!(n.dismiss_at(15, 2)); // inside
        assert!(n.is_empty());
    }
}

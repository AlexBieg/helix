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

/// How long a notification takes to fade out. Dismissing brings a notification's
/// removal deadline forward by this much so it animates out rather than
/// vanishing; auto-dismiss reserves this window at the end of its lifetime.
pub const FADE: Duration = Duration::from_millis(150);

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

    /// Bring the removal deadline forward to `now + fade` (used when dismissing)
    /// so the notification fades out. Never pushes an existing, sooner deadline
    /// later.
    fn begin_fade(&mut self, now: Instant, fade: Duration) {
        let deadline = now + fade;
        self.expires_at = Some(match self.expires_at {
            Some(existing) => existing.min(deadline),
            None => deadline,
        });
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
        self.items
            .retain(|notification| !notification.is_expired(now));
    }

    /// Begin dismissing a single notification by id, fading it out over `fade`
    /// (use `Duration::ZERO` for an immediate removal on the next prune). Returns
    /// whether a matching notification was found.
    pub fn dismiss(&mut self, id: u32, now: Instant, fade: Duration) -> bool {
        match self
            .items
            .iter_mut()
            .find(|notification| notification.id == id)
        {
            Some(notification) => {
                notification.begin_fade(now, fade);
                true
            }
            None => false,
        }
    }

    /// Begin dismissing the most recently shown notification.
    pub fn dismiss_top(&mut self, now: Instant, fade: Duration) -> bool {
        match self.items.last_mut() {
            Some(notification) => {
                notification.begin_fade(now, fade);
                true
            }
            None => false,
        }
    }

    /// Begin dismissing every notification.
    pub fn dismiss_all(&mut self, now: Instant, fade: Duration) {
        for notification in &mut self.items {
            notification.begin_fade(now, fade);
        }
    }

    /// Begin dismissing the topmost notification whose last-rendered rectangle
    /// contains the given screen cell. Returns whether one was hit.
    pub fn dismiss_at(&mut self, column: u16, row: u16, now: Instant, fade: Duration) -> bool {
        let hit = self.last_rects.iter().find_map(|(id, rect)| {
            let within =
                column >= rect.x && column < rect.right() && row >= rect.y && row < rect.bottom();
            within.then_some(*id)
        });
        match hit {
            Some(id) => self.dismiss(id, now, fade),
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
    fn dismiss_variants_remove_after_prune() {
        let now = Instant::now();
        let mut n = Notifications::default();
        let a = n.push(text("a"), Severity::Info, Some(SECOND), now);
        n.push(text("b"), Severity::Info, Some(SECOND), now);
        n.push(text("c"), Severity::Info, Some(SECOND), now);

        // Zero fade dismisses immediately on the next prune.
        assert!(n.dismiss(a, now, Duration::ZERO));
        assert!(!n.dismiss(999, now, Duration::ZERO));
        n.prune(now);
        assert_eq!(n.len(), 2);

        assert!(n.dismiss_top(now, Duration::ZERO));
        n.prune(now);
        assert_eq!(
            n.iter().map(|i| i.text.as_ref()).collect::<Vec<_>>(),
            vec!["b"]
        );

        n.dismiss_all(now, Duration::ZERO);
        n.prune(now);
        assert!(n.is_empty());
    }

    #[test]
    fn dismiss_with_fade_delays_removal() {
        let now = Instant::now();
        let mut n = Notifications::default();
        // Sticky notification gets a removal deadline when dismissed.
        let id = n.push(text("boom"), Severity::Error, None, now);
        assert!(n.dismiss(id, now, FADE));

        assert_eq!(n.iter().next().unwrap().expires_at, Some(now + FADE));
        n.prune(now); // still fading out
        assert_eq!(n.len(), 1);
        n.prune(now + FADE);
        assert!(n.is_empty());
    }

    #[test]
    fn dismiss_never_extends_a_sooner_deadline() {
        let now = Instant::now();
        let mut n = Notifications::default();
        let id = n.push(text("soon"), Severity::Info, Some(SECOND / 2), now);
        // A longer fade must not push the existing, sooner deadline later.
        n.dismiss(id, now, SECOND);
        assert_eq!(n.iter().next().unwrap().expires_at, Some(now + SECOND / 2));
    }

    #[test]
    fn dismiss_at_hits_rendered_rect() {
        let now = Instant::now();
        let mut n = Notifications::default();
        let id = n.push(text("click me"), Severity::Info, Some(SECOND), now);
        n.set_last_rects(vec![(id, Rect::new(10, 1, 20, 4))]);

        assert!(!n.dismiss_at(5, 1, now, Duration::ZERO)); // left of the box
        assert!(!n.dismiss_at(15, 6, now, Duration::ZERO)); // below the box
        assert!(n.dismiss_at(15, 2, now, Duration::ZERO)); // inside
        n.prune(now);
        assert!(n.is_empty());
    }
}

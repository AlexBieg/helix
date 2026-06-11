use std::time::{Duration, Instant};

use helix_core::unicode::width::{UnicodeWidthChar, UnicodeWidthStr};
use helix_view::editor::Severity;
use helix_view::graphics::{Modifier, Rect, Style};
use helix_view::notification::{Notifications, FADE};
use helix_view::Theme;
use tui::buffer::Buffer as Surface;
use tui::widgets::{Block, Widget};

use crate::compositor::Context;

const MIN_BOX_WIDTH: u16 = 30;
const MAX_BOX_WIDTH: u16 = 50;
/// Below this width a box is useless; suppress toasts and rely on the status line.
const ABS_MIN_BOX_WIDTH: u16 = 16;
const MAX_TEXT_LINES: usize = 6;
const RIGHT_MARGIN: u16 = 1;
const TOP_MARGIN: u16 = 1;
/// Vertical gap between stacked toasts.
const GAP: u16 = 1;
const BORDER: u16 = 1;
/// Horizontal padding inside the border.
const PAD: u16 = 1;
/// Rows reserved at the bottom for the statusline (+ optional status message).
const BOTTOM_RESERVED: u16 = 2;

/// How long a toast takes to slide into its resting position.
const SLIDE: Duration = Duration::from_millis(150);
/// How far (in columns) a toast slides horizontally as it enters/leaves.
const SLIDE_DISTANCE: u16 = 6;

struct Toast {
    id: u32,
    rect: Rect,
    severity: Severity,
    title: String,
    lines: Vec<String>,
    created_at: Instant,
    expires_at: Option<Instant>,
}

/// Render the notification stack on top of everything else. Called as a final
/// pass after the compositor has drawn all layers.
pub fn render(viewport: Rect, surface: &mut Surface, cx: &mut Context) {
    cx.editor.notifications.prune(std::time::Instant::now());

    let config = cx.editor.config();
    let enabled = config.notifications.enable;
    let animate = config.notifications.animate;
    let max_visible = config.notifications.max_visible.max(1);
    let use_bufferline = match config.bufferline {
        helix_view::editor::BufferLine::Always => true,
        helix_view::editor::BufferLine::Multiple => cx.editor.documents.len() > 1,
        helix_view::editor::BufferLine::Never => false,
    };
    drop(config);

    if !enabled || cx.editor.notifications.is_empty() {
        cx.editor.notifications.set_last_rects(Vec::new());
        return;
    }

    let now = std::time::Instant::now();

    // Leave a one-row gap below the bufferline (when shown) or the top edge.
    let top_offset = u16::from(use_bufferline) + TOP_MARGIN;
    let (toasts, overflow) = layout(viewport, top_offset, &cx.editor.notifications, max_visible);

    let theme = &cx.editor.theme;
    let base_background = theme
        .try_get_exact("ui.notification")
        .unwrap_or_else(|| theme.get("ui.popup"));
    let base_text = theme
        .try_get_exact("ui.notification.text")
        .unwrap_or_else(|| theme.get("ui.text"));

    let mut drawn_rects = Vec::with_capacity(toasts.len());
    let mut any_animating = false;
    let mut next_y = viewport.y;
    for toast in &toasts {
        // Slide horizontally as the toast enters and leaves; dim while leaving.
        let (shift, dim) = if animate {
            any_animating |= is_animating(toast.created_at, toast.expires_at, now);
            (
                slide_shift(toast.created_at, toast.expires_at, now, SLIDE_DISTANCE)
                    .min(toast.rect.x.saturating_sub(viewport.x)),
                fade_progress(toast.expires_at, now) > 0.0,
            )
        } else {
            (0, false)
        };

        let area = Rect::new(toast.rect.x - shift, toast.rect.y, toast.rect.width, toast.rect.height);
        drawn_rects.push((toast.id, area));

        let background = dimmed(base_background, dim);
        let text_style = dimmed(base_text, dim);
        let accent = dimmed(accent_style(theme, toast.severity), dim);

        surface.clear_with(area, background);
        let block = Block::bordered().title(toast.title.as_str()).border_style(accent);
        let inner = block.inner(area);
        block.render(area, surface);

        let text_x = inner.x + PAD;
        let text_width = inner.width.saturating_sub(2 * PAD) as usize;
        for (i, line) in toast.lines.iter().enumerate() {
            surface.set_stringn(text_x, inner.y + i as u16, line, text_width, text_style);
        }
        next_y = area.bottom() + GAP;
    }

    if overflow > 0 && next_y < viewport.bottom().saturating_sub(BOTTOM_RESERVED) {
        let label = format!("+{overflow} more");
        let width = label.width() as u16;
        let x = viewport.right().saturating_sub(width + RIGHT_MARGIN);
        let style = base_text.add_modifier(Modifier::DIM);
        surface.set_string(x, next_y, &label, style);
    }

    cx.editor.notifications.set_last_rects(drawn_rects);

    if animate && any_animating {
        // While a toast is sliding/fading, keep frames coming (~30 FPS).
        helix_event::request_redraw();
    } else if let Some(deadline) = next_redraw(&cx.editor.notifications, animate) {
        // Otherwise wake once at the next visual change (fade start or removal).
        cx.editor.request_redraw_at(deadline);
    }
}

/// The next instant at which a toast's appearance changes — the start of its
/// fade-out when animating, or its removal otherwise. Drives a single scheduled
/// wake-up so static stacks don't busy-poll.
fn next_redraw(notifications: &Notifications, animate: bool) -> Option<Instant> {
    notifications
        .iter()
        .filter_map(|n| {
            n.expires_at.map(|expires_at| {
                if animate {
                    expires_at.checked_sub(FADE).unwrap_or(expires_at)
                } else {
                    expires_at
                }
            })
        })
        .min()
}

fn clamp01(value: f32) -> f32 {
    value.clamp(0.0, 1.0)
}

fn ease_out_cubic(t: f32) -> f32 {
    1.0 - (1.0 - t).powi(3)
}

fn ease_in_cubic(t: f32) -> f32 {
    t * t * t
}

/// Slide-in completion in `[0, 1]`.
fn slide_in_progress(created_at: Instant, now: Instant) -> f32 {
    clamp01(now.saturating_duration_since(created_at).as_secs_f32() / SLIDE.as_secs_f32())
}

/// Fade-out progress in `[0, 1]`: `0` until the fade window starts, `1` at removal.
fn fade_progress(expires_at: Option<Instant>, now: Instant) -> f32 {
    match expires_at {
        Some(expires_at) => {
            let start = expires_at.checked_sub(FADE).unwrap_or(expires_at);
            if now <= start {
                0.0
            } else {
                clamp01(now.saturating_duration_since(start).as_secs_f32() / FADE.as_secs_f32())
            }
        }
        None => 0.0,
    }
}

/// Whether a toast is mid-animation (sliding in or fading out) at `now`.
fn is_animating(created_at: Instant, expires_at: Option<Instant>, now: Instant) -> bool {
    slide_in_progress(created_at, now) < 1.0 || fade_progress(expires_at, now) > 0.0
}

/// Columns to shift a toast left of its resting position for the slide effect.
fn slide_shift(created_at: Instant, expires_at: Option<Instant>, now: Instant, distance: u16) -> u16 {
    let distance = distance as f32;
    let entering = (1.0 - ease_out_cubic(slide_in_progress(created_at, now))) * distance;
    let leaving = ease_in_cubic(fade_progress(expires_at, now)) * distance;
    entering.max(leaving).round() as u16
}

fn dimmed(style: Style, dim: bool) -> Style {
    if dim {
        style.add_modifier(Modifier::DIM)
    } else {
        style
    }
}

fn layout(
    viewport: Rect,
    top_offset: u16,
    notifications: &Notifications,
    max_visible: usize,
) -> (Vec<Toast>, usize) {
    let available = viewport.width.saturating_sub(RIGHT_MARGIN);

    // Not enough horizontal room for a usable box: fall back to the status-line
    // mirror entirely.
    if available < ABS_MIN_BOX_WIDTH {
        return (Vec::new(), notifications.len());
    }

    // Keep toasts from covering more than ~2/3 of the screen on wide terminals.
    let max_box = MAX_BOX_WIDTH
        .min(available)
        .min((viewport.width.saturating_mul(2) / 3).max(ABS_MIN_BOX_WIDTH));
    let min_box = MIN_BOX_WIDTH.min(max_box);

    // A single uniform width keeps the stack tidy: the widest visible line,
    // clamped to the available range.
    let widest = notifications
        .iter()
        .take(max_visible)
        .flat_map(|n| {
            n.text
                .split('\n')
                .map(UnicodeWidthStr::width)
                .chain(std::iter::once(title(n).width()))
        })
        .max()
        .unwrap_or(0) as u16;
    let box_width = (widest + 2 * BORDER + 2 * PAD).clamp(min_box, max_box);
    let inner_width = box_width.saturating_sub(2 * BORDER + 2 * PAD) as usize;
    let x = viewport.right().saturating_sub(box_width + RIGHT_MARGIN);
    let bottom_limit = viewport.bottom().saturating_sub(BOTTOM_RESERVED);

    let mut toasts = Vec::new();
    let mut y = viewport.y + top_offset;
    for notification in notifications.iter() {
        if toasts.len() >= max_visible {
            break;
        }

        let mut lines = wrap(&notification.text, inner_width);
        if lines.len() > MAX_TEXT_LINES {
            lines.truncate(MAX_TEXT_LINES);
            if let Some(last) = lines.last_mut() {
                truncate_with_ellipsis(last, inner_width);
            }
        }
        let box_height = lines.len() as u16 + 2 * BORDER;
        if y + box_height > bottom_limit {
            break;
        }

        toasts.push(Toast {
            id: notification.id,
            rect: Rect::new(x, y, box_width, box_height),
            severity: notification.severity,
            title: title(notification),
            lines,
            created_at: notification.created_at,
            expires_at: notification.expires_at,
        });
        y += box_height + GAP;
    }

    let overflow = notifications.len() - toasts.len();
    (toasts, overflow)
}

fn title(notification: &helix_view::notification::Notification) -> String {
    let label = match notification.severity {
        Severity::Error => "Error",
        Severity::Warning => "Warning",
        Severity::Info => "Info",
        Severity::Hint => "Hint",
    };
    if notification.count > 1 {
        format!(" {label} (×{}) ", notification.count)
    } else {
        format!(" {label} ")
    }
}

fn accent_style(theme: &Theme, severity: Severity) -> Style {
    let (scope, fallback) = match severity {
        Severity::Error => ("ui.notification.error", "error"),
        Severity::Warning => ("ui.notification.warning", "warning"),
        Severity::Info => ("ui.notification.info", "info"),
        Severity::Hint => ("ui.notification.hint", "hint"),
    };
    theme
        .try_get_exact(scope)
        .unwrap_or_else(|| theme.get(fallback))
}

/// Word-aware wrap to `width` display columns, breaking words longer than the
/// line. Each `\n` in the input starts a new logical line.
fn wrap(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut lines = Vec::new();

    for logical in text.split('\n') {
        let mut line = String::new();
        let mut line_width = 0;

        let mut push_break = |line: &mut String, line_width: &mut usize| {
            lines.push(std::mem::take(line));
            *line_width = 0;
        };

        for word in logical.split(' ') {
            let word_width = word.width();
            let sep = usize::from(!line.is_empty());

            if !line.is_empty() && line_width + sep + word_width > width {
                push_break(&mut line, &mut line_width);
            }

            if word_width <= width {
                if !line.is_empty() {
                    line.push(' ');
                    line_width += 1;
                }
                line.push_str(word);
                line_width += word_width;
            } else {
                // Word is wider than the line: break it across characters.
                for ch in word.chars() {
                    let ch_width = ch.width().unwrap_or(0);
                    if line_width + ch_width > width && !line.is_empty() {
                        push_break(&mut line, &mut line_width);
                    }
                    line.push(ch);
                    line_width += ch_width;
                }
            }
        }

        lines.push(line);
    }

    lines
}

fn truncate_with_ellipsis(line: &mut String, width: usize) {
    let target = width.saturating_sub(1);
    let mut used = 0;
    let mut cut = line.len();
    for (i, ch) in line.char_indices() {
        let ch_width = ch.width().unwrap_or(0);
        if used + ch_width > target {
            cut = i;
            break;
        }
        used += ch_width;
    }
    line.truncate(cut);
    line.push('…');
}

#[cfg(test)]
mod tests {
    use super::*;
    use helix_view::notification::Notifications;
    use std::borrow::Cow;
    use std::time::{Duration, Instant};

    fn notifications(messages: &[(&'static str, Severity)]) -> Notifications {
        let now = Instant::now();
        let mut n = Notifications::default();
        for (text, severity) in messages {
            n.push(Cow::Borrowed(*text), *severity, Some(Duration::from_secs(1)), now);
        }
        n
    }

    #[test]
    fn wrap_breaks_on_words() {
        assert_eq!(wrap("the quick brown fox", 9), vec!["the quick", "brown fox"]);
    }

    #[test]
    fn wrap_breaks_long_words() {
        assert_eq!(wrap("supercalifragilistic", 5), vec!["super", "calif", "ragil", "istic"]);
    }

    #[test]
    fn wrap_preserves_explicit_newlines() {
        assert_eq!(wrap("a\nb", 10), vec!["a", "b"]);
    }

    #[test]
    fn truncate_adds_ellipsis() {
        let mut line = String::from("hello world");
        truncate_with_ellipsis(&mut line, 6);
        assert_eq!(line, "hello…");
    }

    #[test]
    fn layout_stacks_right_aligned() {
        let viewport = Rect::new(0, 0, 100, 40);
        let n = notifications(&[("first", Severity::Info), ("second", Severity::Warning)]);
        let (toasts, overflow) = layout(viewport, TOP_MARGIN, &n, 5);

        assert_eq!(overflow, 0);
        assert_eq!(toasts.len(), 2);
        // Newest first, top of the stack.
        assert_eq!(toasts[0].severity, Severity::Warning);
        // Right-aligned with the margin.
        assert_eq!(toasts[0].rect.right(), viewport.right() - RIGHT_MARGIN);
        // Second box sits below the first with a gap.
        assert!(toasts[1].rect.y >= toasts[0].rect.bottom() + GAP);
    }

    #[test]
    fn layout_caps_visible_and_reports_overflow() {
        let viewport = Rect::new(0, 0, 100, 40);
        let msgs: Vec<_> = (0..8)
            .map(|i| (["a", "b", "c", "d", "e", "f", "g", "h"][i], Severity::Info))
            .collect();
        let n = notifications(&msgs);
        let (toasts, overflow) = layout(viewport, TOP_MARGIN, &n, 5);
        assert_eq!(toasts.len(), 5);
        assert_eq!(overflow, 3);
    }

    #[test]
    fn layout_suppressed_when_too_narrow() {
        let viewport = Rect::new(0, 0, 4, 40);
        let n = notifications(&[("hi", Severity::Info)]);
        let (toasts, overflow) = layout(viewport, TOP_MARGIN, &n, 5);
        assert!(toasts.is_empty());
        assert_eq!(overflow, 1);
    }

    #[test]
    fn slide_shift_eases_in_and_settles() {
        let now = Instant::now();
        // At creation, fully shifted left by the slide distance.
        assert_eq!(slide_shift(now, None, now, SLIDE_DISTANCE), SLIDE_DISTANCE);
        // Once the slide completes, resting with no shift.
        assert_eq!(slide_shift(now, None, now + SLIDE, SLIDE_DISTANCE), 0);
    }

    #[test]
    fn fade_progress_only_inside_window() {
        let now = Instant::now();
        let expires = now + Duration::from_secs(1);
        assert_eq!(fade_progress(Some(expires), now), 0.0); // before the window
        assert!(fade_progress(Some(expires), expires - FADE / 2) > 0.0); // inside
        assert_eq!(fade_progress(None, now), 0.0); // sticky never fades
    }

    #[test]
    fn slide_shift_full_at_end_of_fade() {
        let now = Instant::now();
        let expires = now + Duration::from_secs(10);
        // Settled, then fully faded out at expiry → shifted out by the full distance.
        assert_eq!(
            slide_shift(now, Some(expires), expires, SLIDE_DISTANCE),
            SLIDE_DISTANCE
        );
    }

    #[test]
    fn is_animating_during_slide_and_fade_only() {
        let now = Instant::now();
        let expires = now + Duration::from_secs(10);
        assert!(is_animating(now, Some(expires), now)); // sliding in
        assert!(!is_animating(now, Some(expires), now + SLIDE)); // settled, counting down
        assert!(is_animating(now, Some(expires), expires - FADE / 2)); // fading out
    }

    #[test]
    fn next_redraw_targets_fade_start_or_expiry() {
        let n = notifications(&[("a", Severity::Info)]);
        let expires = n.iter().next().unwrap().expires_at.unwrap();
        assert_eq!(next_redraw(&n, true), Some(expires - FADE));
        assert_eq!(next_redraw(&n, false), Some(expires));
    }

    #[test]
    fn layout_stops_when_out_of_vertical_room() {
        // Tall enough for exactly one 3-row box (rows 1..4) before the reserved
        // bottom rows; the second is pushed to overflow.
        let viewport = Rect::new(0, 0, 100, 7);
        let n = notifications(&[("one", Severity::Info), ("two", Severity::Info)]);
        let (toasts, overflow) = layout(viewport, TOP_MARGIN, &n, 5);
        assert_eq!(toasts.len(), 1);
        assert_eq!(overflow, 1);
    }
}

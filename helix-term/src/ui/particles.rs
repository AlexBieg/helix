//! Particle ring animation that plays around the cursor on mode switch.
//!
//! When switching editor modes a ring of particles briefly expands outward from
//! the cursor position, providing a subtle visual cue. The effect consists of
//! particles placed evenly around a circle that grow in radius over ~300 ms then
//! fade away.

use std::time::{Duration, Instant};

use helix_view::graphics::{Color, Rect, Style};
use tui::buffer::Buffer as Surface;

use crate::compositor::Context;

/// How long the full particle animation lasts.
pub const DURATION: Duration = Duration::from_millis(300);

/// Number of particles in the ring.
const PARTICLE_COUNT: usize = 12;

/// Starting radius (in cells) of the ring. Particles appear at this distance
/// from the cursor and expand outward.
const START_RADIUS: f32 = 0.5;

/// Maximum radius (in cells) the ring reaches before fading.
const MAX_RADIUS: f32 = 5.0;

/// Characters used for particles, cycled through by position.
const PARTICLE_CHARS: &[&str] = &["\u{2022}", "\u{2981}", "\u{b7}", "\u{2726}"];

/// Tracks the particle animation for a single mode-switch event.
#[derive(Debug, Clone)]
pub struct ModeSwitchAnimation {
    /// Time at which the animation started.
    pub started: Instant,
    /// Screen position of the cursor when the mode was switched.
    pub col: f32,
    pub row: f32,
    /// The new mode (used to pick particle color).
    pub mode_index: u8,
}

impl ModeSwitchAnimation {
    pub fn new(now: Instant, col: u16, row: u16, mode_index: u8) -> Self {
        Self {
            started: now,
            col: col as f32,
            row: row as f32,
            mode_index,
        }
    }

    /// Progress in `[0, 1]` of the animation.
    pub fn progress(&self, now: Instant) -> f32 {
        let elapsed = now.saturating_duration_since(self.started);
        (elapsed.as_secs_f32() / DURATION.as_secs_f32()).clamp(0.0, 1.0)
    }

    /// Returns `true` if the animation is still running.
    pub fn is_active(&self, now: Instant) -> bool {
        self.progress(now) < 1.0
    }
}

/// Render the particle ring on top of the editor surface when a mode-switch
/// animation is active.
pub fn render(
    viewport: Rect,
    surface: &mut Surface,
    cx: &mut Context,
    anim_state: &mut Option<ModeSwitchAnimation>,
) {
    let now = std::time::Instant::now();
    let anim = match anim_state {
        Some(anim) if anim.is_active(now) => anim.clone(),
        _ => {
            *anim_state = None;
            return;
        }
    };

    let t = anim.progress(now);

    let expand = ease_out_cubic((t / 0.5).min(1.0));
    let fade = if t > 0.5 {
        ease_in_cubic((t - 0.5) / 0.5)
    } else {
        0.0
    };

    let radius = START_RADIUS + (MAX_RADIUS - START_RADIUS) * expand;
    let alpha = 1.0 - fade;

    let theme_fg = particle_color(&cx.editor.theme, anim.mode_index);
    let fg = fade_color(theme_fg, alpha);
    let style = Style::default().fg(fg);

    let two_pi = 2.0 * std::f32::consts::PI;

    for i in 0..PARTICLE_COUNT {
        let angle = (i as f32 / PARTICLE_COUNT as f32) * two_pi;
        let px = anim.col + angle.cos() * radius;
        let py = anim.row + angle.sin() * radius * 0.5;

        let col = px.round() as i32;
        let row = py.round() as i32;

        if col < viewport.x as i32
            || col >= (viewport.x + viewport.width) as i32
            || row < viewport.y as i32
            || row >= (viewport.y + viewport.height) as i32
        {
            continue;
        }

        let ch = PARTICLE_CHARS[i % PARTICLE_CHARS.len()];
        surface.set_string(col as u16, row as u16, ch, style);
    }

    helix_event::request_redraw();
}

/// Pick a particle color based on the target mode. Checks theme keys first,
/// returning the mode accent color (the statusline background, not its text).
fn particle_color(theme: &helix_view::Theme, mode_index: u8) -> Color {
    let scope = match mode_index {
        0 => "ui.statusline.normal",
        1 => "ui.statusline.select",
        2 => "ui.statusline.insert",
        _ => "ui.statusline",
    };

    let from_theme = theme
        .try_get_exact(scope)
        .or_else(|| theme.try_get_exact("ui.statusline"));

    if let Some(style) = from_theme {
        // The statusline background is the mode accent color (blue/green/purple).
        if let Some(bg) = style.bg {
            if bg != Color::Reset {
                return bg;
            }
        }
        // Fall back to text color if no bg (unusual, but possible).
        if let Some(fg) = style.fg {
            if fg != Color::Reset {
                return fg;
            }
        }
    }

    // Bright fallbacks: Normal=blue, Select=purple, Insert=green
    match mode_index {
        0 => Color::Rgb(120, 160, 255),
        1 => Color::Rgb(210, 140, 255),
        2 => Color::Rgb(130, 210, 130),
        _ => Color::Rgb(190, 190, 190),
    }
}

/// Blend a color toward black based on alpha.
fn fade_color(color: Color, alpha: f32) -> Color {
    if alpha >= 1.0 {
        return color;
    }
    let alpha = alpha.clamp(0.0, 1.0);
    match color {
        Color::Rgb(r, g, b) => Color::Rgb(
            (r as f32 * alpha).round() as u8,
            (g as f32 * alpha).round() as u8,
            (b as f32 * alpha).round() as u8,
        ),
        other => other,
    }
}

fn ease_out_cubic(t: f32) -> f32 {
    1.0 - (1.0 - t).powi(3)
}

fn ease_in_cubic(t: f32) -> f32 {
    t * t * t
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn animation_starts_at_zero() {
        let now = Instant::now();
        let anim = ModeSwitchAnimation::new(now, 10, 5, 0);
        assert_eq!(anim.progress(now), 0.0);
        assert!(anim.is_active(now));
    }

    #[test]
    fn animation_ends_after_duration() {
        let now = Instant::now();
        let anim = ModeSwitchAnimation::new(now, 10, 5, 0);
        assert_eq!(anim.progress(now + DURATION), 1.0);
        assert!(!anim.is_active(now + DURATION));
    }

    #[test]
    fn animation_clamps_at_one() {
        let now = Instant::now();
        let anim = ModeSwitchAnimation::new(now, 10, 5, 0);
        assert_eq!(anim.progress(now + 10 * DURATION), 1.0);
    }

    #[test]
    fn fade_color_dims_rgb() {
        let c = Color::Rgb(200, 100, 50);
        assert_eq!(fade_color(c, 0.5), Color::Rgb(100, 50, 25));
        assert_eq!(fade_color(c, 0.0), Color::Rgb(0, 0, 0));
        assert_eq!(fade_color(c, 1.0), Color::Rgb(200, 100, 50));
    }
}

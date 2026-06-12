//! Small, terminal-friendly animation helpers shared across UI components.
//!
//! These are pure functions of elapsed time so callers stay testable: a
//! component records when it first rendered and derives its current appearance
//! from `now`.

use std::time::{Duration, Instant};

use helix_view::graphics::Color;

/// Default duration of a component's entrance animation.
pub const ENTRANCE: Duration = Duration::from_millis(150);

pub fn clamp01(value: f32) -> f32 {
    value.clamp(0.0, 1.0)
}

pub fn ease_out_cubic(t: f32) -> f32 {
    1.0 - (1.0 - t).powi(3)
}

/// Progress in `[0, 1]` of an entrance animation that began at `started`.
/// Returns `1.0` (finished) when `started` is `None`.
pub fn entrance_progress(started: Option<Instant>, now: Instant, duration: Duration) -> f32 {
    match started {
        Some(started) => {
            clamp01(now.saturating_duration_since(started).as_secs_f32() / duration.as_secs_f32())
        }
        None => 1.0,
    }
}

/// Linearly interpolate between two colors. Only RGB colors can be blended; if
/// either endpoint is a named/indexed color the target `to` is returned, so the
/// animation simply doesn't run on those themes.
pub fn blend(from: Color, to: Color, t: f32) -> Color {
    match (from, to) {
        (Color::Rgb(r0, g0, b0), Color::Rgb(r1, g1, b1)) => {
            let lerp = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t).round() as u8;
            Color::Rgb(lerp(r0, r1), lerp(g0, g1), lerp(b0, b1))
        }
        _ => to,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entrance_progress_clamps_and_finishes() {
        let now = Instant::now();
        assert_eq!(entrance_progress(None, now, ENTRANCE), 1.0);
        assert_eq!(entrance_progress(Some(now), now, ENTRANCE), 0.0);
        assert_eq!(entrance_progress(Some(now), now + ENTRANCE, ENTRANCE), 1.0);
        assert_eq!(entrance_progress(Some(now), now + 10 * ENTRANCE, ENTRANCE), 1.0);
    }

    #[test]
    fn blend_interpolates_rgb() {
        let a = Color::Rgb(0, 0, 0);
        let b = Color::Rgb(100, 200, 50);
        assert_eq!(blend(a, b, 0.0), Color::Rgb(0, 0, 0));
        assert_eq!(blend(a, b, 1.0), Color::Rgb(100, 200, 50));
        assert_eq!(blend(a, b, 0.5), Color::Rgb(50, 100, 25));
    }

    #[test]
    fn blend_skips_non_rgb() {
        assert_eq!(blend(Color::Red, Color::Rgb(1, 2, 3), 0.5), Color::Rgb(1, 2, 3));
        assert_eq!(blend(Color::Rgb(1, 2, 3), Color::Blue, 0.5), Color::Blue);
    }
}

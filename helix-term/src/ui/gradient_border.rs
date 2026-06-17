use helix_view::{
    editor::{GradientBorderConfig, GradientDirection},
    graphics::{Color, Rect},
    theme::Theme,
};
use tui::buffer::Buffer as Surface;

type Rgb = (u8, u8, u8);

pub struct GradientBorder {
    config: GradientBorderConfig,
    animation_frame: u32,
    start_rgb: Rgb,
    end_rgb: Rgb,
    middle_rgb: Option<Rgb>,
}

impl GradientBorder {
    pub fn new(config: GradientBorderConfig) -> Self {
        let (start_rgb, end_rgb, middle_rgb) = Self::compute_cached_colors(&config);
        Self {
            config,
            animation_frame: 0,
            start_rgb,
            end_rgb,
            middle_rgb,
        }
    }

    pub fn tick(&mut self) {
        if self.config.animation_speed > 0 {
            self.animation_frame = self.animation_frame.wrapping_add(1);
        }
    }

    fn parse_hex_color(hex: &str) -> Option<Rgb> {
        if hex.len() != 7 || !hex.starts_with('#') {
            return None;
        }

        let r = u8::from_str_radix(&hex[1..3], 16).ok()?;
        let g = u8::from_str_radix(&hex[3..5], 16).ok()?;
        let b = u8::from_str_radix(&hex[5..7], 16).ok()?;

        Some((r, g, b))
    }

    fn compute_cached_colors(config: &GradientBorderConfig) -> (Rgb, Rgb, Option<Rgb>) {
        let start_rgb = Self::parse_hex_color(&config.start_color).unwrap_or((138, 43, 226));
        let end_rgb = Self::parse_hex_color(&config.end_color).unwrap_or((0, 191, 255));
        let middle_rgb = if config.middle_color.is_empty() {
            None
        } else {
            Self::parse_hex_color(&config.middle_color)
        };
        (start_rgb, end_rgb, middle_rgb)
    }

    fn interpolate_color(start: Rgb, end: Rgb, ratio: f32) -> Color {
        let ratio = ratio.clamp(0.0, 1.0);
        let r = (start.0 as f32 + (end.0 as f32 - start.0 as f32) * ratio) as u8;
        let g = (start.1 as f32 + (end.1 as f32 - start.1 as f32) * ratio) as u8;
        let b = (start.2 as f32 + (end.2 as f32 - start.2 as f32) * ratio) as u8;
        Color::Rgb(r, g, b)
    }

    fn interpolate_three_colors(start: Rgb, middle: Rgb, end: Rgb, ratio: f32) -> Color {
        let ratio = ratio.clamp(0.0, 1.0);
        if ratio < 0.5 {
            Self::interpolate_color(start, middle, ratio * 2.0)
        } else {
            Self::interpolate_color(middle, end, (ratio - 0.5) * 2.0)
        }
    }

    fn get_gradient_color(&mut self, x: u16, y: u16, area: Rect) -> Color {
        let start_color = self.start_rgb;
        let end_color = self.end_rgb;

        let animation_offset = if self.config.animation_speed > 0 {
            (self.animation_frame as f32 * self.config.animation_speed as f32 * 0.01) % 1.0
        } else {
            0.0
        };

        let ratio = match self.config.direction {
            GradientDirection::Horizontal => {
                let base_ratio = (x - area.x) as f32 / area.width.max(1) as f32;
                (base_ratio + animation_offset) % 1.0
            }
            GradientDirection::Vertical => {
                let base_ratio = (y - area.y) as f32 / area.height.max(1) as f32;
                (base_ratio + animation_offset) % 1.0
            }
            GradientDirection::Diagonal => {
                let base_ratio =
                    ((x - area.x) + (y - area.y)) as f32 / (area.width + area.height).max(1) as f32;
                (base_ratio + animation_offset) % 1.0
            }
            GradientDirection::Radial => {
                let center_x = area.x + area.width / 2;
                let center_y = area.y + area.height / 2;
                let distance = ((x as f32 - center_x as f32).powi(2)
                    + (y as f32 - center_y as f32).powi(2))
                .sqrt();
                let max_distance = (area.width.max(area.height) / 2) as f32;
                let base_ratio = (distance / max_distance.max(1.0)).min(1.0);
                (base_ratio + animation_offset) % 1.0
            }
        };

        if let Some(middle_color) = self.middle_rgb {
            return Self::interpolate_three_colors(start_color, middle_color, end_color, ratio);
        }

        Self::interpolate_color(start_color, end_color, ratio)
    }

    fn get_border_chars(thickness: u8, rounded: bool) -> Vec<&'static str> {
        match (thickness, rounded) {
            (1, false) => vec!["─", "│", "┌", "┐", "└", "┘"],
            (1, true) => vec!["─", "│", "╭", "╮", "╰", "╯"],
            (2, false) => vec!["━", "┃", "┏", "┓", "┗", "┛"],
            (2, true) => vec!["━", "┃", "┏", "┓", "┗", "┛"],
            (3, false) => vec!["═", "║", "╔", "╗", "╚", "╝"],
            (3, true) => vec!["═", "║", "╔", "╗", "╚", "╝"],
            (4, _) => vec!["▄", "█", "█", "█", "█", "█"],
            (5, _) => vec!["▀", "█", "█", "█", "█", "█"],
            _ => vec!["─", "│", "┌", "┐", "└", "┘"],
        }
    }

    pub fn render(&mut self, area: Rect, surface: &mut Surface, _theme: &Theme, rounded: bool) {
        if !self.config.enable || area.width < 2 || area.height < 2 {
            return;
        }

        let border_chars = Self::get_border_chars(self.config.thickness, rounded);
        let [horizontal, vertical, top_left, top_right, bottom_left, bottom_right] = [
            border_chars[0],
            border_chars[1],
            border_chars[2],
            border_chars[3],
            border_chars[4],
            border_chars[5],
        ];

        let border_style = Style::default();

        for x in area.left()..area.right() {
            let color = self.get_gradient_color(x, area.top(), area);
            let style = border_style.fg(color);
            let symbol = if x == area.left() {
                top_left
            } else if x == area.right() - 1 {
                top_right
            } else {
                horizontal
            };

            if let Some(cell) = surface.get_mut(x, area.top()) {
                cell.set_symbol(symbol).set_style(style);
            }
        }

        let bottom_y = area.bottom() - 1;
        for x in area.left()..area.right() {
            let color = self.get_gradient_color(x, bottom_y, area);
            let style = border_style.fg(color);
            let symbol = if x == area.left() {
                bottom_left
            } else if x == area.right() - 1 {
                bottom_right
            } else {
                horizontal
            };

            if let Some(cell) = surface.get_mut(x, bottom_y) {
                cell.set_symbol(symbol).set_style(style);
            }
        }

        for y in (area.top() + 1)..(area.bottom() - 1) {
            let color = self.get_gradient_color(area.left(), y, area);
            let style = border_style.fg(color);
            if let Some(cell) = surface.get_mut(area.left(), y) {
                cell.set_symbol(vertical).set_style(style);
            }

            let right_x = area.right() - 1;
            let color = self.get_gradient_color(right_x, y, area);
            let style = border_style.fg(color);
            if let Some(cell) = surface.get_mut(right_x, y) {
                cell.set_symbol(vertical).set_style(style);
            }
        }

        self.tick();
    }

    pub fn from_theme(_theme: &Theme, config: &GradientBorderConfig) -> Self {
        let mut border_config = config.clone();

        if Self::parse_hex_color(&border_config.start_color).is_none() {
            border_config.start_color = "#8A2BE2".to_string();
        }
        if Self::parse_hex_color(&border_config.end_color).is_none() {
            border_config.end_color = "#00BFFF".to_string();
        }

        Self::new(border_config)
    }

    /// Static helper to get a color at a given ratio from a config, without
    /// creating a full GradientBorder instance.
    pub fn interpolate_from_config(config: &GradientBorderConfig, ratio: f32) -> Color {
        let (start, end, middle) = Self::compute_cached_colors(config);
        let ratio = ratio.clamp(0.0, 1.0);
        if let Some(mid) = middle {
            Self::interpolate_three_colors(start, mid, end, ratio)
        } else {
            Self::interpolate_color(start, end, ratio)
        }
    }
}

use helix_view::graphics::Style;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_hex_color_valid() {
        assert_eq!(
            GradientBorder::parse_hex_color("#FF0000"),
            Some((255, 0, 0))
        );
        assert_eq!(
            GradientBorder::parse_hex_color("#00FF00"),
            Some((0, 255, 0))
        );
        assert_eq!(
            GradientBorder::parse_hex_color("#0000FF"),
            Some((0, 0, 255))
        );
    }

    #[test]
    fn test_parse_hex_color_invalid() {
        assert_eq!(GradientBorder::parse_hex_color("FF0000"), None);
        assert_eq!(GradientBorder::parse_hex_color("#FF00"), None);
        assert_eq!(GradientBorder::parse_hex_color("#GGGGGG"), None);
        assert_eq!(GradientBorder::parse_hex_color(""), None);
    }

    #[test]
    fn test_interpolate_color() {
        let color = GradientBorder::interpolate_color((255, 0, 0), (0, 0, 255), 0.0);
        assert_eq!(color, Color::Rgb(255, 0, 0));

        let color = GradientBorder::interpolate_color((255, 0, 0), (0, 0, 255), 1.0);
        assert_eq!(color, Color::Rgb(0, 0, 255));

        let color = GradientBorder::interpolate_color((255, 0, 0), (0, 0, 255), 0.5);
        assert_eq!(color, Color::Rgb(127, 0, 127));
    }

    #[test]
    fn test_interpolate_three_colors() {
        let color = GradientBorder::interpolate_three_colors(
            (255, 0, 0),
            (0, 255, 0),
            (0, 0, 255),
            0.0,
        );
        assert_eq!(color, Color::Rgb(255, 0, 0));

        let color = GradientBorder::interpolate_three_colors(
            (255, 0, 0),
            (0, 255, 0),
            (0, 0, 255),
            0.5,
        );
        assert_eq!(color, Color::Rgb(0, 255, 0));

        let color = GradientBorder::interpolate_three_colors(
            (255, 0, 0),
            (0, 255, 0),
            (0, 0, 255),
            1.0,
        );
        assert_eq!(color, Color::Rgb(0, 0, 255));
    }

    #[test]
    fn test_border_chars() {
        let chars = GradientBorder::get_border_chars(1, false);
        assert_eq!(chars, vec!["─", "│", "┌", "┐", "└", "┘"]);

        let chars = GradientBorder::get_border_chars(1, true);
        assert_eq!(chars, vec!["─", "│", "╭", "╮", "╰", "╯"]);
    }
}

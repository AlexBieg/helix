use crate::compositor::{Component, Context, Event, EventResult};
use crate::ui::gradient_border::GradientBorder;
use crate::ui::prompt::{Completion, Prompt, PromptEvent};
use helix_core::Position;
use helix_core::unicode::width::UnicodeWidthStr;
use helix_view::{
    graphics::{CursorKind, Rect},
    Editor,
};
use std::borrow::Cow;
use tui::{
    buffer::Buffer as Surface,
    widgets::{Block, BorderType, Widget},
};

pub struct CmdlinePopup {
    pub prompt: Prompt,
    popup_area: Rect,
    min_width: u16,
    max_width: u16,
    gradient_border: Option<GradientBorder>,
}

impl CmdlinePopup {
    pub fn new(
        prompt_text: Cow<'static, str>,
        history_register: Option<char>,
        completion_fn: impl FnMut(&Editor, &str) -> Vec<Completion> + 'static,
        callback_fn: impl FnMut(&mut Context, &str, PromptEvent) + 'static,
        config: &helix_view::editor::CmdlineConfig,
    ) -> Self {
        Self {
            prompt: Prompt::new(prompt_text, history_register, completion_fn, callback_fn),
            popup_area: Rect::default(),
            min_width: config.min_popup_width,
            max_width: config.max_popup_width,
            gradient_border: None,
        }
    }

    pub fn with_line(mut self, line: String, editor: &Editor) -> Self {
        self.prompt = self.prompt.with_line(line, editor);
        self
    }

    pub fn with_language(
        mut self,
        language: &'static str,
        loader: std::sync::Arc<arc_swap::ArcSwap<helix_core::syntax::Loader>>,
    ) -> Self {
        self.prompt = self.prompt.with_language(language, loader);
        self
    }

    pub fn with_border(mut self) -> Self {
        self.prompt = self.prompt.with_border();
        self
    }

    fn get_command_icon<'a>(&self, config: &'a helix_view::editor::CmdlineIcons) -> &'a str {
        let prompt = self.prompt.prompt();
        match prompt {
            s if s.starts_with("search:") || s == "Search" => &config.search,
            s if s.starts_with('/') || s.starts_with('?') => &config.search,
            "Cmdline" | ":" => &config.command,
            "insert-output:" | "append-output:" | "pipe:" | "pipe-to:" => &config.shell,
            s if s.starts_with('!') => &config.shell,
            s if s == "shell:" => &config.shell,
            _ => &config.general,
        }
    }

    fn calculate_popup_area(&self, viewport: Rect) -> Rect {
        let content_width = self.prompt.line().width().max(self.min_width as usize);
        let width = (content_width as u16 + 4)
            .min(self.max_width)
            .min(viewport.width.saturating_sub(4));

        let height = 3;

        let x = viewport.x + (viewport.width.saturating_sub(width)) / 2;
        let y = viewport.y + (viewport.height.saturating_sub(height)) / 3;

        Rect::new(x, y, width, height)
    }

    fn render_popup(&mut self, area: Rect, surface: &mut Surface, cx: &mut Context) {
        let popup_area = self.calculate_popup_area(area);
        self.popup_area = popup_area;

        let theme = &cx.editor.theme;
        let config = &cx.editor.config().gradient_borders;

        surface.clear_with(popup_area, theme.get("ui.background"));

        let inner_area = if config.enable {
            if self.gradient_border.is_none() {
                self.gradient_border = Some(GradientBorder::from_theme(theme, config));
            }

            if let Some(ref mut gradient_border) = self.gradient_border {
                let rounded = cx.editor.config().rounded_corners;
                gradient_border.render(popup_area, surface, theme, rounded);
            }

            let t: u16 = config.thickness as u16;
            Rect {
                x: popup_area.x + t,
                y: popup_area.y + t,
                width: popup_area.width.saturating_sub(t * 2),
                height: popup_area.height.saturating_sub(t * 2),
            }
        } else {
            let border_style = theme.get("ui.popup.border");
            let border_type = BorderType::new(cx.editor.config().rounded_corners);
            let block = Block::bordered()
                .border_type(border_type)
                .border_style(border_style);

            let inner_area = block.inner(popup_area);
            block.render(popup_area, surface);
            inner_area
        };

        let cmdline_config = &cx.editor.config().cmdline;
        let icon = if cmdline_config.show_icons {
            self.get_command_icon(&cmdline_config.icons)
        } else {
            ""
        };

        let prefix_width = if icon.is_empty() {
            0
        } else {
            surface.set_string(inner_area.x, inner_area.y, icon, theme.get("ui.text"));
            icon.width() as u16 + 1
        };

        let input_area = Rect::new(
            inner_area.x + prefix_width,
            inner_area.y,
            inner_area.width.saturating_sub(prefix_width),
            1,
        );

        self.render_input_text(input_area, surface, cx);

        let doc = (self.prompt.doc_fn)(self.prompt.line()).map(|d| d.to_string());
        if let Some(doc) = doc {
            self.render_doc(popup_area, area, &doc, surface, cx);
        }

        if !self.prompt.completions().is_empty() {
            self.render_completion_popup(popup_area, surface, cx);
        }
    }

    fn render_input_text(&mut self, area: Rect, surface: &mut Surface, cx: &mut Context) {
        let theme = &cx.editor.theme;
        let text_style = theme.get("ui.text");
        let line_width = area.width as usize;

        self.prompt.update_scroll_anchor(line_width);

        let anchor = self.prompt.anchor();
        let line = self.prompt.line();
        let visible_text = &line[anchor..];

        surface.set_string_anchored(
            area.x,
            area.y,
            self.prompt.truncate_start(),
            self.prompt.truncate_end(),
            visible_text,
            line_width,
            |_| text_style,
        );
    }

    fn render_doc(
        &mut self,
        popup_area: Rect,
        viewport: Rect,
        doc: &str,
        surface: &mut Surface,
        cx: &mut Context,
    ) {
        let theme = &cx.editor.theme;
        let mut text = crate::ui::Text::new(doc.to_string());

        let max_width = 90u16;
        let text_width = max_width.saturating_sub(4);

        let (_width, height) = crate::ui::text::required_size(&text.contents, text_width);
        let height = height as u16;

        // Position above the popup, clamped within the viewport
        let y = if popup_area.y >= height + 2 {
            popup_area.y.saturating_sub(height + 2)
        } else {
            popup_area.y
        };

        let area = Rect {
            x: popup_area.x,
            y,
            width: popup_area.width.min(max_width),
            height: (height + 2).min(viewport.bottom().saturating_sub(y)),
        };

        if area.height < 3 || area.width < 4 {
            return;
        }

        surface.clear_with(area, theme.get("ui.help"));

        let border_type = BorderType::new(cx.editor.config().rounded_corners);
        let block = Block::bordered()
            .border_style(theme.get("ui.help"))
            .border_type(border_type);

        let inner = block.inner(area);
        block.render(area, surface);
        text.render(inner, surface, cx);
    }

    fn render_completion_popup(&mut self, base_area: Rect, surface: &mut Surface, cx: &Context) {
        let theme = &cx.editor.theme;
        let completion_bg = theme.get("ui.background");
        let selected_row_bg = theme.get("ui.menu.selected");

        let max_display_items = 10;
        let total_items = self.prompt.completions().len();
        let visible_items = total_items.min(max_display_items);
        let comp_height = visible_items as u16 + 2;
        let comp_width = base_area.width;
        let comp_area = Rect::new(
            base_area.x,
            base_area.y + base_area.height,
            comp_width,
            comp_height,
        );

        surface.clear_with(comp_area, completion_bg);

        let config = &cx.editor.config().gradient_borders;

        let inner_area = if config.enable {
            if let Some(ref mut gradient_border) = self.gradient_border {
                let rounded = cx.editor.config().rounded_corners;
                gradient_border.render(comp_area, surface, theme, rounded);
            }

            let t: u16 = config.thickness as u16;
            Rect {
                x: comp_area.x + t,
                y: comp_area.y + t,
                width: comp_area.width.saturating_sub(t * 2),
                height: comp_area.height.saturating_sub(t * 2),
            }
        } else {
            let border_type = BorderType::new(cx.editor.config().rounded_corners);
            let block = Block::bordered()
                .border_type(border_type)
                .border_style(theme.get("ui.popup.border"))
                .style(completion_bg);

            let inner_area = block.inner(comp_area);
            block.render(comp_area, surface);
            inner_area
        };

        let completions = self.prompt.completions();
        let selected_index = self.prompt.selection().unwrap_or(0);

        let scroll_offset = if selected_index >= max_display_items {
            selected_index.saturating_sub(max_display_items - 1)
        } else {
            0
        };

        for (display_idx, (completion_idx, (_range, completion, _desc))) in completions
            .iter()
            .enumerate()
            .skip(scroll_offset)
            .take(max_display_items)
            .enumerate()
        {
            let y = inner_area.y + display_idx as u16;
            let is_selected = self.prompt.selection() == Some(completion_idx);
            let item_style = if is_selected {
                let spaces = " ".repeat(inner_area.width as usize);
                surface.set_stringn(
                    inner_area.x,
                    y,
                    &spaces,
                    inner_area.width as usize,
                    selected_row_bg,
                );
                selected_row_bg
            } else {
                completion_bg.patch(completion.style)
            };

            surface.set_stringn(
                inner_area.x,
                y,
                &completion.content,
                inner_area.width as usize,
                item_style,
            );
        }

        if total_items > max_display_items {
            let scroll_indicator_style = theme.get("ui.text.inactive");
            if scroll_offset > 0 {
                surface.set_string(
                    inner_area.x + inner_area.width.saturating_sub(1),
                    inner_area.y,
                    "\u{2191}",
                    scroll_indicator_style,
                );
            }

            if scroll_offset + max_display_items < total_items {
                surface.set_string(
                    inner_area.x + inner_area.width.saturating_sub(1),
                    inner_area.y + inner_area.height.saturating_sub(1),
                    "\u{2193}",
                    scroll_indicator_style,
                );
            }
        }
    }
}

impl Component for CmdlinePopup {
    fn handle_event(&mut self, event: &Event, cx: &mut Context) -> EventResult {
        self.prompt.handle_event(event, cx)
    }

    fn render(&mut self, area: Rect, surface: &mut Surface, cx: &mut Context) {
        self.render_popup(area, surface, cx)
    }

    fn cursor(&self, _area: Rect, editor: &Editor) -> (Option<Position>, CursorKind) {
        let config = editor.config();
        let icon = if config.cmdline.show_icons {
            self.get_command_icon(&config.cmdline.icons)
        } else {
            ""
        };
        let prefix_width = if icon.is_empty() {
            0u16
        } else {
            icon.width() as u16 + 1
        };

        let inner_area = if editor.config().gradient_borders.enable {
            let t: u16 = editor.config().gradient_borders.thickness as u16;
            Rect {
                x: self.popup_area.x + t,
                y: self.popup_area.y + t,
                width: self.popup_area.width.saturating_sub(t * 2),
                height: self.popup_area.height.saturating_sub(t * 2),
            }
        } else {
            Block::bordered().inner(self.popup_area)
        };

        let input_area = Rect::new(
            inner_area.x + prefix_width,
            inner_area.y,
            inner_area.width.saturating_sub(prefix_width),
            1,
        );

        let byte_pos = self.prompt.position();
        let anchor = self.prompt.anchor();
        let line = self.prompt.line();

        let truncate_start = self.prompt.truncate_start();
        let visible_cursor_offset = if byte_pos >= anchor {
            line[anchor..byte_pos].width()
        } else {
            0
        };

        let indicator_offset = if truncate_start { 1 } else { 0 };
        let cursor_offset = (visible_cursor_offset + indicator_offset) as u16;
        let clamped_offset = cursor_offset.min(input_area.width.saturating_sub(1));
        let cursor_x = input_area.x as usize + clamped_offset as usize;
        let cursor_y = input_area.y as usize;

        (
            Some(Position::new(cursor_y, cursor_x)),
            editor
                .config()
                .cursor_shape
                .from_mode(helix_view::document::Mode::Insert),
        )
    }
}

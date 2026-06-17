use crate::compositor::{Component, Context};
use crate::ui::gradient_border::GradientBorder;
use helix_view::graphics::{Margin, Rect};
use helix_view::info::Info;
use tui::buffer::Buffer as Surface;
use tui::text::Text;
use tui::widgets::{Block, BorderType, Paragraph, Widget};

impl Component for Info {
    fn render(&mut self, viewport: Rect, surface: &mut Surface, cx: &mut Context) {
        let text_style = cx.editor.theme.get("ui.text.info");
        let popup_style = cx.editor.theme.get("ui.popup.info");

        let width = self.width + 2 + 2;
        let height = self.height + 2;
        let area = viewport.intersection(Rect::new(
            viewport.width.saturating_sub(width),
            viewport.height.saturating_sub(height + 2),
            width,
            height,
        ));
        surface.clear_with(area, popup_style);

        let inner = if cx.editor.config().gradient_borders.enable {
            let mut gb = GradientBorder::new(cx.editor.config().gradient_borders.clone());
            gb.render(area, surface, &cx.editor.theme, cx.editor.config().rounded_corners);

            let t: u16 = cx.editor.config().gradient_borders.thickness as u16;
            Rect {
                x: area.x + t,
                y: area.y + t,
                width: area.width.saturating_sub(t * 2),
                height: area.height.saturating_sub(t * 2),
            }
        } else {
            let border_type = BorderType::new(cx.editor.config().rounded_corners);
            let block = Block::bordered()
                .title(self.title.as_ref())
                .border_style(popup_style)
                .border_type(border_type);
            let inner = block.inner(area);
            block.render(area, surface);
            inner
        };

        let margin = Margin::horizontal(1);
        let inner = inner.inner(margin);

        // Render title on the gradient border top
        if cx.editor.config().gradient_borders.enable {
            let title = self.title.as_ref();
            if !title.is_empty() && area.width > title.len() as u16 + 4 {
                let title_start = area.x + 2;
                for (i, ch) in title.chars().enumerate() {
                    let x = title_start + i as u16;
                    if x < area.right() - 1 {
                        if let Some(cell) = surface.get_mut(x, area.y) {
                            cell.set_symbol(&ch.to_string()).set_style(text_style);
                        }
                    }
                }
            }
        }

        Paragraph::new(&Text::from(self.text.as_str()))
            .style(text_style)
            .render(inner, surface);
    }
}

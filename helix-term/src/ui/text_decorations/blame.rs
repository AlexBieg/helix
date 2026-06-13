use helix_core::{doc_formatter::FormattedGrapheme, Position};
use helix_view::{
    graphics::{Color, Modifier, Rect},
    theme::Style,
    Document, Theme,
};

use crate::ui::{
    document::{LinePos, TextRenderer},
    text_decorations::Decoration,
};

struct BlameStyles {
    base: Style,
    background: Style,
    commit: Style,
    author: Style,
    time: Style,
    summary: Style,
}

impl BlameStyles {
    fn from_theme(theme: &Theme) -> Self {
        // Use try_get_exact so we don't inherit from parent scopes like "ui".
        let base = theme.try_get_exact("ui.blame").unwrap_or_default();
        let background = theme
            .try_get_exact("ui.blame.background")
            .unwrap_or_else(|| base.bg(Color::Indexed(235)));

        let commit = styled(
            theme,
            "ui.blame.commit",
            base,
            Color::Indexed(172),
            Modifier::BOLD,
        );
        let author = styled(
            theme,
            "ui.blame.author",
            base,
            Color::Indexed(68),
            Modifier::empty(),
        );
        let time = styled(
            theme,
            "ui.blame.time",
            base,
            Color::Indexed(245),
            Modifier::DIM,
        );
        let summary = styled(
            theme,
            "ui.blame.summary",
            base,
            Color::Indexed(253),
            Modifier::empty(),
        );

        BlameStyles {
            base,
            background,
            commit,
            author,
            time,
            summary,
        }
    }
}

fn styled(
    theme: &Theme,
    scope: &str,
    base: Style,
    fallback_fg: Color,
    fallback_mod: Modifier,
) -> Style {
    match theme.try_get_exact(scope) {
        Some(s) => base.patch(s).add_modifier(fallback_mod),
        None => base.fg(fallback_fg).add_modifier(fallback_mod),
    }
}

pub struct BlameDecoration<'a> {
    doc: &'a Document,
    cursor: usize,
    cursor_line: usize,
    styles: BlameStyles,
}

impl<'a> BlameDecoration<'a> {
    pub fn new(doc: &'a Document, theme: &Theme, cursor: usize) -> Self {
        let cursor_line = doc.text().slice(..).char_to_line(cursor);
        BlameDecoration {
            doc,
            cursor,
            cursor_line,
            styles: BlameStyles::from_theme(theme),
        }
    }
}

impl Decoration for BlameDecoration<'_> {
    fn render_virt_lines(
        &mut self,
        renderer: &mut TextRenderer,
        pos: LinePos,
        virt_off: Position,
    ) -> Position {
        if !pos.first_visual_line || pos.doc_line != self.cursor_line {
            return Position::new(0, 0);
        }

        let y = pos.visual_line + virt_off.row as u16;

        // Fill the entire line background for both virtual rows.
        renderer.set_style(
            Rect::new(renderer.viewport.x, y, renderer.viewport.width, 2),
            self.styles.background,
        );

        let blame = match self.doc.blame() {
            Some(entries) => entries,
            None => return Position::new(0, 0),
        };

        let x = renderer.viewport.x;

        match blame.get(self.cursor_line).and_then(|e| e.as_ref()) {
            None => {
                renderer.set_string(x, y, "(uncommitted)", self.styles.summary);
                renderer.set_string(x, y + 1, "line not in HEAD", self.styles.time);
            }
            Some(entry) => {
                renderer.set_string(x, y, &entry.commit, self.styles.commit);
                let mut col = x + entry.commit.len() as u16;
                renderer.set_string(col, y, " ", self.styles.base);
                col += 1;
                renderer.set_string(col, y, &entry.author, self.styles.author);
                col += entry.author.len() as u16;
                renderer.set_string(col, y, "  ", self.styles.base);
                col += 2;
                renderer.set_string(col, y, &entry.time, self.styles.time);

                // Summary on its own line below
                let summary = if entry.summary.is_empty() {
                    "(no message)"
                } else {
                    &entry.summary
                };
                renderer.set_string(x, y + 1, summary, self.styles.summary);
            }
        }

        Position::new(2, 0)
    }

    fn reset_pos(&mut self, char_idx: usize) -> usize {
        if char_idx <= self.cursor {
            char_idx
        } else {
            self.cursor
        }
    }

    fn decorate_grapheme(
        &mut self,
        _renderer: &mut TextRenderer,
        grapheme: &FormattedGrapheme,
    ) -> usize {
        if grapheme.char_idx == self.cursor {
            usize::MAX
        } else {
            self.cursor
        }
    }
}

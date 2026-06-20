use helix_core::{doc_formatter::FormattedGrapheme, Position};
use helix_view::{
    graphics::{Modifier, Rect},
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
        let base = theme
            .try_get("ui.blame")
            .or_else(|| theme.try_get_exact("ui.virtual"))
            .unwrap_or_default();
        let background = theme
            .try_get_exact("ui.blame.background")
            .or_else(|| theme.try_get_exact("ui.popup"))
            .or_else(|| theme.try_get_exact("ui.statusline"))
            .unwrap_or(base);

        let commit = styled(theme, "ui.blame.commit", "constant", base, Modifier::BOLD);
        let author = styled(theme, "ui.blame.author", "function", base, Modifier::empty());
        let time = styled(theme, "ui.blame.time", "comment", base, Modifier::DIM);
        let summary = styled(theme, "ui.blame.summary", "string", base, Modifier::empty());

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
    fallback_scope: &str,
    base: Style,
    fallback_mod: Modifier,
) -> Style {
    match theme.try_get_exact(scope) {
        Some(s) => base.patch(s).add_modifier(fallback_mod),
        None => {
            let semantic = theme.try_get(fallback_scope).unwrap_or_default();
            base.patch(semantic).add_modifier(fallback_mod)
        }
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

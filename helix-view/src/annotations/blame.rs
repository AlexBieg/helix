use helix_core::{
    doc_formatter::FormattedGrapheme,
    text_annotations::LineAnnotation,
    Position,
};

use crate::Document;

pub(crate) struct BlameLineAnnotation<'a> {
    _doc: &'a Document,
    cursor: usize,
    cursor_line: usize,
}

impl<'a> BlameLineAnnotation<'a> {
    #[allow(clippy::new_ret_no_self)]
    pub(crate) fn new(
        doc: &'a Document,
        cursor: usize,
    ) -> Box<dyn LineAnnotation + 'a> {
        let cursor_line = doc.text().slice(..).char_to_line(cursor);
        Box::new(BlameLineAnnotation {
            _doc: doc,
            cursor,
            cursor_line,
        })
    }
}

impl LineAnnotation for BlameLineAnnotation<'_> {
    fn reset_pos(&mut self, char_idx: usize) -> usize {
        if char_idx <= self.cursor {
            char_idx
        } else {
            self.cursor
        }
    }

    fn process_anchor(&mut self, grapheme: &FormattedGrapheme) -> usize {
        if grapheme.char_idx == self.cursor {
            usize::MAX
        } else {
            self.cursor
        }
    }

    fn insert_virtual_lines(
        &mut self,
        _line_end_char_idx: usize,
        _line_end_visual_pos: Position,
        doc_line: usize,
    ) -> Position {
        if doc_line == self.cursor_line {
            Position::new(2, 0)
        } else {
            Position::new(0, 0)
        }
    }
}

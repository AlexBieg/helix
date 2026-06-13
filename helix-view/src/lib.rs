#[macro_use]
pub mod macros;

pub mod annotations;
pub mod clipboard;
pub mod document;
pub mod editor;
pub mod events;
pub mod expansion;
pub mod file_watcher;
pub mod graphics;
pub mod gutter;
pub mod handlers;
pub mod info;
pub mod input;
pub mod keyboard;
pub mod notification;
pub mod register;
pub mod session;
pub mod theme;
pub mod tree;
pub mod view;

use std::num::NonZeroUsize;

use serde::{Deserialize, Serialize};

// uses NonZeroUsize so Option<DocumentId> use a byte rather than two
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DocumentId(NonZeroUsize);

impl Default for DocumentId {
    fn default() -> DocumentId {
        DocumentId(NonZeroUsize::new(1).unwrap())
    }
}

#[cfg(test)]
impl DocumentId {
    /// Constructs a `DocumentId` with the given non-zero id, for use in tests
    /// that need several distinct ids without spinning up an `Editor`.
    pub(crate) fn new(id: usize) -> DocumentId {
        DocumentId(NonZeroUsize::new(id).expect("document id must be non-zero"))
    }
}

impl std::fmt::Display for DocumentId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!("{}", self.0))
    }
}

slotmap::new_key_type! {
    pub struct ViewId;
}

impl Serialize for ViewId {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u64(self.0.as_ffi())
    }
}

impl<'de> Deserialize<'de> for ViewId {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let val = u64::deserialize(deserializer)?;
        Ok(ViewId(slotmap::KeyData::from_ffi(val)))
    }
}

pub enum Align {
    Top,
    Center,
    Bottom,
}

pub fn align_view(doc: &mut Document, view: &View, align: Align) {
    let doc_text = doc.text().slice(..);
    let cursor = doc.selection(view.id).primary().cursor(doc_text);
    let viewport = view.inner_area(doc);
    let last_line_height = viewport.height.saturating_sub(1);
    let mut view_offset = doc.view_offset(view.id);

    let relative = match align {
        Align::Center => last_line_height / 2,
        Align::Top => 0,
        Align::Bottom => last_line_height,
    };

    let text_fmt = doc.text_format(viewport.width, None);
    (view_offset.anchor, view_offset.vertical_offset) = char_idx_at_visual_offset(
        doc_text,
        cursor,
        -(relative as isize),
        0,
        &text_fmt,
        &view.text_annotations(doc, None),
    );
    doc.set_view_offset(view.id, view_offset);
}

pub use document::Document;
pub use editor::Editor;
use helix_core::char_idx_at_visual_offset;
pub use theme::Theme;
pub use view::View;

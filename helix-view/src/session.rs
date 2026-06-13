use serde::{Deserialize, Serialize};

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Session {
    /// The currently focused document index (into the documents vec).
    pub active_document_index: usize,
    /// Open documents in order (first is the most recently active).
    pub documents: Vec<SessionDocument>,
    /// Window split layout tree.
    pub splits: SessionTree,
    /// Recently opened files in MRU order.
    #[serde(default)]
    pub recent_files: Vec<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct SessionDocument {
    /// Absolute path to the file, if it has one.
    pub path: Option<PathBuf>,
    /// Cursor position: line (1-indexed) and column (1-indexed).
    pub cursor_line: usize,
    pub cursor_col: usize,
    /// Selections (multiple cursors). Each selection is a pair of (anchor_line, anchor_col, head_line, head_col).
    pub selections: Vec<SessionSelection>,
    /// Scroll position: first visible line (1-indexed), horizontal scroll offset, and vertical offset.
    pub scroll_line: usize,
    pub scroll_col: usize,
    #[serde(default)]
    pub vertical_offset: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct SessionSelection {
    pub anchor_line: usize,
    pub anchor_col: usize,
    pub head_line: usize,
    pub head_col: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SessionTree {
    /// A leaf node: a single view showing a document at the given index in `documents`.
    View {
        document_index: usize,
    },
    /// A container splitting horizontally or vertically.
    Split {
        layout: SessionLayout,
        children: Vec<SessionTree>,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SessionLayout {
    Horizontal,
    Vertical,
}

impl Session {
    pub fn save_to_file(&self, path: &Path) -> anyhow::Result<()> {
        let serialized = toml::to_string_pretty(self).map_err(|e| anyhow::anyhow!(e))?;
        std::fs::write(path, serialized)?;
        Ok(())
    }

    pub fn load_from_file(path: &Path) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        toml::from_str(&content).map_err(|e| anyhow::anyhow!(e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_roundtrip_single_view() {
        let session = Session {
            active_document_index: 0,
            documents: vec![SessionDocument {
                path: Some(PathBuf::from("/tmp/test.rs")),
                cursor_line: 10,
                cursor_col: 5,
                selections: vec![SessionSelection {
                    anchor_line: 10,
                    anchor_col: 3,
                    head_line: 10,
                    head_col: 8,
                }],
                scroll_line: 5,
                scroll_col: 1,
                vertical_offset: 0,
            }],
            splits: SessionTree::View { document_index: 0 },
            recent_files: vec![PathBuf::from("/tmp/other.rs")],
        };

        let toml_str = toml::to_string_pretty(&session).unwrap();
        let restored: Session = toml::from_str(&toml_str).unwrap();

        assert_eq!(restored.active_document_index, 0);
        assert_eq!(restored.documents.len(), 1);
        assert_eq!(restored.documents[0].path, Some(PathBuf::from("/tmp/test.rs")));
        assert_eq!(restored.documents[0].cursor_line, 10);
        assert_eq!(restored.documents[0].cursor_col, 5);
        assert_eq!(restored.documents[0].scroll_line, 5);
        assert_eq!(restored.documents[0].scroll_col, 1);
        assert_eq!(restored.documents[0].vertical_offset, 0);
        assert_eq!(restored.documents[0].selections.len(), 1);
        assert_eq!(restored.documents[0].selections[0].anchor_line, 10);
        assert_eq!(restored.documents[0].selections[0].head_col, 8);
        assert!(matches!(restored.splits, SessionTree::View { document_index: 0 }));
        assert_eq!(restored.recent_files, vec![PathBuf::from("/tmp/other.rs")]);
    }

    #[test]
    fn test_session_roundtrip_split_tree() {
        let session = Session {
            active_document_index: 1,
            documents: vec![
                SessionDocument {
                    path: Some(PathBuf::from("/tmp/a.rs")),
                    cursor_line: 1,
                    cursor_col: 1,
                    selections: vec![],
                    scroll_line: 1,
                    scroll_col: 1,
                    vertical_offset: 0,
                },
                SessionDocument {
                    path: Some(PathBuf::from("/tmp/b.rs")),
                    cursor_line: 42,
                    cursor_col: 7,
                    selections: vec![],
                    scroll_line: 30,
                    scroll_col: 1,
                    vertical_offset: 2,
                },
            ],
            splits: SessionTree::Split {
                layout: SessionLayout::Horizontal,
                children: vec![
                    SessionTree::View { document_index: 0 },
                    SessionTree::View { document_index: 1 },
                ],
            },
            recent_files: vec![],
        };

        let toml_str = toml::to_string_pretty(&session).unwrap();
        let restored: Session = toml::from_str(&toml_str).unwrap();

        assert_eq!(restored.active_document_index, 1);
        assert_eq!(restored.documents.len(), 2);
        assert_eq!(restored.documents[1].vertical_offset, 2);

        match &restored.splits {
            SessionTree::Split { layout, children } => {
                assert!(matches!(layout, SessionLayout::Horizontal));
                assert_eq!(children.len(), 2);
            }
            _ => panic!("expected split tree"),
        }
    }

    #[test]
    fn test_session_roundtrip_nested_splits() {
        let session = Session {
            active_document_index: 0,
            documents: (0..4)
                .map(|i| SessionDocument {
                    path: Some(PathBuf::from(format!("/tmp/d{i}.rs"))),
                    cursor_line: 1,
                    cursor_col: 1,
                    selections: vec![],
                    scroll_line: 1,
                    scroll_col: 1,
                    vertical_offset: 0,
                })
                .collect(),
            splits: SessionTree::Split {
                layout: SessionLayout::Vertical,
                children: vec![
                    SessionTree::View { document_index: 0 },
                    SessionTree::Split {
                        layout: SessionLayout::Horizontal,
                        children: vec![
                            SessionTree::View { document_index: 1 },
                            SessionTree::View { document_index: 2 },
                        ],
                    },
                    SessionTree::View { document_index: 3 },
                ],
            },
            recent_files: vec![],
        };

        let toml_str = toml::to_string_pretty(&session).unwrap();
        let restored: Session = toml::from_str(&toml_str).unwrap();

        assert_eq!(restored.documents.len(), 4);
        assert!(matches!(restored.splits, SessionTree::Split { .. }));
    }

    #[test]
    fn test_session_deserialize_vertical_offset_missing() {
        // Old session files may lack vertical-offset; default should be 0.
        let toml = r#"
active-document-index = 0
recent-files = []
documents = [
    { path = "/tmp/x.rs", cursor-line = 3, cursor-col = 10, selections = [], scroll-line = 1, scroll-col = 1 },
]
[splits.view]
document_index = 0
"#;
        let session: Session = toml::from_str(toml).unwrap();
        assert_eq!(session.documents[0].vertical_offset, 0);
    }
}

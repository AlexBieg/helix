//! Snapshot-based editor context for the MCP server.
//!
//! The `McpContext` holds a `parking_lot::RwLock`-protected snapshot of the
//! current editor state. The snapshot is updated by `Application::render()`
//! on the main thread, and read by MCP session handlers on async tasks.
//!
//! This design avoids locking the entire `Editor` across threads while still
//! providing up-to-date workspace information to connected MCP clients.

use std::collections::HashMap;

use parking_lot::RwLock;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use tokio::sync::broadcast;

use crate::audit::AuditLogger;

/// Well-known LanguageServerId used for MCP agent diagnostics.
/// Must match the ID registered in the LSP registry at startup.
pub const MCP_AGENT_DIAG_ID: u64 = u64::MAX;

/// Information about an open file in the editor.
#[derive(Debug, Clone)]
pub struct FileInfo {
    /// The document ID as a string (e.g. "1", "2").
    pub doc_id: String,
    /// The file path, if the document has been saved.
    pub path: Option<String>,
    /// The language name (e.g. "rust", "markdown").
    pub language: Option<String>,
    /// The total number of lines in the document.
    pub line_count: usize,
    /// The full text content of the document.
    pub text: String,
    /// Whether the document has unsaved changes.
    pub modified: bool,
    /// Selection data for this document from its views.
    pub selections: Vec<SelectionData>,
}

/// Selection information for a document view.
#[derive(Debug, Clone, Default)]
pub struct SelectionData {
    /// Byte offset of the anchor.
    pub anchor_byte: usize,
    /// Byte offset of the cursor (head).
    pub cursor_byte: usize,
    /// Line number of the anchor (1-based).
    pub anchor_line: usize,
    /// Line number of the cursor (1-based).
    pub cursor_line: usize,
    /// Selected text, if any.
    pub text: String,
}

/// Diagnostic information for a document.
#[derive(Debug, Clone)]
pub struct DiagnosticData {
    /// Byte range `(start, end)` of the diagnostic.
    pub range: (usize, usize),
    /// Severity as a string: "error", "warning", "info", or "hint".
    pub severity: String,
    /// The diagnostic message.
    pub message: String,
    /// Optional error/warning code.
    pub code: Option<String>,
    /// Optional source (e.g. language server name).
    pub source: Option<String>,
}

/// A point-in-time snapshot of the editor workspace.
#[derive(Debug, Clone, Default)]
pub struct EditorSnapshot {
    /// The path of the currently focused file, if any.
    pub active_file: Option<String>,
    /// The current editor mode (e.g. "normal", "insert", "select").
    pub mode: String,
    /// Information about all open files.
    pub files: Vec<FileInfo>,
    /// Diagnostics across all open documents (from LSP).
    pub diagnostics: Vec<DiagnosticData>,
    /// Agent-provided diagnostics keyed by document ID.
    pub agent_diagnostics: HashMap<String, Vec<DiagnosticData>>,
}

impl EditorSnapshot {
    /// Find a file by its document ID.
    pub fn document_by_id(&self, doc_id: &str) -> Option<&FileInfo> {
        self.files.iter().find(|f| f.doc_id == doc_id)
    }
}

/// Shared context for MCP server operations.
///
/// Wraps an `EditorSnapshot` in a `parking_lot::RwLock` so the main thread
/// can write updates during rendering and MCP session handlers can read
/// concurrently without blocking.
pub struct McpContext {
    /// The current editor snapshot, updated by the main thread.
    snapshot: Arc<RwLock<EditorSnapshot>>,
    /// Broadcast channel to notify sessions of state updates.
    update_tx: broadcast::Sender<()>,
    /// Number of active client connections.
    active_connections: AtomicU32,
    /// Optional audit logger for Mutate-tier operations.
    pub audit_logger: Option<Arc<AuditLogger>>,
}

impl McpContext {
    /// Create a new `McpContext` with an empty snapshot.
    pub fn new() -> Self {
        let (update_tx, _) = broadcast::channel(64);
        McpContext {
            snapshot: Arc::new(RwLock::new(EditorSnapshot::default())),
            update_tx,
            active_connections: AtomicU32::new(0),
            audit_logger: AuditLogger::new().map(Arc::new),
        }
    }

    /// Take a read-only snapshot of the current editor state.
    pub fn snapshot(&self) -> EditorSnapshot {
        self.snapshot.read().clone()
    }

    /// Run a mutation against the editor snapshot.
    ///
    /// Acquires a write lock on the snapshot and passes a mutable reference
    /// to the closure. Used by Mutate-tier tools after confirmation gating.
    pub fn mutate<T>(&self, f: impl FnOnce(&mut EditorSnapshot) -> T) -> T {
        let mut snap = self.snapshot.write();
        f(&mut snap)
    }

    /// Add agent-provided diagnostics for a document URI.
    pub fn add_agent_diagnostics(&self, doc_id: &str, diagnostics: Vec<DiagnosticData>) {
        let mut snap = self.snapshot.write();
        snap.agent_diagnostics
            .insert(doc_id.to_string(), diagnostics);
    }

    /// Get agent-provided diagnostics for a document.
    pub fn get_agent_diagnostics(&self, doc_id: &str) -> Vec<DiagnosticData> {
        self.snapshot
            .read()
            .agent_diagnostics
            .get(doc_id)
            .cloned()
            .unwrap_or_default()
    }

    /// Get a clone of all agent diagnostics.
    pub fn all_agent_diagnostics(&self) -> HashMap<String, Vec<DiagnosticData>> {
        self.snapshot.read().agent_diagnostics.clone()
    }

    /// Load a file from disk into the snapshot. Used in headless mode to
    /// populate the snapshot with file arguments.
    pub fn load_file(&self, doc_id: &str, path: &str) {
        let text = std::fs::read_to_string(path).unwrap_or_default();
        let line_count = text.lines().count();
        let language = path.rsplit('.').next().map(|ext| ext.to_string());

        let file_info = FileInfo {
            doc_id: doc_id.to_string(),
            path: Some(path.to_string()),
            language,
            line_count,
            text,
            modified: false,
            selections: Vec::new(),
        };

        let mut snap = self.snapshot.write();
        // Remove any existing entry for this doc_id
        snap.files.retain(|f| f.doc_id != doc_id);
        snap.files.push(file_info);
    }

    /// Return the number of files currently in the snapshot.
    pub fn file_count(&self) -> usize {
        self.snapshot.read().files.len()
    }

    /// Return the number of active client connections.
    pub fn connection_count(&self) -> u32 {
        self.active_connections.load(Ordering::Relaxed)
    }

    /// Increment the active connection count.
    pub fn increment_connections(&self) {
        self.active_connections.fetch_add(1, Ordering::Relaxed);
    }

    /// Decrement the active connection count.
    pub fn decrement_connections(&self) {
        self.active_connections.fetch_sub(1, Ordering::Relaxed);
    }

    /// Subscribe to editor state updates.
    ///
    /// Returns a `Receiver` that will receive a unit signal whenever the
    /// editor snapshot has been updated.
    pub fn subscribe_to_updates(&self) -> broadcast::Receiver<()> {
        self.update_tx.subscribe()
    }

    /// Update the editor snapshot from the given editor state.
    ///
    /// Called from `Application::render()` on the main thread.
    pub fn update_snapshot(&self, editor: &helix_view::Editor) {
        let mode = format!("{}", editor.mode);
        let mut files = Vec::new();
        let mut diagnostics = Vec::new();

        for (doc_id, doc) in &editor.documents {
            let text: String = doc.text().slice(..).into();
            let line_count = text.lines().count();

            // Gather selections from views that target this document
            let mut doc_selections: Vec<SelectionData> = Vec::new();
            for (view, _) in editor.tree.views() {
                if view.doc == *doc_id {
                    let sel = doc.selection(view.id);
                    for range in sel.ranges() {
                        let rope = doc.text();
                        let anchor_byte = rope.char_to_byte(range.anchor);
                        let cursor_byte = rope.char_to_byte(range.head);
                        let anchor_line = rope.char_to_line(range.anchor) + 1;
                        let cursor_line = rope.char_to_line(range.head) + 1;
                        let selected_text: String = if range.anchor != range.head {
                            let from = range.from();
                            let to = range.to();
                            rope.slice(from..to).into()
                        } else {
                            String::new()
                        };
                        doc_selections.push(SelectionData {
                            anchor_byte,
                            cursor_byte,
                            anchor_line,
                            cursor_line,
                            text: selected_text,
                        });
                    }
                }
            }

            // Gather diagnostics for this document
            if let Some(uri) = doc.uri() {
                if let Some(diags) = editor.diagnostics.get(&uri) {
                    for (lsp_diag, provider) in diags {
                        let code = lsp_diag.code.as_ref().map(|c| match c {
                            helix_lsp::lsp::NumberOrString::Number(n) => n.to_string(),
                            helix_lsp::lsp::NumberOrString::String(s) => s.clone(),
                        });
                        let source = match provider {
                            helix_core::diagnostic::DiagnosticProvider::Lsp {
                                server_id,
                                identifier,
                            } => identifier.as_ref().map(|id| id.to_string()).or_else(|| {
                                editor
                                    .language_servers
                                    .get_by_id(*server_id)
                                    .map(|ls| ls.name().to_string())
                            }),
                        };
                        let range = (
                            lsp_diag.range.start.character as usize,
                            lsp_diag.range.end.character as usize,
                        );
                        diagnostics.push(DiagnosticData {
                            range,
                            severity: match lsp_diag.severity {
                                Some(helix_lsp::lsp::DiagnosticSeverity::ERROR) => {
                                    "error".to_string()
                                }
                                Some(helix_lsp::lsp::DiagnosticSeverity::WARNING) => {
                                    "warning".to_string()
                                }
                                Some(helix_lsp::lsp::DiagnosticSeverity::INFORMATION) => {
                                    "info".to_string()
                                }
                                Some(helix_lsp::lsp::DiagnosticSeverity::HINT) => {
                                    "hint".to_string()
                                }
                                _ => "info".to_string(),
                            },
                            message: lsp_diag.message.clone(),
                            code,
                            source,
                        });
                    }
                }
            }

            files.push(FileInfo {
                doc_id: doc_id.to_string(),
                path: doc.path().map(|p| p.to_string_lossy().to_string()),
                language: doc.language_name().map(|s| s.to_string()),
                line_count,
                text,
                modified: doc.is_modified(),
                selections: doc_selections,
            });
        }

        // Determine the active (focused) file
        let active_file = {
            let focused_view_id = editor.tree.focus;
            editor
                .tree
                .views()
                .find(|(view, _)| view.id == focused_view_id)
                .and_then(|(view, _)| {
                    editor
                        .documents
                        .get(&view.doc)
                        .and_then(|doc| doc.path())
                        .map(|p| p.to_string_lossy().to_string())
                })
        };

        let mut snap = self.snapshot.write();

        // MCP mutation persistence: preserve files that were modified by
        // MCP mutations (e.g. document_write, edit_apply) so they survive
        // the render cycle. Files with the `modified` flag set in the old
        // snapshot were touched by MCP; carry their text forward.
        for old_file in &snap.files {
            if old_file.modified {
                if let Some(new_file) = files.iter_mut().find(|f| f.doc_id == old_file.doc_id) {
                    new_file.text = old_file.text.clone();
                    new_file.line_count = old_file.line_count;
                    new_file.modified = old_file.modified;
                }
            }
        }

        // Preserve agent-pushed diagnostics across render cycles.
        // The main thread only updates LSP diagnostics; agent diagnostics
        // are managed exclusively by the diagnostics_publish tool.
        let agent_diagnostics = std::mem::take(&mut snap.agent_diagnostics);

        snap.active_file = active_file;
        snap.mode = mode;
        snap.files = files;
        snap.diagnostics = diagnostics;
        snap.agent_diagnostics = agent_diagnostics;

        // Notify subscribers that the snapshot has changed
        let _ = self.update_tx.send(());
    }
}

impl Default for McpContext {
    fn default() -> Self {
        Self::new()
    }
}

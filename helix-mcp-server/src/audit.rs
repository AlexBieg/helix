//! Audit logging for Mutate-tier MCP operations.
//!
//! Every Mutate-tier tool execution is logged to a JSON-lines file for
//! security tracking and debugging.

use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

use serde::Serialize;

/// A single audit log entry.
#[derive(Debug, Clone, Serialize)]
pub struct AuditEntry {
    /// ISO 8601 timestamp of the operation.
    pub timestamp: String,
    /// The name of the tool that was executed.
    pub tool: String,
    /// The document ID that was affected.
    pub doc_id: String,
    /// A human-readable summary of what was done.
    pub summary: String,
    /// An identifier for the connected client, if available.
    pub client_id: Option<String>,
}

/// Thread-safe audit logger that appends JSON-lines entries to a file.
pub struct AuditLogger {
    file: Mutex<std::fs::File>,
}

impl AuditLogger {
    /// Create a new `AuditLogger`.
    ///
    /// The log file is resolved as follows:
    /// 1. `HELIX_MCP_AUDIT_DIR` environment variable
    /// 2. `$HOME/.cache/helix/mcp-audit.log`
    /// 3. `$TMPDIR/helix-mcp-audit.log` (fallback)
    ///
    /// Returns `None` if the log file cannot be opened.
    pub fn new() -> Option<Self> {
        let log_dir = std::env::var("HELIX_MCP_AUDIT_DIR").unwrap_or_else(|_| {
            let home = std::env::var("HOME")
                .unwrap_or_else(|_| std::env::temp_dir().to_string_lossy().to_string());
            format!("{}/.cache/helix", home)
        });

        let log_path = PathBuf::from(&log_dir).join("mcp-audit.log");

        let _ = std::fs::create_dir_all(&log_dir);

        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
            .ok()?;

        Some(AuditLogger {
            file: Mutex::new(file),
        })
    }

    /// Append an audit entry to the log file.
    pub fn log(&self, entry: &AuditEntry) {
        if let Ok(mut f) = self.file.lock() {
            let json = serde_json::to_string(entry).unwrap_or_default();
            let _ = writeln!(f, "{}", json);
        }
    }
}

//! Three-tier security model for MCP tool operations.
//!
//! Each tool is assigned an `OperationTier`:
//! - **Read**: Read-only operations (default for unknown tools).
//! - **Preview**: Non-destructive preview of potential changes.
//! - **Mutate**: Destructive operations that modify editor state.
//!
//! Mutate-tier operations require explicit user confirmation before execution.
//! The confirmation is handled via a `ConfirmationRequest` sent through a
//! channel to the main application thread.

/// The security tier for a tool operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationTier {
    /// Read-only operation — no confirmation needed.
    Read,
    /// Non-destructive preview — no confirmation needed.
    Preview,
    /// Destructive operation — requires user confirmation.
    Mutate,
}

/// A request for user confirmation before executing a Mutate-tier tool.
pub struct ConfirmationRequest {
    /// The name of the tool being confirmed.
    pub tool_name: String,
    /// A human-readable summary of what the tool will do.
    pub summary: String,
    /// The operation tier (always `Mutate` when sent through the channel).
    pub tier: OperationTier,
    /// Channel to send the user's response (`true` = approved, `false` = denied).
    pub response_tx: tokio::sync::oneshot::Sender<bool>,
}

/// Return the `OperationTier` for a given tool name.
pub fn tool_tier(name: &str) -> OperationTier {
    match name {
        "document_read" | "selection_read" | "search_text" | "diagnostics_read" | "lsp_request"
        | "workspace_info" => OperationTier::Read,
        "goto_position" | "selection_set" => OperationTier::Preview,
        "document_write" | "edit_apply" | "diagnostics_publish" | "diagnostics_clear" => OperationTier::Mutate,
        _ => OperationTier::Read,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_read_tier_tools() {
        assert_eq!(tool_tier("document_read"), OperationTier::Read);
        assert_eq!(tool_tier("selection_read"), OperationTier::Read);
        assert_eq!(tool_tier("search_text"), OperationTier::Read);
        assert_eq!(tool_tier("diagnostics_read"), OperationTier::Read);
        assert_eq!(tool_tier("lsp_request"), OperationTier::Read);
        assert_eq!(tool_tier("workspace_info"), OperationTier::Read);
    }

    #[test]
    fn test_preview_tier_tools() {
        assert_eq!(tool_tier("goto_position"), OperationTier::Preview);
        assert_eq!(tool_tier("selection_set"), OperationTier::Preview);
    }

    #[test]
    fn test_mutate_tier_tools() {
        assert_eq!(tool_tier("document_write"), OperationTier::Mutate);
        assert_eq!(tool_tier("edit_apply"), OperationTier::Mutate);
        assert_eq!(tool_tier("diagnostics_publish"), OperationTier::Mutate);
    }

    #[test]
    fn test_unknown_tool_defaults_to_read() {
        assert_eq!(tool_tier("nonexistent"), OperationTier::Read);
        assert_eq!(tool_tier(""), OperationTier::Read);
    }
}

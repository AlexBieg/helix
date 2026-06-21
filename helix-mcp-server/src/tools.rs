//! MCP tool implementations for Helix.
//!
//! Phase 1: three read-only tools (workspace_info, document_read, goto_position).
//! Phase 2: seven additional tools (document_write, selection_read, selection_set,
//!          edit_apply, search_text, diagnostics_read, lsp_request).
//!
//! Each tool function is marked with `#[doc = "..."]` noting its security tier.

use anyhow::{anyhow, Result};
use serde_json::Value;

use crate::context::DiagnosticData;
use crate::context::EditorSnapshot;
use helix_mcp::protocol::{ContentItem, Tool};

/// Return the list of all available tools.
pub fn all_tools() -> Vec<Tool> {
    vec![
        // --- Phase 1 tools ---
        Tool {
            name: "workspace_info".to_string(),
            description: Some(
                "Get information about all open documents: file paths, \
                 languages, line counts, modification status, and which \
                 document has focus."
                    .to_string(),
            ),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {}
            }),
        },
        Tool {
            name: "document_read".to_string(),
            description: Some(
                "Read the full text content of a document. If no path is \
                 specified, reads the currently focused document."
                    .to_string(),
            ),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "The file path of the document to read. \
                                        If omitted, reads the active document."
                    }
                }
            }),
        },
        Tool {
            name: "goto_position".to_string(),
            description: Some(
                "Get the viewport context around a specific line and column \
                 position in a document. Useful for navigating to a location."
                    .to_string(),
            ),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "The file path of the document."
                    },
                    "line": {
                        "type": "integer",
                        "minimum": 1,
                        "description": "1-based line number."
                    },
                    "column": {
                        "type": "integer",
                        "minimum": 1,
                        "description": "1-based column number. Defaults to 1."
                    }
                },
                "required": ["path", "line"]
            }),
        },
        // --- Phase 2 tools ---
        Tool {
            name: "document_write".to_string(),
            description: Some(
                "[Mutate] Replace the entire contents of a document with new text. \
                 Requires user confirmation before applying."
                    .to_string(),
            ),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "doc_id": {
                        "type": "string",
                        "description": "The document ID to write to."
                    },
                    "new_text": {
                        "type": "string",
                        "description": "The new text content to replace the document with."
                    }
                },
                "required": ["doc_id", "new_text"]
            }),
        },
        Tool {
            name: "selection_read".to_string(),
            description: Some(
                "[Read] Read the current selection(s) for a document. Returns anchor/cursor \
                 positions and selected text."
                    .to_string(),
            ),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "doc_id": {
                        "type": "string",
                        "description": "The document ID to read selections from."
                    }
                },
                "required": ["doc_id"]
            }),
        },
        Tool {
            name: "selection_set".to_string(),
            description: Some(
                "[Preview] Set the selection in a document to specific byte ranges. \
                 Non-destructive preview — no confirmation needed."
                    .to_string(),
            ),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "doc_id": {
                        "type": "string",
                        "description": "The document ID."
                    },
                    "selections": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "anchor": {
                                    "type": "integer",
                                    "description": "Byte offset for the anchor."
                                },
                                "cursor": {
                                    "type": "integer",
                                    "description": "Byte offset for the cursor."
                                }
                            },
                            "required": ["anchor", "cursor"]
                        },
                        "description": "Array of selection ranges."
                    }
                },
                "required": ["doc_id", "selections"]
            }),
        },
        Tool {
            name: "edit_apply".to_string(),
            description: Some(
                "[Mutate] Apply text edits to a document. Each edit specifies a byte range \
                 and replacement text. Requires user confirmation before applying."
                    .to_string(),
            ),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "doc_id": {
                        "type": "string",
                        "description": "The document ID to edit."
                    },
                    "edits": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "range": {
                                    "type": "object",
                                    "properties": {
                                        "start": {
                                            "type": "integer",
                                            "description": "Byte offset for the start of the range."
                                        },
                                        "end": {
                                            "type": "integer",
                                            "description": "Byte offset for the end of the range."
                                        }
                                    },
                                    "required": ["start", "end"]
                                },
                                "new_text": {
                                    "type": "string",
                                    "description": "The replacement text."
                                }
                            },
                            "required": ["range", "new_text"]
                        },
                        "description": "Array of edits to apply."
                    }
                },
                "required": ["doc_id", "edits"]
            }),
        },
        Tool {
            name: "search_text".to_string(),
            description: Some(
                "[Read] Search for text across document contents using regex. Returns \
                 matches with surrounding context lines."
                    .to_string(),
            ),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "The regex pattern to search for."
                    },
                    "doc_ids": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Optional list of document IDs to search. \
                                        If omitted, searches all documents."
                    },
                    "case_sensitive": {
                        "type": "boolean",
                        "description": "Whether the search is case-sensitive. Defaults to true."
                    },
                    "max_results": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": 200,
                        "description": "Maximum number of results. Defaults to 50."
                    }
                },
                "required": ["query"]
            }),
        },
        Tool {
            name: "diagnostics_read".to_string(),
            description: Some(
                "[Read] Read diagnostics (errors, warnings, hints) for a document. \
                 Optional severity filter. Includes both LSP and agent-provided diagnostics."
                    .to_string(),
            ),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "doc_id": {
                        "type": "string",
                        "description": "The document ID to read diagnostics for."
                    },
                    "severity": {
                        "type": "string",
                        "enum": ["error", "warning", "info", "hint"],
                        "description": "Filter by severity level."
                    }
                },
                "required": ["doc_id"]
            }),
        },
        Tool {
            name: "lsp_request".to_string(),
            description: Some(
                "[Read] Make an LSP request for a position in a document. \
                 Currently only hover is supported (Phase 2)."
                    .to_string(),
            ),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "doc_id": {
                        "type": "string",
                        "description": "The document ID."
                    },
                    "request_type": {
                        "type": "string",
                        "enum": ["hover", "references", "definition", "implementation", "type_definition"],
                        "description": "The type of LSP request."
                    },
                    "position": {
                        "type": "object",
                        "properties": {
                            "line": { "type": "integer", "description": "1-based line number." },
                            "character": { "type": "integer", "description": "1-based character offset." }
                        },
                        "required": ["line", "character"]
                    }
                },
                "required": ["doc_id", "request_type", "position"]
            }),
        },
        // --- Phase 4 tools ---
        Tool {
            name: "diagnostics_publish".to_string(),
            description: Some(
                "[Mutate] Publish agent-provided diagnostics for a document. \
                 These appear alongside LSP diagnostics. Requires user confirmation."
                    .to_string(),
            ),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "doc_id": {
                        "type": "string",
                        "description": "The document ID to publish diagnostics for."
                    },
                    "diagnostics": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "range": {
                                    "type": "object",
                                    "properties": {
                                        "start": {
                                            "type": "integer",
                                            "description": "Byte offset for the start of the range."
                                        },
                                        "end": {
                                            "type": "integer",
                                            "description": "Byte offset for the end of the range."
                                        }
                                    },
                                    "required": ["start", "end"]
                                },
                                "severity": {
                                    "type": "string",
                                    "enum": ["error", "warning", "info", "hint"]
                                },
                                "message": {
                                    "type": "string",
                                    "description": "The diagnostic message."
                                },
                                "code": {
                                    "type": "string",
                                    "description": "Optional error/warning code."
                                },
                                "source": {
                                    "type": "string",
                                    "description": "Optional source (e.g. agent name)."
                                }
                            },
                            "required": ["range", "severity", "message"]
                        },
                        "description": "Array of diagnostics to publish for this document."
                    }
                },
                "required": ["doc_id", "diagnostics"]
            }),
        },
        Tool {
            name: "diagnostics_clear".to_string(),
            description: Some(
                "[Mutate] Clear all agent-published diagnostics for a document. \
                 If no doc_id is specified, clears diagnostics for all documents. \
                 Requires user confirmation."
                    .to_string(),
            ),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "doc_id": {
                        "type": "string",
                        "description": "The document ID to clear diagnostics for. If omitted, clears all."
                    }
                }
            }),
        },
    ]
}

/// Generate a confirmation summary for a Mutate-tier tool.
/// This is called before the tool executes, so the session can present
/// a human-readable description to the user.
pub fn confirmation_summary(
    name: &str,
    arguments: &Option<Value>,
    snapshot: &EditorSnapshot,
) -> String {
    match name {
        "document_write" => {
            let doc_id = arguments
                .as_ref()
                .and_then(|a| a.get("doc_id"))
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            let new_text = arguments
                .as_ref()
                .and_then(|a| a.get("new_text"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let line_count = new_text.lines().count();
            let path = snapshot
                .document_by_id(doc_id)
                .and_then(|f| f.path.as_deref())
                .unwrap_or(doc_id);
            format!(
                "document_write: Replace buffer contents of {} ({} lines)",
                path, line_count
            )
        }
        "edit_apply" => {
            let doc_id = arguments
                .as_ref()
                .and_then(|a| a.get("doc_id"))
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            let edit_count = arguments
                .as_ref()
                .and_then(|a| a.get("edits"))
                .and_then(|v| v.as_array())
                .map(|a| a.len())
                .unwrap_or(0);
            let path = snapshot
                .document_by_id(doc_id)
                .and_then(|f| f.path.as_deref())
                .unwrap_or(doc_id);
            format!("edit_apply: {} edit(s) to {}", edit_count, path)
        }
        "diagnostics_publish" => {
            let doc_id = arguments
                .as_ref()
                .and_then(|a| a.get("doc_id"))
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            let diag_count = arguments
                .as_ref()
                .and_then(|a| a.get("diagnostics"))
                .and_then(|v| v.as_array())
                .map(|a| a.len())
                .unwrap_or(0);
            let path = snapshot
                .document_by_id(doc_id)
                .and_then(|f| f.path.as_deref())
                .unwrap_or(doc_id);
            format!(
                "diagnostics_publish: {} diagnostic(s) for {}",
                diag_count, path
            )
        }
        "diagnostics_clear" => {
            let doc_id = arguments
                .as_ref()
                .and_then(|a| a.get("doc_id"))
                .and_then(|v| v.as_str());
            match doc_id {
                Some(id) => format!("diagnostics_clear: clear diagnostics for doc {}", id),
                None => "diagnostics_clear: clear all diagnostics".to_string(),
            }
        }
        _ => format!("{} operation", name),
    }
}

/// Dispatch a tool call by name with the given arguments.
pub fn call_tool(
    name: &str,
    arguments: &Option<Value>,
    snapshot: &EditorSnapshot,
) -> Result<Vec<ContentItem>> {
    match name {
        "workspace_info" => workspace_info(snapshot),
        "document_read" => document_read(arguments, snapshot),
        "goto_position" => goto_position(arguments, snapshot),
        "selection_read" => selection_read(arguments, snapshot),
        "selection_set" => selection_set(arguments, snapshot),
        "search_text" => search_text(arguments, snapshot),
        "diagnostics_read" => diagnostics_read(arguments, snapshot),
        "lsp_request" => lsp_request(arguments, snapshot),
        // Mutate tools must go through apply_mutation()
        "document_write" | "edit_apply" | "diagnostics_publish" | "diagnostics_clear" => Err(anyhow!(
            "Tool '{}' requires mutable access — use apply_mutation",
            name
        )),
        _ => Err(anyhow!("Unknown tool: {}", name)),
    }
}

/// Apply a mutation tool. Takes mutable access to the snapshot. Only
/// handles Mutate-tier tools. Called after confirmation gating.
pub fn apply_mutation(
    name: &str,
    arguments: &Option<Value>,
    snapshot: &mut EditorSnapshot,
) -> Result<Vec<ContentItem>> {
    match name {
        "document_write" => document_write(arguments, snapshot),
        "edit_apply" => edit_apply(arguments, snapshot),
        "diagnostics_publish" => diagnostics_publish(arguments, snapshot),
        "diagnostics_clear" => diagnostics_clear(arguments, snapshot),
        _ => Err(anyhow!("Tool '{}' is not a mutation tool", name)),
    }
}

/// Tool: workspace_info (Read tier)
///
/// Returns a JSON object describing all open documents and editor state.
fn workspace_info(snapshot: &EditorSnapshot) -> Result<Vec<ContentItem>> {
    let files: Vec<Value> = snapshot
        .files
        .iter()
        .map(|f| {
            serde_json::json!({
                "doc_id": f.doc_id,
                "path": f.path,
                "language": f.language,
                "line_count": f.line_count,
                "modified": f.modified,
            })
        })
        .collect();

    let info = serde_json::json!({
        "active_file": snapshot.active_file,
        "mode": snapshot.mode,
        "files": files,
    });

    let text = serde_json::to_string_pretty(&info).unwrap_or_else(|_| "{}".to_string());
    Ok(vec![ContentItem::text(text)])
}

/// Tool: document_read (Read tier)
///
/// Returns the full text content of a document.
fn document_read(arguments: &Option<Value>, snapshot: &EditorSnapshot) -> Result<Vec<ContentItem>> {
    let path = arguments
        .as_ref()
        .and_then(|v| v.get("path"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let file = find_file(snapshot, path.as_deref())?;

    let result = serde_json::json!({
        "path": file.path,
        "language": file.language,
        "line_count": file.line_count,
        "text": file.text,
    });

    let text = serde_json::to_string_pretty(&result).unwrap_or_else(|_| "{}".to_string());
    Ok(vec![ContentItem::text(text)])
}

/// Tool: goto_position (Preview tier)
///
/// Returns context around a specific line and column in a document.
fn goto_position(arguments: &Option<Value>, snapshot: &EditorSnapshot) -> Result<Vec<ContentItem>> {
    let args = arguments
        .as_ref()
        .ok_or_else(|| anyhow!("Missing arguments"))?;

    let path = args
        .get("path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Missing required argument: path"))?;

    let line: usize = args
        .get("line")
        .and_then(|v| v.as_u64())
        .map(|n| n as usize)
        .ok_or_else(|| anyhow!("Missing required argument: line"))?;

    let column: usize = args
        .get("column")
        .and_then(|v| v.as_u64())
        .map(|n| n as usize)
        .unwrap_or(1);

    let file = find_file(snapshot, Some(path))?;
    let lines: Vec<&str> = file.text.lines().collect();

    let line = line.saturating_sub(1); // Convert to 0-based
    if line >= lines.len() {
        return Err(anyhow!(
            "Line {} is out of range (document has {} lines)",
            line + 1,
            lines.len()
        ));
    }

    // Return 5 lines of context around the target line
    let context_start = line.saturating_sub(5);
    let context_end = (line + 6).min(lines.len());
    let context_lines: Vec<&str> = lines[context_start..context_end].to_vec();

    let result = serde_json::json!({
        "path": file.path,
        "line": line + 1,
        "column": column,
        "total_lines": lines.len(),
        "context_start_line": context_start + 1,
        "context": context_lines.join("\n"),
    });

    let text = serde_json::to_string_pretty(&result).unwrap_or_else(|_| "{}".to_string());
    Ok(vec![ContentItem::text(text)])
}

/// Tool: document_write (Mutate tier)
///
/// Replaces the entire text content of a document. The caller (session)
/// ensures confirmation gating has already passed before invoking this.
#[doc = "Mutate tier — requires confirmation"]
fn document_write(
    arguments: &Option<Value>,
    snapshot: &mut EditorSnapshot,
) -> Result<Vec<ContentItem>> {
    let args = arguments
        .as_ref()
        .ok_or_else(|| anyhow!("Missing arguments"))?;

    let doc_id = args
        .get("doc_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Missing required argument: doc_id"))?;

    let new_text = args
        .get("new_text")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Missing required argument: new_text"))?;

    let file = snapshot
        .files
        .iter_mut()
        .find(|f| f.doc_id == doc_id)
        .ok_or_else(|| anyhow!("Document not found: {}", doc_id))?;

    file.text = new_text.to_string();
    file.line_count = new_text.lines().count();
    file.modified = true;

    let result = serde_json::json!({
        "doc_id": file.doc_id,
        "applied": true,
        "line_count": file.line_count,
    });

    let text = serde_json::to_string_pretty(&result).unwrap_or_else(|_| "{}".to_string());
    Ok(vec![ContentItem::text(text)])
}

/// Tool: selection_read (Read tier)
///
/// Reads selection information for a document from the editor snapshot.
#[doc = "Read tier"]
fn selection_read(
    arguments: &Option<Value>,
    snapshot: &EditorSnapshot,
) -> Result<Vec<ContentItem>> {
    let args = arguments
        .as_ref()
        .ok_or_else(|| anyhow!("Missing arguments"))?;

    let doc_id = args
        .get("doc_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Missing required argument: doc_id"))?;

    let file = snapshot
        .document_by_id(doc_id)
        .ok_or_else(|| anyhow!("Document not found: {}", doc_id))?;

    let selections: Vec<Value> = file
        .selections
        .iter()
        .map(|s| {
            serde_json::json!({
                "anchor_byte": s.anchor_byte,
                "cursor_byte": s.cursor_byte,
                "anchor_line": s.anchor_line,
                "cursor_line": s.cursor_line,
                "text": s.text,
            })
        })
        .collect();

    let result = serde_json::json!({
        "doc_id": file.doc_id,
        "selections": selections,
    });

    let text = serde_json::to_string_pretty(&result).unwrap_or_else(|_| "{}".to_string());
    Ok(vec![ContentItem::text(text)])
}

/// Tool: selection_set (Preview tier)
///
/// Records selection ranges for a document. Non-destructive, no confirmation needed.
/// In Phase 2, this is a stub that returns the count of ranges provided.
#[doc = "Preview tier"]
fn selection_set(arguments: &Option<Value>, snapshot: &EditorSnapshot) -> Result<Vec<ContentItem>> {
    let args = arguments
        .as_ref()
        .ok_or_else(|| anyhow!("Missing arguments"))?;

    let doc_id = args
        .get("doc_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Missing required argument: doc_id"))?;

    let selections = args
        .get("selections")
        .and_then(|v| v.as_array())
        .ok_or_else(|| anyhow!("Missing required argument: selections"))?;

    let _file = snapshot
        .document_by_id(doc_id)
        .ok_or_else(|| anyhow!("Document not found: {}", doc_id))?;

    let count = selections.len();

    let result = serde_json::json!({
        "doc_id": doc_id,
        "count": count,
    });

    let text = serde_json::to_string_pretty(&result).unwrap_or_else(|_| "{}".to_string());
    Ok(vec![ContentItem::text(text)])
}

/// Tool: edit_apply (Mutate tier)
///
/// Applies text edits to a document. Each edit specifies a byte range and
/// replacement text. The caller (session) ensures confirmation gating has
/// already passed before invoking this. Edits are applied in reverse order
/// so that byte offsets remain valid.
#[doc = "Mutate tier — requires confirmation"]
fn edit_apply(
    arguments: &Option<Value>,
    snapshot: &mut EditorSnapshot,
) -> Result<Vec<ContentItem>> {
    let args = arguments
        .as_ref()
        .ok_or_else(|| anyhow!("Missing arguments"))?;

    let doc_id = args
        .get("doc_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Missing required argument: doc_id"))?;

    let edits = args
        .get("edits")
        .and_then(|v| v.as_array())
        .ok_or_else(|| anyhow!("Missing required argument: edits"))?;

    let file = snapshot
        .files
        .iter_mut()
        .find(|f| f.doc_id == doc_id)
        .ok_or_else(|| anyhow!("Document not found: {}", doc_id))?;

    // Collect edits, sorted by start byte descending for safe in-place apply
    let mut parsed_edits: Vec<(usize, usize, String)> = Vec::new();
    for edit in edits {
        let range = edit
            .get("range")
            .ok_or_else(|| anyhow!("Edit missing range"))?;
        let start = range
            .get("start")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| anyhow!("Edit range missing start"))? as usize;
        let end = range
            .get("end")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| anyhow!("Edit range missing end"))? as usize;
        let new_text = edit
            .get("new_text")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        if start > file.text.len() || end > file.text.len() || start > end {
            return Err(anyhow!(
                "Invalid edit range [{start}, {end}) for text of length {}",
                file.text.len()
            ));
        }

        parsed_edits.push((start, end, new_text));
    }

    // Sort descending by start byte so earlier edits don't affect later offsets
    parsed_edits.sort_by(|a, b| b.0.cmp(&a.0));

    let mut text = file.text.clone();
    for (start, end, new_text) in &parsed_edits {
        text.replace_range(*start..*end, new_text);
    }

    let applied_count = parsed_edits.len();
    file.text = text;
    file.line_count = file.text.lines().count();
    file.modified = true;

    let result = serde_json::json!({
        "doc_id": doc_id,
        "applied": true,
        "applied_count": applied_count,
    });

    let text = serde_json::to_string_pretty(&result).unwrap_or_else(|_| "{}".to_string());
    Ok(vec![ContentItem::text(text)])
}

/// Tool: search_text (Read tier)
///
/// Searches document text using a regex pattern and returns matches with
/// surrounding context lines.
#[doc = "Read tier"]
fn search_text(arguments: &Option<Value>, snapshot: &EditorSnapshot) -> Result<Vec<ContentItem>> {
    let args = arguments
        .as_ref()
        .ok_or_else(|| anyhow!("Missing arguments"))?;

    let query = args
        .get("query")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Missing required argument: query"))?;

    let case_sensitive = args
        .get("case_sensitive")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);

    let max_results: usize = args
        .get("max_results")
        .and_then(|v| v.as_u64())
        .map(|n| n as usize)
        .unwrap_or(50)
        .min(200);

    let doc_ids: Option<Vec<&str>> = args
        .get("doc_ids")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect());

    let re = match if case_sensitive {
        regex::Regex::new(query)
    } else {
        regex::RegexBuilder::new(query)
            .case_insensitive(true)
            .build()
    } {
        Ok(re) => re,
        Err(e) => return Err(anyhow!("Invalid regex pattern: {}", e)),
    };

    let mut matches: Vec<Value> = Vec::new();

    for file in &snapshot.files {
        if let Some(ref ids) = doc_ids {
            if !ids.contains(&file.doc_id.as_str()) {
                continue;
            }
        }

        for (line_num, line) in file.text.lines().enumerate() {
            for m in re.find_iter(line) {
                if matches.len() >= max_results {
                    break;
                }

                let lines: Vec<&str> = file.text.lines().collect();
                let context_before = if line_num > 0 {
                    lines[(line_num.saturating_sub(1))..line_num].join("\n")
                } else {
                    String::new()
                };
                let context_after = if line_num + 1 < lines.len() {
                    lines[(line_num + 1)..(line_num + 2).min(lines.len())].join("\n")
                } else {
                    String::new()
                };

                matches.push(serde_json::json!({
                    "doc_id": file.doc_id,
                    "line_number": line_num + 1,
                    "byte_offset": m.start() + file.text.lines().take(line_num).map(|l| l.len() + 1).sum::<usize>(),
                    "match_text": m.as_str(),
                    "context_before": context_before,
                    "context_after": context_after,
                }));
            }
            if matches.len() >= max_results {
                break;
            }
        }
    }

    let result = serde_json::json!({
        "matches": matches,
        "total_matches": matches.len(),
    });

    let text = serde_json::to_string_pretty(&result).unwrap_or_else(|_| "{}".to_string());
    Ok(vec![ContentItem::text(text)])
}

/// Tool: diagnostics_read (Read tier)
///
/// Reads diagnostics for a document, with an optional severity filter.
/// Merges both LSP diagnostics and agent-provided diagnostics.
#[doc = "Read tier"]
fn diagnostics_read(
    arguments: &Option<Value>,
    snapshot: &EditorSnapshot,
) -> Result<Vec<ContentItem>> {
    let args = arguments
        .as_ref()
        .ok_or_else(|| anyhow!("Missing arguments"))?;

    let doc_id = args
        .get("doc_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Missing required argument: doc_id"))?;

    let severity_filter: Option<&str> = args.get("severity").and_then(|v| v.as_str());

    let file = snapshot
        .document_by_id(doc_id)
        .ok_or_else(|| anyhow!("Document not found: {}", doc_id))?;

    let doc_path = file.path.as_deref().unwrap_or("");

    let mut diags: Vec<Value> = snapshot
        .diagnostics
        .iter()
        .filter(|d| {
            if let Some(sev) = severity_filter {
                d.severity == sev
            } else {
                true
            }
        })
        .map(|d| {
            serde_json::json!({
                "range": { "start": d.range.0, "end": d.range.1 },
                "severity": d.severity,
                "message": d.message,
                "code": d.code,
                "source": d.source,
            })
        })
        .collect();

    // Add agent-provided diagnostics for this document
    if let Some(agent_diags) = snapshot.agent_diagnostics.get(doc_id) {
        for d in agent_diags {
            if let Some(sev) = severity_filter {
                if d.severity != sev {
                    continue;
                }
            }
            diags.push(serde_json::json!({
                "range": { "start": d.range.0, "end": d.range.1 },
                "severity": d.severity,
                "message": d.message,
                "code": d.code,
                "source": d.source,
            }));
        }
    }

    let result = serde_json::json!({
        "doc_id": doc_id,
        "path": doc_path,
        "diagnostics": diags,
    });

    let text = serde_json::to_string_pretty(&result).unwrap_or_else(|_| "{}".to_string());
    Ok(vec![ContentItem::text(text)])
}

/// Tool: lsp_request (Read tier)
///
/// Makes an LSP request for a position in a document. In Phase 2, only
/// the "hover" request type returns a stub result; other request types
/// return an error indicating they are not yet implemented.
#[doc = "Read tier"]
fn lsp_request(arguments: &Option<Value>, snapshot: &EditorSnapshot) -> Result<Vec<ContentItem>> {
    let args = arguments
        .as_ref()
        .ok_or_else(|| anyhow!("Missing arguments"))?;

    let doc_id = args
        .get("doc_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Missing required argument: doc_id"))?;

    let request_type = args
        .get("request_type")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Missing required argument: request_type"))?;

    let _file = snapshot
        .document_by_id(doc_id)
        .ok_or_else(|| anyhow!("Document not found: {}", doc_id))?;

    match request_type {
        "hover" => {
            let result = serde_json::json!({
                "result": {
                    "contents": "LSP request hover not yet available"
                }
            });
            let text = serde_json::to_string_pretty(&result).unwrap_or_else(|_| "{}".to_string());
            Ok(vec![ContentItem::text(text)])
        }
        "references" | "definition" | "implementation" | "type_definition" => Err(anyhow!(
            "LSP request '{}' not yet implemented in Phase 2",
            request_type
        )),
        _ => Err(anyhow!("Unknown LSP request type: {}", request_type)),
    }
}

/// Tool: diagnostics_publish (Mutate tier)
///
/// Publishes agent-provided diagnostics for a document. These diagnostics are
/// stored alongside LSP diagnostics and can be read via `diagnostics_read`.
#[doc = "Mutate tier — requires confirmation"]
fn diagnostics_publish(
    arguments: &Option<Value>,
    snapshot: &mut EditorSnapshot,
) -> Result<Vec<ContentItem>> {
    let args = arguments
        .as_ref()
        .ok_or_else(|| anyhow!("Missing arguments"))?;

    let doc_id = args
        .get("doc_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Missing required argument: doc_id"))?;

    let diags_arr = args
        .get("diagnostics")
        .and_then(|v| v.as_array())
        .ok_or_else(|| anyhow!("Missing required argument: diagnostics"))?;

    // Verify the document exists
    let _file = snapshot
        .document_by_id(doc_id)
        .ok_or_else(|| anyhow!("Document not found: {}", doc_id))?;

    let mut diagnostics: Vec<DiagnosticData> = Vec::new();
    for diag in diags_arr {
        let range = diag
            .get("range")
            .ok_or_else(|| anyhow!("Diagnostic missing range"))?;
        let start = range
            .get("start")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| anyhow!("Diagnostic range missing start"))? as usize;
        let end = range
            .get("end")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| anyhow!("Diagnostic range missing end"))? as usize;
        let severity = diag
            .get("severity")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("Diagnostic missing severity"))?
            .to_string();
        let message = diag
            .get("message")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("Diagnostic missing message"))?
            .to_string();
        let code = diag
            .get("code")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let source = diag
            .get("source")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        diagnostics.push(DiagnosticData {
            range: (start, end),
            severity,
            message,
            code,
            source,
        });
    }

    let published = diagnostics.len();
    snapshot
        .agent_diagnostics
        .entry(doc_id.to_string())
        .or_default()
        .extend(diagnostics);

    let result = serde_json::json!({
        "doc_id": doc_id,
        "published": published,
    });

    let text = serde_json::to_string_pretty(&result).unwrap_or_else(|_| "{}".to_string());
    Ok(vec![ContentItem::text(text)])
}

/// Tool: diagnostics_clear (Mutate tier)
///
/// Removes all agent-published diagnostics for a document. If doc_id is omitted,
/// clears diagnostics for all documents.
#[doc = "Mutate tier — requires confirmation"]
fn diagnostics_clear(
    arguments: &Option<Value>,
    snapshot: &mut EditorSnapshot,
) -> Result<Vec<ContentItem>> {
    let args = arguments.as_ref();
    let doc_id = args.and_then(|v| v.get("doc_id")).and_then(|v| v.as_str());

    let cleared = match doc_id {
        Some(id) => snapshot.agent_diagnostics.remove(id).map(|d| d.len()).unwrap_or(0),
        None => {
            let count: usize = snapshot.agent_diagnostics.values().map(|v| v.len()).sum();
            snapshot.agent_diagnostics.clear();
            count
        }
    };

    let result = serde_json::json!({
        "cleared": cleared,
    });
    let text = serde_json::to_string_pretty(&result).unwrap_or_else(|_| "{}".to_string());
    Ok(vec![ContentItem::text(text)])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::{DiagnosticData, EditorSnapshot, FileInfo, SelectionData};
    use std::collections::HashMap;

    fn make_snapshot() -> EditorSnapshot {
        EditorSnapshot {
            active_file: Some("/src/main.rs".to_string()),
            mode: "normal".to_string(),
            files: vec![
                FileInfo {
                    doc_id: "1".to_string(),
                    path: Some("/src/main.rs".to_string()),
                    language: Some("rust".to_string()),
                    line_count: 5,
                    text: "fn main() {\n    println!(\"hello\");\n}\n\n// comment\n".to_string(),
                    modified: false,
                    selections: vec![SelectionData {
                        anchor_byte: 0,
                        cursor_byte: 9,
                        anchor_line: 1,
                        cursor_line: 1,
                        text: "fn main()".to_string(),
                    }],
                },
                FileInfo {
                    doc_id: "2".to_string(),
                    path: Some("/src/lib.rs".to_string()),
                    language: Some("rust".to_string()),
                    line_count: 3,
                    text: "pub fn greet() -> &'static str {\n    \"hello\"\n}\n".to_string(),
                    modified: true,
                    selections: vec![],
                },
            ],
            diagnostics: vec![DiagnosticData {
                range: (5, 10),
                severity: "warning".to_string(),
                message: "unused variable `x`".to_string(),
                code: Some("unused_variables".to_string()),
                source: Some("rust-analyzer".to_string()),
            }],
            agent_diagnostics: HashMap::new(),
        }
    }

    fn make_empty_snapshot() -> EditorSnapshot {
        EditorSnapshot {
            active_file: Some("/missing.rs".to_string()),
            ..Default::default()
        }
    }

    #[test]
    fn test_empty_snapshot_has_no_documents() {
        let snap = make_empty_snapshot();
        assert!(snap.document_by_id("1").is_none());
        assert_eq!(snap.files.len(), 0);
        assert_eq!(snap.diagnostics.len(), 0);
    }

    // --- Phase 1 tests ---

    #[test]
    fn test_all_tools_returns_correct_count() {
        let tools = all_tools();
        assert_eq!(tools.len(), 12);
    }

    #[test]
    fn test_all_tools_have_names_and_schemas() {
        let tools = all_tools();
        for tool in &tools {
            assert!(!tool.name.is_empty());
            assert!(tool.input_schema.is_object());
        }
    }

    #[test]
    fn test_call_tool_unknown() {
        let snap = make_snapshot();
        let result = call_tool("nonexistent", &None, &snap);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Unknown tool"));
    }

    #[test]
    fn test_workspace_info() {
        let snap = make_snapshot();
        let result = call_tool("workspace_info", &None, &snap).unwrap();
        assert_eq!(result.len(), 1);

        let text = match &result[0] {
            ContentItem::Text { text, .. } => text,
            other => panic!("expected Text content, got {other:?}"),
        };
        assert!(text.contains("main.rs"));
        assert!(text.contains("lib.rs"));
        assert!(text.contains("normal"));
    }

    #[test]
    fn test_document_read_active() {
        let snap = make_snapshot();
        let result = call_tool("document_read", &None, &snap).unwrap();
        let text = match &result[0] {
            ContentItem::Text { text, .. } => text,
            other => panic!("expected Text content, got {other:?}"),
        };
        assert!(text.contains("fn main()"));
        assert!(text.contains("println!"));
    }

    #[test]
    fn test_document_read_by_path() {
        let snap = make_snapshot();
        let args = serde_json::json!({"path": "/src/lib.rs"});
        let result = call_tool("document_read", &Some(args), &snap).unwrap();
        let text = match &result[0] {
            ContentItem::Text { text, .. } => text,
            other => panic!("expected Text content, got {other:?}"),
        };
        assert!(text.contains("pub fn greet()"));
    }

    #[test]
    fn test_document_read_by_suffix() {
        let snap = make_snapshot();
        let args = serde_json::json!({"path": "lib.rs"});
        let result = call_tool("document_read", &Some(args), &snap).unwrap();
        let text = match &result[0] {
            ContentItem::Text { text, .. } => text,
            other => panic!("expected Text content, got {other:?}"),
        };
        assert!(text.contains("pub fn greet()"));
    }

    #[test]
    fn test_document_read_not_found() {
        let snap = make_snapshot();
        let args = serde_json::json!({"path": "/nonexistent.rs"});
        let result = call_tool("document_read", &Some(args), &snap);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
    }

    #[test]
    fn test_goto_position() {
        let snap = make_snapshot();
        let args = serde_json::json!({"path": "/src/main.rs", "line": 2, "column": 5});
        let result = call_tool("goto_position", &Some(args), &snap).unwrap();
        let text = match &result[0] {
            ContentItem::Text { text, .. } => text,
            other => panic!("expected Text content, got {other:?}"),
        };
        assert!(text.contains("\"line\": 2"));
    }

    #[test]
    fn test_goto_position_missing_path() {
        let snap = make_snapshot();
        let args = serde_json::json!({"line": 1});
        let result = call_tool("goto_position", &Some(args), &snap);
        assert!(result.is_err());
    }

    #[test]
    fn test_goto_position_out_of_range() {
        let snap = make_snapshot();
        let args = serde_json::json!({"path": "/src/main.rs", "line": 999});
        let result = call_tool("goto_position", &Some(args), &snap);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("out of range"));
    }

    #[test]
    fn test_goto_position_default_column() {
        let snap = make_snapshot();
        let args = serde_json::json!({"path": "/src/main.rs", "line": 1});
        let result = call_tool("goto_position", &Some(args), &snap).unwrap();
        let text = match &result[0] {
            ContentItem::Text { text, .. } => text,
            other => panic!("expected Text content, got {other:?}"),
        };
        assert!(text.contains("\"column\": 1"));
    }

    #[test]
    fn test_find_file_active_no_files() {
        let snap = EditorSnapshot {
            active_file: Some("/missing.rs".to_string()),
            ..Default::default()
        };
        let result = find_file(&snap, None);
        assert!(result.is_err());
    }

    // --- Phase 2 tests ---

    #[test]
    fn test_document_write() {
        let mut snap = make_snapshot();
        let args = serde_json::json!({"doc_id": "1", "new_text": "fn main() {\n    // new\n}\n"});
        let result = apply_mutation("document_write", &Some(args), &mut snap).unwrap();
        let text = match &result[0] {
            ContentItem::Text { text, .. } => text,
            other => panic!("expected Text content, got {:?}", other),
        };
        assert!(text.contains("\"doc_id\": \"1\""));
        assert!(text.contains("\"line_count\": 3"));
        assert!(text.contains("\"applied\": true"));
        // Verify the text was actually changed
        let file = snap.document_by_id("1").unwrap();
        assert_eq!(file.text, "fn main() {\n    // new\n}\n");
        assert_eq!(file.line_count, 3);
        assert!(file.modified);
    }

    #[test]
    fn test_document_write_missing_args() {
        let mut snap = make_snapshot();
        let result = apply_mutation("document_write", &None, &mut snap);
        assert!(result.is_err());
    }

    #[test]
    fn test_document_write_not_found() {
        let mut snap = make_snapshot();
        let args = serde_json::json!({"doc_id": "999", "new_text": "test"});
        let result = apply_mutation("document_write", &Some(args), &mut snap);
        assert!(result.is_err());
    }

    #[test]
    fn test_selection_read() {
        let snap = make_snapshot();
        let args = serde_json::json!({"doc_id": "1"});
        let result = call_tool("selection_read", &Some(args), &snap).unwrap();
        let text = match &result[0] {
            ContentItem::Text { text, .. } => text,
            other => panic!("expected Text content, got {:?}", other),
        };
        assert!(text.contains("\"text\": \"fn main()\""));
        assert!(text.contains("\"anchor_byte\""));
    }

    #[test]
    fn test_selection_read_not_found() {
        let snap = make_snapshot();
        let args = serde_json::json!({"doc_id": "999"});
        let result = call_tool("selection_read", &Some(args), &snap);
        assert!(result.is_err());
    }

    #[test]
    fn test_selection_set() {
        let snap = make_snapshot();
        let args = serde_json::json!({
            "doc_id": "1",
            "selections": [
                {"anchor": 0, "cursor": 5},
                {"anchor": 10, "cursor": 15}
            ]
        });
        let result = call_tool("selection_set", &Some(args), &snap).unwrap();
        let text = match &result[0] {
            ContentItem::Text { text, .. } => text,
            other => panic!("expected Text content, got {:?}", other),
        };
        assert!(text.contains("\"count\": 2"));
    }

    #[test]
    fn test_selection_set_missing_args() {
        let snap = make_snapshot();
        let result = call_tool("selection_set", &None, &snap);
        assert!(result.is_err());
    }

    #[test]
    fn test_edit_apply() {
        let mut snap = make_snapshot();
        let original_text = snap.document_by_id("1").unwrap().text.clone();
        let args = serde_json::json!({
            "doc_id": "1",
            "edits": [
                {"range": {"start": 0, "end": 10}, "new_text": "fn edited()"},
                {"range": {"start": 20, "end": 30}, "new_text": "bar"}
            ]
        });
        let result = apply_mutation("edit_apply", &Some(args), &mut snap).unwrap();
        let text = match &result[0] {
            ContentItem::Text { text, .. } => text,
            other => panic!("expected Text content, got {:?}", other),
        };
        assert!(text.contains("\"applied\": true"));
        assert!(text.contains("\"applied_count\": 2"));
        // Verify the text was actually changed
        let file = snap.document_by_id("1").unwrap();
        assert!(file.text != original_text);
        assert!(file.modified);
    }

    #[test]
    fn test_edit_apply_missing_args() {
        let mut snap = make_snapshot();
        let result = apply_mutation("edit_apply", &None, &mut snap);
        assert!(result.is_err());
    }

    #[test]
    fn test_search_text() {
        let snap = make_snapshot();
        let args = serde_json::json!({"query": "fn main"});
        let result = call_tool("search_text", &Some(args), &snap).unwrap();
        let text = match &result[0] {
            ContentItem::Text { text, .. } => text,
            other => panic!("expected Text content, got {:?}", other),
        };
        assert!(text.contains("fn main"));
        assert!(text.contains("\"doc_id\": \"1\""));
    }

    #[test]
    fn test_search_text_case_sensitive() {
        let snap = make_snapshot();
        let args = serde_json::json!({"query": "FN MAIN", "case_sensitive": true});
        let result = call_tool("search_text", &Some(args), &snap).unwrap();
        let text = match &result[0] {
            ContentItem::Text { text, .. } => text,
            other => panic!("expected Text content, got {:?}", other),
        };
        assert!(text.contains("\"total_matches\": 0"));
    }

    #[test]
    fn test_search_text_case_insensitive() {
        let snap = make_snapshot();
        let args = serde_json::json!({"query": "FN MAIN", "case_sensitive": false});
        let result = call_tool("search_text", &Some(args), &snap).unwrap();
        let text = match &result[0] {
            ContentItem::Text { text, .. } => text,
            other => panic!("expected Text content, got {:?}", other),
        };
        assert!(text.contains("fn main"));
    }

    #[test]
    fn test_search_text_doc_ids_filter() {
        let snap = make_snapshot();
        let args = serde_json::json!({
            "query": "fn",
            "doc_ids": ["2"]
        });
        let result = call_tool("search_text", &Some(args), &snap).unwrap();
        let text = match &result[0] {
            ContentItem::Text { text, .. } => text,
            other => panic!("expected Text content, got {:?}", other),
        };
        assert!(text.contains("\"doc_id\": \"2\""));
        assert!(!text.contains("\"doc_id\": \"1\""));
    }

    #[test]
    fn test_search_text_invalid_regex() {
        let snap = make_snapshot();
        let args = serde_json::json!({"query": "[invalid"});
        let result = call_tool("search_text", &Some(args), &snap);
        assert!(result.is_err());
    }

    #[test]
    fn test_search_text_max_results() {
        let snap = make_snapshot();
        let args = serde_json::json!({"query": "\\w", "max_results": 2});
        let result = call_tool("search_text", &Some(args), &snap).unwrap();
        let text = match &result[0] {
            ContentItem::Text { text, .. } => text,
            other => panic!("expected Text content, got {:?}", other),
        };
        assert!(text.contains("\"total_matches\": 2"));
    }

    #[test]
    fn test_diagnostics_read() {
        let snap = make_snapshot();
        let args = serde_json::json!({"doc_id": "1"});
        let result = call_tool("diagnostics_read", &Some(args), &snap).unwrap();
        let text = match &result[0] {
            ContentItem::Text { text, .. } => text,
            other => panic!("expected Text content, got {:?}", other),
        };
        assert!(text.contains("unused variable"));
    }

    #[test]
    fn test_diagnostics_read_filtered() {
        let snap = make_snapshot();
        let args = serde_json::json!({"doc_id": "1", "severity": "error"});
        let result = call_tool("diagnostics_read", &Some(args), &snap).unwrap();
        let text = match &result[0] {
            ContentItem::Text { text, .. } => text,
            other => panic!("expected Text content, got {:?}", other),
        };
        assert!(!text.contains("unused variable")); // filtered out (warning)
    }

    #[test]
    fn test_diagnostics_read_not_found() {
        let snap = make_snapshot();
        let args = serde_json::json!({"doc_id": "999"});
        let result = call_tool("diagnostics_read", &Some(args), &snap);
        assert!(result.is_err());
    }

    #[test]
    fn test_lsp_request_hover() {
        let snap = make_snapshot();
        let args = serde_json::json!({
            "doc_id": "1",
            "request_type": "hover",
            "position": {"line": 1, "character": 1}
        });
        let result = call_tool("lsp_request", &Some(args), &snap).unwrap();
        let text = match &result[0] {
            ContentItem::Text { text, .. } => text,
            other => panic!("expected Text content, got {:?}", other),
        };
        assert!(text.contains("not yet available"));
    }

    #[test]
    fn test_lsp_request_definition_not_implemented() {
        let snap = make_snapshot();
        let args = serde_json::json!({
            "doc_id": "1",
            "request_type": "definition",
            "position": {"line": 1, "character": 1}
        });
        let result = call_tool("lsp_request", &Some(args), &snap);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("not yet implemented"));
    }

    #[test]
    fn test_lsp_request_not_found() {
        let snap = make_snapshot();
        let args = serde_json::json!({
            "doc_id": "999",
            "request_type": "hover",
            "position": {"line": 1, "character": 1}
        });
        let result = call_tool("lsp_request", &Some(args), &snap);
        assert!(result.is_err());
    }

    #[test]
    fn test_confirmation_summary_document_write() {
        let snap = make_snapshot();
        let args = serde_json::json!({"doc_id": "1", "new_text": "a\nb\nc"});
        let summary = confirmation_summary("document_write", &Some(args), &snap);
        assert!(summary.contains("document_write"));
        assert!(summary.contains("3 lines"));
        assert!(summary.contains("main.rs"));
    }

    #[test]
    fn test_confirmation_summary_edit_apply() {
        let snap = make_snapshot();
        let args = serde_json::json!({
            "doc_id": "1",
            "edits": [{"range": {"start": 0, "end": 1}, "new_text": "x"}, {"range": {"start": 2, "end": 3}, "new_text": "y"}]
        });
        let summary = confirmation_summary("edit_apply", &Some(args), &snap);
        assert!(summary.contains("edit_apply"));
        assert!(summary.contains("2 edit"));
    }

    // --- Phase 4 tests ---

    #[test]
    fn test_diagnostics_publish() {
        let mut snap = make_snapshot();
        let args = serde_json::json!({
            "doc_id": "1",
            "diagnostics": [
                {
                    "range": {"start": 0, "end": 10},
                    "severity": "error",
                    "message": "AI review: potential null deref",
                    "code": "ai-null-check",
                    "source": "code-review-agent"
                }
            ]
        });
        let result = apply_mutation("diagnostics_publish", &Some(args), &mut snap).unwrap();
        let text = match &result[0] {
            ContentItem::Text { text, .. } => text,
            other => panic!("expected Text content, got {:?}", other),
        };
        assert!(text.contains("\"published\": 1"));

        // Verify agent diagnostics are stored
        let agent_diags = snap.agent_diagnostics.get("1").unwrap();
        assert_eq!(agent_diags.len(), 1);
        assert_eq!(agent_diags[0].severity, "error");
        assert_eq!(agent_diags[0].message, "AI review: potential null deref");
        assert_eq!(agent_diags[0].code.as_deref(), Some("ai-null-check"));
        assert_eq!(agent_diags[0].source.as_deref(), Some("code-review-agent"));
    }

    #[test]
    fn test_diagnostics_publish_missing_args() {
        let mut snap = make_snapshot();
        let result = apply_mutation("diagnostics_publish", &None, &mut snap);
        assert!(result.is_err());
    }

    #[test]
    fn test_diagnostics_publish_not_found() {
        let mut snap = make_snapshot();
        let args = serde_json::json!({
            "doc_id": "999",
            "diagnostics": []
        });
        let result = apply_mutation("diagnostics_publish", &Some(args), &mut snap);
        assert!(result.is_err());
    }

    #[test]
    fn test_diagnostics_read_includes_agent_diagnostics() {
        // Publish agent diagnostics, then read back and verify they're merged
        let mut snap = make_snapshot();
        let pub_args = serde_json::json!({
            "doc_id": "1",
            "diagnostics": [
                {
                    "range": {"start": 0, "end": 5},
                    "severity": "error",
                    "message": "AI warning",
                    "code": null,
                    "source": "ai"
                }
            ]
        });
        apply_mutation("diagnostics_publish", &Some(pub_args), &mut snap).unwrap();

        // Now read diagnostics for doc "1" and verify both LSP and agent diags appear
        let read_args = serde_json::json!({"doc_id": "1"});
        let result = call_tool("diagnostics_read", &Some(read_args), &snap).unwrap();
        let text = match &result[0] {
            ContentItem::Text { text, .. } => text,
            other => panic!("expected Text content, got {:?}", other),
        };
        assert!(text.contains("unused variable")); // original LSP diagnostic
        assert!(text.contains("AI warning")); // agent diagnostic
    }
}

/// Find a file in the snapshot by path. If no path is given, returns the
/// active file. If a path is given, matches against file paths.
fn find_file<'a>(
    snapshot: &'a EditorSnapshot,
    path: Option<&str>,
) -> Result<&'a crate::context::FileInfo> {
    match path {
        Some(path) => snapshot
            .files
            .iter()
            .find(|f| f.path.as_deref() == Some(path))
            .or_else(|| {
                snapshot
                    .files
                    .iter()
                    .find(|f| f.path.as_deref().is_some_and(|p| p.ends_with(path)))
            })
            .ok_or_else(|| anyhow!("Document not found: {}", path)),
        None => {
            // Return the active file
            let active = snapshot
                .active_file
                .as_deref()
                .ok_or_else(|| anyhow!("No active document"))?;

            snapshot
                .files
                .iter()
                .find(|f| f.path.as_deref() == Some(active))
                .or_else(|| {
                    // If active file is a doc_id, match by doc_id
                    snapshot.files.iter().find(|f| f.doc_id == active)
                })
                .ok_or_else(|| anyhow!("Active document not found: {}", active))
        }
    }
}

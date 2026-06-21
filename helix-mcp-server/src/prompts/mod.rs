//! MCP prompt template implementations for Helix.
//!
//! Phase 2 implements four prompts that inject document context (selection text,
//! diagnostics, file path, language) into LLM prompt templates:
//! 1. `helix/refactor` — Refactor selected code
//! 2. `helix/review` — Review code for issues
//! 3. `helix/explain` — Explain selected code
//! 4. `helix/fix-diagnostics` — Fix reported diagnostics

use crate::context::EditorSnapshot;
use helix_mcp::protocol::{ContentItem, Prompt, PromptMessage};
use serde_json::Value;

/// Return the list of all available prompts.
pub fn all_prompts() -> Vec<Prompt> {
    vec![
        Prompt {
            name: "helix/refactor".to_string(),
            description: Some(
                "Refactor the selected code. Injects selection text, \
                 language, and file path."
                    .to_string(),
            ),
            arguments: None,
        },
        Prompt {
            name: "helix/review".to_string(),
            description: Some(
                "Review code for bugs, style issues, and performance problems. \
                 Injects document text and diagnostics."
                    .to_string(),
            ),
            arguments: None,
        },
        Prompt {
            name: "helix/explain".to_string(),
            description: Some("Explain what the selected code does.".to_string()),
            arguments: None,
        },
        Prompt {
            name: "helix/fix-diagnostics".to_string(),
            description: Some(
                "Suggest fixes for diagnostics in a document. \
                 Injects diagnostics and relevant code."
                    .to_string(),
            ),
            arguments: None,
        },
    ]
}

/// Get a specific prompt by name with the current editor context.
pub fn get_prompt(
    name: &str,
    _arguments: &Option<Value>,
    snapshot: &EditorSnapshot,
) -> Result<helix_mcp::protocol::GetPromptResult, String> {
    match name {
        "helix/refactor" => refactor_prompt(snapshot),
        "helix/review" => review_prompt(snapshot),
        "helix/explain" => explain_prompt(snapshot),
        "helix/fix-diagnostics" => fix_diagnostics_prompt(snapshot),
        _ => Err(format!("Unknown prompt: {}", name)),
    }
}

/// Prompt: `helix/refactor`
///
/// Injects the active document's selection, language, and path into a
/// refactoring prompt template.
fn refactor_prompt(
    snapshot: &EditorSnapshot,
) -> Result<helix_mcp::protocol::GetPromptResult, String> {
    let file = active_file_info(snapshot)?;
    let selection_text = selection_text(file);

    let content = format!(
        "Refactor the following {} code in {}. The selected code is:\n```\n{}\n```\nExplain your changes.",
        file.language.as_deref().unwrap_or("unknown"),
        file.path.as_deref().unwrap_or("unknown file"),
        selection_text,
    );

    Ok(helix_mcp::protocol::GetPromptResult {
        description: Some("Refactor selected code with explanation".to_string()),
        messages: vec![PromptMessage {
            role: "user".to_string(),
            content: ContentItem::text(content),
        }],
    })
}

/// Prompt: `helix/review`
///
/// Injects document text and diagnostics into a code review prompt.
fn review_prompt(
    snapshot: &EditorSnapshot,
) -> Result<helix_mcp::protocol::GetPromptResult, String> {
    let file = active_file_info(snapshot)?;
    let diagnostics_text = diagnostics_for_doc(snapshot, file);

    let content = format!(
        "Review the following {} code in {} for bugs, style issues, and performance problems:\n```\n{}\n```\nDiagnostics:\n{}",
        file.language.as_deref().unwrap_or("unknown"),
        file.path.as_deref().unwrap_or("unknown file"),
        file.text,
        diagnostics_text,
    );

    Ok(helix_mcp::protocol::GetPromptResult {
        description: Some("Review code for bugs, style, and performance issues".to_string()),
        messages: vec![PromptMessage {
            role: "user".to_string(),
            content: ContentItem::text(content),
        }],
    })
}

/// Prompt: `helix/explain`
///
/// Injects selected code from the active document into an explanation prompt.
fn explain_prompt(
    snapshot: &EditorSnapshot,
) -> Result<helix_mcp::protocol::GetPromptResult, String> {
    let file = active_file_info(snapshot)?;
    let selection_text = selection_text(file);

    let content = format!(
        "Explain what the following code does:\n```\n{}\n```",
        selection_text,
    );

    Ok(helix_mcp::protocol::GetPromptResult {
        description: Some("Explain what the selected code does".to_string()),
        messages: vec![PromptMessage {
            role: "user".to_string(),
            content: ContentItem::text(content),
        }],
    })
}

/// Prompt: `helix/fix-diagnostics`
///
/// Injects diagnostics and relevant code into a diagnostic-fixing prompt.
fn fix_diagnostics_prompt(
    snapshot: &EditorSnapshot,
) -> Result<helix_mcp::protocol::GetPromptResult, String> {
    let file = active_file_info(snapshot)?;
    let diagnostics_text = diagnostics_for_doc(snapshot, file);

    let content = format!(
        "Fix the following diagnostics in {}:\n{}\n\nThe relevant code:\n```\n{}\n```",
        file.path.as_deref().unwrap_or("unknown file"),
        diagnostics_text,
        file.text,
    );

    Ok(helix_mcp::protocol::GetPromptResult {
        description: Some("Suggest fixes for diagnostics in the document".to_string()),
        messages: vec![PromptMessage {
            role: "user".to_string(),
            content: ContentItem::text(content),
        }],
    })
}

/// Helper: get the active file's FileInfo, or error if no active file.
fn active_file_info(snapshot: &EditorSnapshot) -> Result<&crate::context::FileInfo, String> {
    let active = snapshot
        .active_file
        .as_deref()
        .ok_or_else(|| "No active document".to_string())?;

    snapshot
        .files
        .iter()
        .find(|f| f.path.as_deref() == Some(active))
        .or_else(|| snapshot.files.iter().find(|f| f.doc_id == active))
        .ok_or_else(|| format!("Active document not found: {}", active))
}

/// Helper: get selection text from a FileInfo, falling back to all text.
fn selection_text(file: &crate::context::FileInfo) -> String {
    if file.selections.is_empty() {
        return file.text.clone();
    }
    let combined: Vec<&str> = file
        .selections
        .iter()
        .filter_map(|s| {
            if s.text.is_empty() {
                None
            } else {
                Some(s.text.as_str())
            }
        })
        .collect();
    if combined.is_empty() {
        file.text.clone()
    } else {
        combined.join("\n---\n")
    }
}

/// Helper: format diagnostics for a specific document.
fn diagnostics_for_doc(snapshot: &EditorSnapshot, file: &crate::context::FileInfo) -> String {
    let doc_path = file.path.as_deref().unwrap_or("");
    let mut diags: Vec<String> = Vec::new();
    for d in &snapshot.diagnostics {
        let range_str = format!("[{}, {}]", d.range.0, d.range.1);
        let code_str = d
            .code
            .as_ref()
            .map(|c| format!(" [{}]", c))
            .unwrap_or_default();
        let source_str = d
            .source
            .as_ref()
            .map(|s| format!(" ({})", s))
            .unwrap_or_default();
        diags.push(format!(
            "{}: {}:{}{}{} - {}",
            d.severity, doc_path, range_str, code_str, source_str, d.message
        ));
    }
    if diags.is_empty() {
        "No diagnostics".to_string()
    } else {
        diags.join("\n")
    }
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
            files: vec![FileInfo {
                doc_id: "1".to_string(),
                path: Some("/src/main.rs".to_string()),
                language: Some("rust".to_string()),
                line_count: 5,
                text: "fn main() {\n    println!(\"hello\");\n}\n".to_string(),
                modified: false,
                selections: vec![SelectionData {
                    anchor_byte: 0,
                    cursor_byte: 9,
                    anchor_line: 1,
                    cursor_line: 1,
                    text: "fn main()".to_string(),
                }],
            }],
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
        EditorSnapshot::default()
    }

    #[test]
    fn test_all_prompts_count() {
        let prompts = all_prompts();
        assert_eq!(prompts.len(), 4);
    }

    #[test]
    fn test_all_prompts_have_names() {
        let prompts = all_prompts();
        for p in &prompts {
            assert!(!p.name.is_empty());
            assert!(p.description.is_some());
        }
    }

    #[test]
    fn test_refactor_prompt() {
        let snap = make_snapshot();
        let result = get_prompt("helix/refactor", &None, &snap).unwrap();
        assert!(result.description.is_some());
        assert_eq!(result.messages.len(), 1);
        let text = match &result.messages[0].content {
            ContentItem::Text { text, .. } => text,
            other => panic!("expected Text, got {:?}", other),
        };
        assert!(text.contains("fn main()"));
        assert!(text.contains("rust"));
        assert!(text.contains("main.rs"));
    }

    #[test]
    fn test_review_prompt() {
        let snap = make_snapshot();
        let result = get_prompt("helix/review", &None, &snap).unwrap();
        let text = match &result.messages[0].content {
            ContentItem::Text { text, .. } => text,
            other => panic!("expected Text, got {:?}", other),
        };
        assert!(text.contains("fn main()"));
        assert!(text.contains("unused variable"));
    }

    #[test]
    fn test_explain_prompt() {
        let snap = make_snapshot();
        let result = get_prompt("helix/explain", &None, &snap).unwrap();
        let text = match &result.messages[0].content {
            ContentItem::Text { text, .. } => text,
            other => panic!("expected Text, got {:?}", other),
        };
        assert!(text.contains("fn main()"));
    }

    #[test]
    fn test_fix_diagnostics_prompt() {
        let snap = make_snapshot();
        let result = get_prompt("helix/fix-diagnostics", &None, &snap).unwrap();
        let text = match &result.messages[0].content {
            ContentItem::Text { text, .. } => text,
            other => panic!("expected Text, got {:?}", other),
        };
        assert!(text.contains("unused variable"));
        assert!(text.contains("fn main()"));
    }

    #[test]
    fn test_unknown_prompt() {
        let snap = make_snapshot();
        let result = get_prompt("nonexistent", &None, &snap);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Unknown prompt"));
    }

    #[test]
    fn test_no_active_file() {
        let snap = make_empty_snapshot();
        let result = get_prompt("helix/refactor", &None, &snap);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("No active document"));
    }

    #[test]
    fn test_selection_text_fallback() {
        // Test that when selections are empty, full text is used
        let text = selection_text(&FileInfo {
            doc_id: "1".to_string(),
            path: None,
            language: None,
            line_count: 1,
            text: "hello world".to_string(),
            modified: false,
            selections: vec![],
        });
        assert_eq!(text, "hello world");
    }

    #[test]
    fn test_diagnostics_for_no_diags() {
        let snap = EditorSnapshot::default();
        let file = FileInfo {
            doc_id: "1".to_string(),
            path: Some("/test.rs".to_string()),
            language: Some("rust".to_string()),
            line_count: 1,
            text: "".to_string(),
            modified: false,
            selections: vec![],
        };
        let diags = diagnostics_for_doc(&snap, &file);
        assert_eq!(diags, "No diagnostics");
    }
}

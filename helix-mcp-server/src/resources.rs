//! MCP resource implementations for Helix.
//!
//! Phase 1: two resources (document://{path}, workspace://open-documents).
//! Phase 2: three additional resources (document://{doc_id}/selection,
//!          diagnostics://{doc_id}, diagnostics://workspace).

use anyhow::{anyhow, Result};

use crate::context::EditorSnapshot;
use helix_mcp::protocol::{Resource, ResourceContent};

/// Return the list of all available resources based on the current snapshot.
pub fn all_resources(snapshot: &EditorSnapshot) -> Vec<Resource> {
    let mut resources = Vec::new();

    // document:// resources for each open file
    for file in &snapshot.files {
        if let Some(ref path) = file.path {
            let uri = format!("document://{}", path);
            resources.push(Resource {
                uri,
                name: format!("Document: {}", path),
                description: Some(format!(
                    "{} ({}, {} lines{})",
                    path,
                    file.language.as_deref().unwrap_or("text"),
                    file.line_count,
                    if file.modified { ", modified" } else { "" }
                )),
                mime_type: Some("text/plain".to_string()),
            });
        }
    }

    // document://{doc_id}/selection resources
    for file in &snapshot.files {
        if !file.selections.is_empty() {
            let uri = format!("document://{}/selection", file.doc_id);
            resources.push(Resource {
                uri,
                name: format!(
                    "Selection: {}",
                    file.path.as_deref().unwrap_or(&file.doc_id)
                ),
                description: Some(format!(
                    "Selection data for {} ({} ranges)",
                    file.path.as_deref().unwrap_or(&file.doc_id),
                    file.selections.len()
                )),
                mime_type: Some("application/json".to_string()),
            });
        }
    }

    // diagnostics://{doc_id} resources
    let has_lsp_diags = !snapshot.diagnostics.is_empty();
    let has_agent_diags = !snapshot.agent_diagnostics.is_empty();
    if has_lsp_diags || has_agent_diags {
        for file in &snapshot.files {
            // Show diagnostics resource if this doc has either LSP or agent diagnostics
            let has_doc_agent = snapshot.agent_diagnostics.contains_key(&file.doc_id);
            if has_lsp_diags || has_doc_agent {
                let uri = format!("diagnostics://{}", file.doc_id);
                resources.push(Resource {
                    uri,
                    name: format!(
                        "Diagnostics: {}",
                        file.path.as_deref().unwrap_or(&file.doc_id)
                    ),
                    description: Some(format!(
                        "Diagnostics for {}",
                        file.path.as_deref().unwrap_or(&file.doc_id)
                    )),
                    mime_type: Some("application/json".to_string()),
                });
            }
        }

        // diagnostics://workspace
        resources.push(Resource {
            uri: "diagnostics://workspace".to_string(),
            name: "Workspace Diagnostics".to_string(),
            description: Some("Aggregated diagnostics across all open documents".to_string()),
            mime_type: Some("application/json".to_string()),
        });
    }

    // workspace://open-documents manifest
    resources.push(Resource {
        uri: "workspace://open-documents".to_string(),
        name: "Open Documents".to_string(),
        description: Some("Manifest of all currently open documents".to_string()),
        mime_type: Some("application/json".to_string()),
    });

    resources
}

/// Read the content of a resource by URI.
pub fn read_resource(uri: &str, snapshot: &EditorSnapshot) -> Result<Vec<ResourceContent>> {
    if uri == "workspace://open-documents" {
        return workspace_open_documents(snapshot);
    }

    if uri == "diagnostics://workspace" {
        return diagnostics_workspace(snapshot);
    }

    if let Some(path) = uri.strip_prefix("document://") {
        // Check if it's a selection resource: document://{doc_id}/selection
        if let Some(doc_id) = path.strip_suffix("/selection") {
            return document_selection_read(doc_id, snapshot);
        }
        return document_read(path, snapshot);
    }

    if let Some(doc_id) = uri.strip_prefix("diagnostics://") {
        return diagnostics_read(doc_id, snapshot);
    }

    Err(anyhow!("Unknown resource URI: {}", uri))
}

/// Resource handler for `document://{path}`.
fn document_read(path: &str, snapshot: &EditorSnapshot) -> Result<Vec<ResourceContent>> {
    let file = snapshot
        .files
        .iter()
        .find(|f| f.path.as_deref() == Some(path))
        .or_else(|| {
            snapshot
                .files
                .iter()
                .find(|f| f.path.as_deref().is_some_and(|p| p.ends_with(path)))
        })
        .ok_or_else(|| anyhow!("Document not found: {}", path))?;

    Ok(vec![ResourceContent {
        uri: format!("document://{}", file.path.as_deref().unwrap_or(path)),
        mime_type: Some("text/plain".to_string()),
        text: Some(file.text.clone()),
        blob: None,
    }])
}

/// Resource handler for `document://{doc_id}/selection`.
fn document_selection_read(
    doc_id: &str,
    snapshot: &EditorSnapshot,
) -> Result<Vec<ResourceContent>> {
    let file = snapshot
        .document_by_id(doc_id)
        .ok_or_else(|| anyhow!("Document not found: {}", doc_id))?;

    let selections: Vec<serde_json::Value> = file
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

    Ok(vec![ResourceContent {
        uri: format!("document://{}/selection", doc_id),
        mime_type: Some("application/json".to_string()),
        text: Some(text),
        blob: None,
    }])
}

/// Resource handler for `diagnostics://{doc_id}`.
fn diagnostics_read(doc_id: &str, snapshot: &EditorSnapshot) -> Result<Vec<ResourceContent>> {
    let _file = snapshot
        .document_by_id(doc_id)
        .ok_or_else(|| anyhow!("Document not found: {}", doc_id))?;

    let mut diags: Vec<serde_json::Value> = snapshot
        .diagnostics
        .iter()
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
        "diagnostics": diags,
    });

    let text = serde_json::to_string_pretty(&result).unwrap_or_else(|_| "{}".to_string());

    Ok(vec![ResourceContent {
        uri: format!("diagnostics://{}", doc_id),
        mime_type: Some("application/json".to_string()),
        text: Some(text),
        blob: None,
    }])
}

/// Resource handler for `diagnostics://workspace`.
fn diagnostics_workspace(snapshot: &EditorSnapshot) -> Result<Vec<ResourceContent>> {
    let mut diags: Vec<serde_json::Value> = snapshot
        .diagnostics
        .iter()
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

    // Add all agent-provided diagnostics
    for agent_diags in snapshot.agent_diagnostics.values() {
        for d in agent_diags {
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
        "diagnostics": diags,
        "total": diags.len(),
    });

    let text = serde_json::to_string_pretty(&result).unwrap_or_else(|_| "{}".to_string());

    Ok(vec![ResourceContent {
        uri: "diagnostics://workspace".to_string(),
        mime_type: Some("application/json".to_string()),
        text: Some(text),
        blob: None,
    }])
}

/// Resource handler for `workspace://open-documents`.
fn workspace_open_documents(snapshot: &EditorSnapshot) -> Result<Vec<ResourceContent>> {
    let files: Vec<serde_json::Value> = snapshot
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

    let manifest = serde_json::json!({
        "active_file": snapshot.active_file,
        "mode": snapshot.mode,
        "documents": files,
    });

    let text = serde_json::to_string_pretty(&manifest).unwrap_or_else(|_| "{}".to_string());

    Ok(vec![ResourceContent {
        uri: "workspace://open-documents".to_string(),
        mime_type: Some("application/json".to_string()),
        text: Some(text),
        blob: None,
    }])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::{DiagnosticData, EditorSnapshot, FileInfo, SelectionData};
    use std::collections::HashMap;

    fn make_snapshot() -> EditorSnapshot {
        EditorSnapshot {
            active_file: Some("/src/main.rs".to_string()),
            mode: "insert".to_string(),
            files: vec![
                FileInfo {
                    doc_id: "1".to_string(),
                    path: Some("/src/main.rs".to_string()),
                    language: Some("rust".to_string()),
                    line_count: 10,
                    text: "fn main() {\n    println!(\"hello\");\n}\n".to_string(),
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
                    path: Some("/Cargo.toml".to_string()),
                    language: Some("toml".to_string()),
                    line_count: 5,
                    text: "[package]\nname = \"test\"\n".to_string(),
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

    #[test]
    fn test_all_resources_includes_document_uris() {
        let snap = make_snapshot();
        let resources = all_resources(&snap);
        let uris: Vec<&str> = resources.iter().map(|r| r.uri.as_str()).collect();
        assert!(uris.contains(&"document:///src/main.rs"));
        assert!(uris.contains(&"document:///Cargo.toml"));
        assert!(uris.contains(&"workspace://open-documents"));
    }

    #[test]
    fn test_all_resources_includes_selection_resource() {
        let snap = make_snapshot();
        let resources = all_resources(&snap);
        let uris: Vec<&str> = resources.iter().map(|r| r.uri.as_str()).collect();
        assert!(uris.contains(&"document://1/selection"));
    }

    #[test]
    fn test_all_resources_includes_diagnostics_resources() {
        let snap = make_snapshot();
        let resources = all_resources(&snap);
        let uris: Vec<&str> = resources.iter().map(|r| r.uri.as_str()).collect();
        assert!(uris.contains(&"diagnostics://1"));
        assert!(uris.contains(&"diagnostics://2"));
        assert!(uris.contains(&"diagnostics://workspace"));
    }

    #[test]
    fn test_all_resources_no_selection_when_empty() {
        let snap = make_snapshot();
        // doc_id "2" has no selections, should not have a selection resource
        let resources = all_resources(&snap);
        let uris: Vec<&str> = resources.iter().map(|r| r.uri.as_str()).collect();
        assert!(!uris.contains(&"document://2/selection"));
        assert!(uris.contains(&"document://1/selection"));
    }

    #[test]
    fn test_resource_metadata() {
        let snap = make_snapshot();
        let resources = all_resources(&snap);
        let doc = resources
            .iter()
            .find(|r| r.uri == "document:///src/main.rs")
            .unwrap();
        assert_eq!(doc.mime_type.as_deref(), Some("text/plain"));
        assert!(doc.name.contains("main.rs"));
        assert!(doc.description.as_ref().unwrap().contains("10 lines"));

        let modified = resources
            .iter()
            .find(|r| r.uri == "document:///Cargo.toml")
            .unwrap();
        assert!(modified.description.as_ref().unwrap().contains("modified"));
    }

    #[test]
    fn test_read_document_resource() {
        let snap = make_snapshot();
        let contents = read_resource("document:///src/main.rs", &snap).unwrap();
        assert_eq!(contents.len(), 1);
        assert_eq!(contents[0].uri, "document:///src/main.rs");
        assert!(contents[0].text.as_ref().unwrap().contains("fn main()"));
    }

    #[test]
    fn test_read_document_resource_not_found() {
        let snap = make_snapshot();
        let result = read_resource("document:///nonexistent.rs", &snap);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
    }

    #[test]
    fn test_read_selection_resource() {
        let snap = make_snapshot();
        let contents = read_resource("document://1/selection", &snap).unwrap();
        assert_eq!(contents.len(), 1);
        let text = contents[0].text.as_ref().unwrap();
        assert!(text.contains("fn main()"));
        assert!(text.contains("anchor_byte"));
    }

    #[test]
    fn test_read_selection_resource_not_found() {
        let snap = make_snapshot();
        let result = read_resource("document://999/selection", &snap);
        assert!(result.is_err());
    }

    #[test]
    fn test_read_diagnostics_resource() {
        let snap = make_snapshot();
        let contents = read_resource("diagnostics://1", &snap).unwrap();
        assert_eq!(contents.len(), 1);
        let text = contents[0].text.as_ref().unwrap();
        assert!(text.contains("unused variable"));
    }

    #[test]
    fn test_read_diagnostics_resource_not_found() {
        let snap = make_snapshot();
        let result = read_resource("diagnostics://999", &snap);
        assert!(result.is_err());
    }

    #[test]
    fn test_read_diagnostics_workspace() {
        let snap = make_snapshot();
        let contents = read_resource("diagnostics://workspace", &snap).unwrap();
        assert_eq!(contents.len(), 1);
        let text = contents[0].text.as_ref().unwrap();
        assert!(text.contains("unused variable"));
        assert!(text.contains("\"total\": 1"));
    }

    #[test]
    fn test_read_workspace_resource() {
        let snap = make_snapshot();
        let contents = read_resource("workspace://open-documents", &snap).unwrap();
        assert_eq!(contents.len(), 1);
        let text = contents[0].text.as_ref().unwrap();
        assert!(text.contains("main.rs"));
        assert!(text.contains("Cargo.toml"));
        assert!(text.contains("insert"));
    }

    #[test]
    fn test_read_unknown_resource() {
        let snap = make_snapshot();
        let result = read_resource("unknown://foo", &snap);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Unknown resource"));
    }

    #[test]
    fn test_no_resources_for_empty_snapshot() {
        let snap = EditorSnapshot::default();
        let resources = all_resources(&snap);
        // Only the workspace manifest, no document or selection resources
        assert_eq!(resources.len(), 1);
        assert_eq!(resources[0].uri, "workspace://open-documents");
    }
}

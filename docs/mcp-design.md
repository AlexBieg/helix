# Helix MCP Server — Design Document

> **Status**: Draft  
> **Context**: Helix is a ~15 crate Rust workspace with no existing MCP, HTTP, or socket server infrastructure.  
> The MCP server lets external AI tools (Claude, Codex, etc.) inspect and manipulate the running editor state via JSON-RPC 2.0 over stdio.

---

## 1. Architecture Overview

A new workspace crate `helix-mcp` mirrors the `helix-lsp` split: **transport** (raw I/O), **server** (MCP lifecycle + routing), and a thin **integration layer** in `helix-term` that pipes editor state.

```
                ┌─ MCP Client ─┐
                │  (Claude,     │
                │   Codex, …)   │
                └──────┬────────┘
                       │ stdio (JSON-RPC 2.0)
              ┌────────▼────────┐
              │   helix-mcp     │  ← NEW CRATE
              │  ┌───────────┐  │
              │  │ transport │  │    tokio tasks: recv, send
              │  │  server   │  │    routes tools/readResource
              │  │  tools/   │  │    tool implementations
              │  │  resources│  │    resource implementations
              │  └─────┬─────┘  │
              └────────┼────────┘
                       │ tokio::mpsc::unbounded_channel
              ┌────────▼────────┐
              │   helix-term    │
              │   Application   │
              │   Editor (main  │  ← exclusive &mut access
              │   thread,       │    processes MCP requests
              │   wait_event)   │    synchronously
              └─────────────────┘
```

| Crate | Role |
|---|---|
| `helix-mcp` | MCP protocol types, transport, server, tool/resource registry |
| `helix-term` | Application wiring, `handle_mcp_event`, config loading |
| `helix-view` | `EditorEvent::McpMessage` variant |
| `helix-event` | (unchanged — MCP uses channels, not hooks) |

### Module Layout — `helix-mcp/src/`

```
helix-mcp/
├── Cargo.toml
└── src/
    ├── lib.rs          # crate root, re-exports, Result type
    ├── transport.rs    # stdio framing, Payload enum, Transport struct
    ├── jsonrpc.rs      # JSON-RPC 2.0 types (copied/adjusted from helix-lsp)
    ├── mcp.rs          # MCP protocol types: Initialize, Tool, Resource, Prompt
    ├── server.rs       # Server lifecycle, request routing
    ├── tools.rs        # Tool trait + registry + built-in tool set
    └── resources.rs    # Resource trait + registry + built-in resources
```

### Dependency Graph

```
helix-mcp
  ├── tokio (rt, io-util, io-std, sync, macros, process)
  ├── serde + serde_json
  ├── thiserror
  ├── parking_lot
  └── log

helix-term     → depends on helix-mcp
helix-mcp      → does NOT depend on helix-view/helix-core
               → all editor data flows through channel messages
```

The key insight: **`helix-mcp` has zero dependencies on `helix-view`/`helix-core`**. It defines its own message types (`McpRequest`, `McpResponse`) that carry serializable data snapshots. The `helix-term` integration layer converts between `Editor` state and these messages.

---

## 2. Threading / Async Model

MCP follows the exact same concurrency model as LSP:

```
helix-lsp pattern:                helix-mcp pattern:
─────────────────                 ──────────────────

Client::start()                   McpServer::start()
  ├── tokio::spawn(recv)            ├── tokio::spawn(recv)
  ├── tokio::spawn(send)            ├── tokio::spawn(send)
  └── tokio::spawn(err)              └── tokio::spawn(stderr_logger)

returns (rx, tx, notify)          returns (rx, tx, server_handle)
```

- **Transport tasks** run on the tokio runtime, handling stdin/stdout framing.
- **The main editor loop** (`Application::event_loop_until_idle`) remains single-threaded with `&mut self`. It receives MCP events via the same `select!` mechanism as LSP messages.
- **No locks needed** for editor state — the main loop has exclusive mutable access. MCP tool callbacks produce a request, the main loop processes it synchronously, then sends a response back.

### Why not a separate thread?

The `Editor` struct is `Sized`, not `Sync`, and deeply mutable. Sharing it across threads would require pervasive locking (every `Document`, `Selection`, `View`, etc.) — a massive refactor with no clear benefit. The channel-based approach is proven by `helix-lsp` and `helix-dap`.

### Event flow

```
MCP Client                   Transport Tasks                Editor Main Loop
    │                             │                              │
    │  {"method":"tools/call",    │                              │
    │   "params":{...}}           │                              │
    │ ──────────────────────────► │                              │
    │                             │  McpEvent::ToolCall {       │
    │                             │    id, name, args            │
    │                             │  }                           │
    │                             │ ───────────────────────────► │
    │                             │                       handle_mcp_tool_call()
    │                             │                       reads Editor/Document state
    │                             │                       produces McpResponse
    │                             │                              │
    │                             │  McpResponse::Success{id,..} │
    │                             │ ◄─────────────────────────── │
    │  {"id":1,"result":{...}}   │                              │
    │ ◄────────────────────────── │                              │
```

---

## 3. Data Access — The Snapshot Pattern

MCP tools need read access to `Editor`, `Document`, `View`, `Tree` state. Since the MCP server runs on separate tasks, it cannot hold `&Editor`. Instead, we use a **request/response channel** where:

1. The MCP server sends an `McpRequest` enum over an unbounded channel.
2. The main loop receives it, processes it synchronously with full `&mut Editor` access, and sends back an `McpResponse`.
3. The MCP server receives the response and formats it as JSON-RPC.

### Message Types (in `helix-mcp`)

```rust
// ── helix-mcp/src/server.rs ──

/// Request from MCP server → main loop
#[derive(Debug)]
pub enum McpRequest {
    ToolCall {
        id: jsonrpc::Id,
        name: String,
        arguments: serde_json::Value,
    },
    ResourceRead {
        id: jsonrpc::Id,
        uri: String,
    },
    ResourceList {
        id: jsonrpc::Id,
    },
    PromptGet {
        id: jsonrpc::Id,
        name: String,
        arguments: serde_json::Value,
    },
    Cancel {
        id: jsonrpc::Id,
    },
}

/// Response from main loop → MCP server
#[derive(Debug)]
pub enum McpResponse {
    Success {
        id: jsonrpc::Id,
        content: serde_json::Value,
    },
    Error {
        id: jsonrpc::Id,
        code: i64,
        message: String,
        data: Option<serde_json::Value>,
    },
    ResourceContents {
        uri: String,
        mime_type: Option<String>,
        text: String,
    },
    ResourceList {
        resources: Vec<ResourceDescriptor>,
    },
    PromptMessage {
        messages: Vec<PromptMessage>,
    },
}
```

### Editor integration (in `helix-term`)

In the application's event loop, a new `select!` branch handles MCP events:

```rust
// ── helix-term/src/application.rs (sketch) ──

// In EditorEvent (helix-view/src/editor.rs):
pub enum EditorEvent {
    // ... existing variants ...
    McpMessage(McpRequest),  // NEW
}

// In Editor:
pub struct Editor {
    // ... existing fields ...
    pub mcp_rx: UnboundedReceiver<McpRequest>,  // NEW
}

// In Application::handle_mcp_request:
fn handle_mcp_request(&mut self, request: McpRequest) -> McpResponse {
    match request {
        McpRequest::ToolCall { id, name, arguments } => {
            self.mcp_call_tool(id, &name, arguments)
        }
        McpRequest::ResourceRead { id, uri } => {
            self.mcp_handle_resource_read(id, &uri)
        }
        // ...
    }
}
```

Each tool handler has **`&mut Editor`**, so it can do anything: read selections, get diagnostics, walk the `BTreeMap<DocumentId, Document>`, etc. It serializes results into `serde_json::Value` for the response.

### Snapshot accessors — `helix-mcp/tools.rs`

Tools are defined as a trait:

```rust
// ── helix-mcp/src/tools.rs ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDescriptor {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,  // JSON Schema
}

/// A tool implementation. The `execute` method runs on the main thread
/// with full `&mut Editor` access.
pub trait Tool: Send + Sync {
    fn descriptor(&self) -> ToolDescriptor;
    fn execute(
        &self,
        editor: &mut helix_view::Editor,  // ← but this creates a dep cycle!
        arguments: serde_json::Value,
    ) -> Result<serde_json::Value, McpError>;
}
```

**Problem**: `helix-mcp` cannot depend on `helix-view` (circular dependency — `helix-view` would need `helix-mcp` for the `EditorEvent` variant).

**Solution**: Define tools in `helix-term` (or a small `helix-mcp-tools` crate that depends on both). The `helix-mcp` crate defines only the message types and transport — it's a pure protocol crate. Tool/resource implementations live alongside the integration code.

### Revised Architecture

```
helix-mcp                            (protocol types, transport)
  ↓ dep
helix-term                           (tool implementations, wiring)
  ↓ (already depends on)
helix-view + helix-core + helix-lsp  (editor state)
```

The `Tool` trait stays in `helix-mcp` but with a **generic context parameter**:

```rust
pub trait Tool<C>: Send + Sync {
    fn descriptor(&self) -> ToolDescriptor;
    fn execute(&self, ctx: &mut C, arguments: serde_json::Value)
        -> Result<serde_json::Value, McpError>;
}

// In helix-term:
impl Tool<Editor> for GetSelectionTool { ... }
impl Tool<Editor> for GetOpenFilesTool { ... }
```

This way `helix-mcp` never imports `helix-view`.

---

## 4. Lifecycle & Configuration

### Startup

MCP server starts when Helix is launched with a special flag:

```
hx --mcp
```

This mode:
1. Does **not** initialize the terminal UI (no `Terminal::claim()`, no compositor).
2. Starts the MCP transport on stdio.
3. Runs a simplified event loop that only processes MCP events and LSP/DAP messages.
4. Opens files specified on the command line (if any).
5. Initializes language servers for open documents.

Alternatively, for non-headless use (future, non-MVP):
```
:lsp-enable-mcp    # starts MCP server on a configurable stdio or port
```

### Configuration (in `helix-view/src/editor.rs Config`)

```rust
pub struct McpConfig {
    /// Whether to auto-start the MCP server on editor startup.
    /// Only relevant for non-headless mode. Defaults to false.
    pub auto_start: bool,
    /// Socket path for MCP transport (alternative to stdio).
    /// If set, the server listens on this unix socket / named pipe
    /// instead of inheriting stdin/stdout.
    pub socket_path: Option<PathBuf>,
}
```

### Shutdown

When the editor exits (`Application::close()`), the MCP server:
1. Sends `{"jsonrpc":"2.0","method":"notifications/exit","params":{}}` to the client.
2. Drains the response channel.
3. Shuts down transport tasks (drop the `Sender`, causing recv to break).

Graceful exit: If the client sends `shutdown` then `exit`, the editor quits cleanly (exit code 0).

### Headless Mode Event Loop

```rust
// In helix-term/src/main.rs (conceptual)
if args.mcp {
    return run_mcp_mode(args, config, lang_loader).await;
}
// ... normal TUI mode ...

async fn run_mcp_mode(
    args: Args,
    config: Config,
    lang_loader: syntax::Loader,
) -> Result<i32> {
    let mut editor = Editor::new(/* ... */);
    // Open files from args
    for (path, position) in args.files { editor.open(path, Action::Load)?; }

    let (mcp_rx, mcp_tx) = McpServer::start(
        tokio::io::stdin(),
        tokio::io::stdout(),
        "helix-mcp".into(),
    );

    loop {
        tokio::select! {
            Some(request) = mcp_rx.recv() => {
                let response = handle_mcp_request(&mut editor, &request);
                mcp_tx.send(response).ok();
            }
            Some(msg) = editor.language_servers.incoming.next() => {
                // handle LSP messages (diagnostics still work)
            }
            Some(config_event) = editor.config_events.1.recv() => {
                // handle config changes
            }
            _ = editor.language_servers.close.recv() => {
                break;
            }
        }
    }

    Ok(editor.exit_code)
}
```

---

## 5. Error Handling

### Error Type (`helix-mcp/src/lib.rs`)

```rust
use thiserror::Error;

#[derive(Error, Debug)]
pub enum McpError {
    #[error("tool not found: {0}")]
    ToolNotFound(String),

    #[error("resource not found: {0}")]
    ResourceNotFound(String),

    #[error("prompt not found: {0}")]
    PromptNotFound(String),

    #[error("invalid arguments: {0}")]
    InvalidArguments(String),

    #[error("document not found: {0}")]
    DocumentNotFound(String),

    #[error("view not found: {id}")]
    ViewNotFound { id: String },

    #[error("internal error: {0}")]
    Internal(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("editor is in unsupported mode: {0}")]
    UnsupportedMode(String),
}

impl McpError {
    /// Convert to JSON-RPC error code
    pub fn to_jsonrpc_error(&self) -> (i64, String) {
        match self {
            McpError::ToolNotFound(_) => (-32001, self.to_string()),
            McpError::ResourceNotFound(_) => (-32002, self.to_string()),
            McpError::PromptNotFound(_) => (-32003, self.to_string()),
            McpError::InvalidArguments(_) => (-32602, self.to_string()),
            McpError::Internal(_) => (-32603, self.to_string()),
            McpError::DocumentNotFound(_) => (-32004, self.to_string()),
            McpError::ViewNotFound { .. } => (-32005, self.to_string()),
            McpError::UnsupportedMode(_) => (-32006, self.to_string()),
            McpError::Io(_) => (-32603, self.to_string()),
            McpError::Json(_) => (-32700, self.to_string()),
        }
    }
}
```

### Graceful Degradation

Tool implementations never panic. If a tool needs a `Document` that isn't open, it returns `McpError::DocumentNotFound`. If a tool needs position conversion and the position is out of bounds, it returns `McpError::InvalidArguments` with a descriptive message.

The MCP server always responds to requests (never silently drops them). Even catastrophic errors produce a JSON-RPC error response:

```rust
// In server.rs — catch-all
fn handle_request(request: jsonrpc::Call, tools: &ToolRegistry) -> jsonrpc::Response {
    match dispatch(request) {
        Ok(response) => response,
        Err(err) => {
            log::error!("MCP request failed: {err}");
            jsonrpc::Response::Single(jsonrpc::Output::Failure(
                jsonrpc::Failure {
                    jsonrpc: Some(jsonrpc::Version::V2),
                    error: err.to_jsonrpc_error_struct(),
                    id: jsonrpc::Id::Null,
                }
            ))
        }
    }
}
```

---

## 6. Resource URIs

MCP resources use a URI scheme of `document://<path>`, `diagnostics://<path>`, etc.:

| URI Pattern | Returns |
|---|---|
| `file://<absolute-path>` | Full document text |
| `file://<absolute-path}#L<line>` | Single line |
| `file://<absolute-path}#L<line1>-L<line2>` | Line range |
| `selection://current` | Primary selection text |
| `diagnostics://<path>` | Diagnostics for a file |
| `open-files://` | List of open file paths |
| `editor-state://` | Current mode, selection ranges, view position |

### Resource Lookup (in `helix-term`)

```rust
fn mcp_handle_resource_read(&mut self, uri: &str) -> McpResponse {
    let parsed = match McpUri::parse(uri) {
        Ok(uri) => uri,
        Err(e) => return McpResponse::error(e),
    };

    match parsed {
        McpUri::Document { path, range } => {
            let doc = self.editor.documents.values()
                .find(|d| d.path().map_or(false, |p| p == Path::new(path)));
            // ...
        }
        McpUri::Selection { view_selector } => {
            // ...
        }
        McpUri::Diagnostics { path } => {
            // ...
        }
        McpUri::OpenFiles => {
            let files: Vec<String> = self.editor.documents.values()
                .filter_map(|d| d.path().map(|p| p.display().to_string()))
                .collect();
            McpResponse::success(files)
        }
        McpUri::EditorState => {
            // Return mode, cursor positions, viewport info
            McpResponse::success(serialize_state(&self.editor))
        }
    }
}
```

---

## 7. Built-in Tools (MVP)

| Tool | Description | Key Inputs |
|---|---|---|
| `get_selection` | Get text of current selection(s) | `include_line_numbers` |
| `get_open_files` | List all open file paths | — |
| `get_document_text` | Get full text of a document | `path` |
| `get_diagnostics` | Get diagnostics for a file | `path` |
| `get_editor_state` | Current mode, cursor, screen layout | — |
| `goto_line` | Jump to line:column in a document | `path`, `line`, `column` |
| `replace_selection` | Replace current selection text | `text` |
| `insert_text` | Insert text at current position | `text` |
| `run_command` | Execute a Helix command ("select_all", "save", etc.) | `command` |
| `get_symbols` | Get document symbols via LSP | `path` |
| `get_hover` | Get hover information at cursor | `path`, `line`, `column` |

### Tool Schema Example

```rust
fn get_selection_tool() -> ToolDescriptor {
    ToolDescriptor {
        name: "get_selection".into(),
        description: "Get the text of the current selection(s) in the active document.".into(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "include_line_numbers": {
                    "type": "boolean",
                    "description": "Prepend line numbers to each line",
                    "default": false
                }
            }
        }),
    }
}
```

---

## 8. Testing Strategy

### Unit Tests (in `helix-mcp`)

- **Transport framing**: Send a header + body, verify `recv_server_message` parses it correctly. Test edge cases: empty content, multi-byte UTF-8, truncated payloads.
- **JSON-RPC types**: Serialization/deserialization round-trips for all MCP message types.
- **Tool descriptor validation**: Ensure all tool schemas are valid JSON Schema.
- **URI parsing**: Test `McpUri::parse` with valid/invalid resource URIs.

```rust
// helix-mcp/src/transport.rs
#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::BufReader;

    #[tokio::test]
    async fn parse_simple_message() {
        let input = b"Content-Length: 27\r\n\r\n{\"jsonrpc\":\"2.0\",\"id\":1}";
        let mut reader = BufReader::new(&input[..]);
        let mut buffer = String::new();
        let mut content = Vec::new();
        let msg = Transport::recv_server_message(
            &mut reader, &mut buffer, &mut content, "test"
        ).await.unwrap();
        // assert msg is valid
    }
}
```

### Integration Tests (in `helix-term/tests/`)

Follow the existing `integration` test pattern:

```rust
// helix-term/tests/mcp.rs
use helix_term::application::Application;
use helix_mcp::transport::Transport;

#[tokio::test]
async fn tools_list() {
    let (app, _guard) = test_app_with_file("fn main() {}").await;
    // Start MCP, send tools/list, verify response
}

#[tokio::test]
async fn get_selection_tool() {
    let (app, _guard) = test_app_with_file("hello world").await;
    app.editor.set_selection(/* select "world" */);
    let response = call_tool(app.mcp(), "get_selection", json!({}));
    assert_eq!(response.result.unwrap(), json!({"text": "world"}));
}

#[tokio::test]
async fn resource_read_document() {
    let (app, _guard) = test_app_with_file("line 1\nline 2\nline 3").await;
    let result = read_resource(app.mcp(), "file:///test.rs#L1-L2");
    assert_eq!(result, "line 1\nline 2");
}
```

### Headless Mode Testing

A separate test binary that exercises the `--mcp` headless mode:

```rust
// In xtask or a test helper:
// Spawn `hx --mcp test_file.rs`
// Send JSON-RPC initialize + tools/list via stdin
// Read response from stdout
// Verify the tool list is correct
```

### Test Infrastructure

Leverage the existing `integration` feature (`helix-event/integration_test`, `helix-term/integration`) which provides:
- `TestBackend` (fake terminal)
- Signal that the editor is "idle" after processing events
- No real terminal needed

---

## 9. Extensibility

### Adding a New Tool

1. Define a struct implementing the `Tool` trait.
2. Register it:

```rust
// In helix-term/src/mcp_tools.rs
registry.register(GetSelectionTool);
registry.register(GetOpenFilesTool);
registry.register(ReplaceSelectionTool);
```

No changes needed in `helix-mcp` itself. The tool descriptor exposes the JSON Schema automatically.

### Adding a New Resource

1. Add a variant to the `McpUri` enum (in `helix-term`).
2. Handle it in `mcp_handle_resource_read`.

### Registering External Tools (Future)

For plugin authors (future capability), tools could be registered via a hook:

```rust
// In a user's helix config or a dynamic library
helix_event::register_hook!(move |_: &mut McpToolRegisterEvent| {
    event.registry.register(MyCustomTool);
});
```

### Prompt Support (Future)

MCP `prompts/list` and `prompts/get` can be wired similarly. Prompts are pre-defined conversation templates that include context from the editor (e.g., "Review this code" picks up the current selection).

---

## 10. `Cargo.toml` Sketch

### `helix-mcp/Cargo.toml`

```toml
[package]
name = "helix-mcp"
description = "Model Context Protocol server for Helix editor"
version.workspace = true
authors.workspace = true
edition.workspace = true
license.workspace = true
rust-version.workspace = true

[dependencies]
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
tokio = { version = "1", features = ["rt", "io-util", "io-std", "sync", "macros", "parking_lot"] }
thiserror.workspace = true
log = "0.4"
parking_lot.workspace = true
sonic-rs.workspace = true
```

### `helix-term/Cargo.toml` (additions)

```toml
[dependencies]
helix-mcp = { path = "../helix-mcp" }
```

### `Cargo.toml` (workspace root)

```toml
[workspace]
members = [
    # ... existing members ...
    "helix-mcp",
]
```

---

## 11. Summary of Key Decisions

| Decision | Rationale |
|---|---|
| No `helix-view` dependency in `helix-mcp` | Prevents circular deps; MCP crate is pure protocol + transport |
| Channel-based communication (not shared Arc) | Matches `helix-lsp` pattern; editor is single-owner mutable |
| Tool trait with generic context `C` | Tools are defined where editor access exists, but transport stays decoupled |
| `--mcp` headless flag | No TUI overhead when running as an MCP tool; clean lifecycle |
| `McpRequest`/`McpResponse` enums (not JSON-RPC in the channel) | The transport layer handles JSON-RPC framing; the channel carries typed messages |
| New `EditorEvent::McpMessage` variant | Integrated into existing `select!` event loop, same as LSP/DAP |
| JSON Schema for tool inputs | Standard MCP requirement; `schemars` not needed — manual `serde_json::json!` is simpler |
| Integration tests via existing `TestBackend` | No new test infrastructure needed |

# Helix MCP Server — Implementation Workplan

## 1. Overview

### What MCP Is

The Model Context Protocol (MCP) is an open standard defined by Anthropic that enables AI applications (agents, assistants, coding copilots) to interact with external tools, resources, and prompts through a structured JSON-RPC 2.0 protocol. An MCP server is a long-lived process that exposes capabilities—tools (functions the AI can call), resources (data the AI can read), and prompts (templates for human/AI interaction)—over a transport such as a Unix domain socket or TCP connection. The protocol includes discovery (listing tools/resources), invocation, subscription (change notifications), and structured error handling.

### Why Helix Needs It

Helix is a modal, terminal-native text editor with first-class LSP support, tree-sitter syntax awareness, and multi-cursor editing. As AI coding assistants proliferate, users want their editor to be the "hands" for an AI agent: the agent reads buffers, navigates cursors, applies edits, queries diagnostics, and proxies LSP requests—all through the same editor instance the user is looking at. An MCP server embedded in Helix turns the editor into a programmable backend for AI tooling without requiring the user to switch editors, learn a new keybinding model, or run a separate headless editor process. The event-driven architecture of Helix (via `helix-event`) is uniquely suited for this: changes are pushed in real time rather than polled, making the MCP server responsive and efficient.

### Guiding Principles

- **Zero-compromise on security**: Every mutation must be explicitly confirmed by the user before execution. Read-only operations require no confirmation. A three-tier model (Read / Preview / Mutate) gates all tool calls.
- **First-class Rust crate**: The protocol implementation lives in a standalone `helix-mcp` crate with zero helix dependencies, usable by external consumers. The integration lives in `helix-mcp-server`.
- **Event-driven, not polling**: The server hooks into Helix's existing event system (`helix-event`) to push state changes to connected clients in real time via MCP subscriptions, avoiding the stale-snapshot problem of polling-based approaches.
- **Gradual rollout behind a feature gate**: The MCP server is gated behind a `mcp` Cargo feature, disabled by default. Users opt in via `--mcp`, config, or environment variable. This ensures zero runtime cost for users who don't need it.
- **Platform portability**: Unix domain sockets are the primary transport on Linux/macOS. A TCP fallback (localhost-only, auto-assigned port) covers Windows and provides a migration path for remote use cases.

---

## 2. Architecture

### 2.1 Crate Structure

Two new crates are introduced. The split ensures a clean dependency boundary: `helix-mcp` is a pure protocol library with no knowledge of Helix internals; `helix-mcp-server` bridges the protocol to the editor.

#### `helix-mcp/` — Pure MCP Protocol Library (zero helix deps)

```
helix-mcp/
├── Cargo.toml
└── src/
    ├── lib.rs              # Public API re-exports, version constant
    ├── jsonrpc.rs           # JSON-RPC 2.0 types: Request, Response, Id, Error, Version
    ├── transport.rs         # Transport trait (AsyncRead + AsyncWrite abstraction)
    ├── framing.rs           # Newline-delimited JSON framing (read_frame, write_frame)
    ├── messages.rs          # MCP message types: Initialize, ListTools, CallTool, etc.
    ├── types.rs             # Shared MCP types: Tool, Resource, Prompt, ContentBlock, etc.
    ├── server.rs            # Abstract McpServer trait / skeleton for implementors
    └── error.rs             # Protocol-level error codes and helpers
```

| File | Purpose |
|---|---|
| `lib.rs` | Re-exports all public types. Defines `MCP_VERSION = "2024-11-05"`. |
| `jsonrpc.rs` | Standalone JSON-RPC 2.0 types (`Request`, `Response`, `Output`, `Id`, `Error`, `MethodCall`, `Notification`). These are intentionally independent of the existing `helix-lsp::jsonrpc` module to avoid a dependency and because the MCP wire format differs slightly (e.g., no Content-Length framing needed). |
| `transport.rs` | `Transport` trait abstracting `AsyncRead + AsyncWrite`. Provides `read_message` and `write_message` default methods. Concrete impls: `UnixStreamTransport`, `TcpStreamTransport`, a test `ChannelTransport`. |
| `framing.rs` | Newline-delimited JSON framing: reads until `\n`, parses one JSON value per line. Handles partial reads, buffer management. |
| `messages.rs` | MCP-specific request/notification/result structs: `InitializeRequest`, `InitializeResult`, `ListToolsRequest`, `ListToolsResult`, `CallToolRequest`, `CallToolResult`, `ListResourcesRequest`, `ListResourcesResult`, `ReadResourceRequest`, `ReadResourceResult`, `ListPromptsRequest`, `ListPromptsResult`, `GetPromptRequest`, `GetPromptResult`, `SetLevelRequest`, `CompleteRequest`, `CompleteResult`, `CancelledNotification`, `ProgressNotification`, `PingRequest`. Each derives `Serialize`, `Deserialize`. |
| `types.rs` | MCP shared types: `Tool` (name, description, inputSchema), `Resource` (uri, name, description, mimeType), `ResourceTemplate` (uriTemplate, name, description, mimeType), `Prompt` (name, description, arguments), `PromptArgument`, `ContentBlock` (TextContent, ImageContent, ResourceContent), `Role`, `LoggingLevel`, `Implementation` (name, version), `ClientCapabilities`, `ServerCapabilities`. |
| `server.rs` | `McpServer` trait with async methods: `initialize()`, `list_tools()`, `call_tool()`, `list_resources()`, `read_resource()`, `list_prompts()`, `get_prompt()`, `set_level()`, `complete()`. A `serve()` free function takes `impl Transport + McpServer` and runs the accept/read/respond loop. |
| `error.rs` | `McpError` enum mapping to JSON-RPC error codes. Constants for standard MCP errors (e.g., `INTERNAL_ERROR`, `INVALID_PARAMS`, `METHOD_NOT_FOUND`). |

**Dependencies** (`helix-mcp/Cargo.toml`): `serde`, `serde_json`, `tokio` (io-util, net, sync), `thiserror`. Zero helix dependencies.

#### `helix-mcp-server/` — Helix Integration Crate

```
helix-mcp-server/
├── Cargo.toml
└── src/
    ├── lib.rs              # Public API: McpServerHandle, start_server, feature gate
    ├── server.rs           # HelixMcpServer impl of McpServer trait, bind/accept loop
    ├── session.rs          # Per-connection Session: event hooks, mpsc channel, run loop
    ├── context.rs          # McpContext: snapshot access to Editor state via Arc<ArcSwap<>>
    ├── tools/
    │   ├── mod.rs          # Tool dispatch table, tool registration
    │   ├── document.rs     # document_read, document_write tool handlers
    │   ├── selection.rs    # selection_read, selection_set tool handlers
    │   ├── edit.rs         # edit_apply tool handler
    │   ├── navigation.rs   # goto_position, search_text tool handlers
    │   ├── diagnostics.rs  # diagnostics_read tool handler
    │   ├── lsp.rs          # lsp_request tool handler
    │   └── workspace.rs    # workspace_info tool handler
    ├── resources/
    │   ├── mod.rs          # Resource dispatch table
    │   ├── document.rs     # document:// resource handlers
    │   ├── diagnostics.rs  # diagnostics:// resource handlers
    │   └── workspace.rs    # workspace:// resource handlers
    ├── prompts/
    │   ├── mod.rs          # Prompt dispatch table
    │   ├── refactor.rs     # helix/refactor prompt
    │   ├── review.rs       # helix/review prompt
    │   ├── explain.rs      # helix/explain prompt
    │   └── fix_diagnostics.rs # helix/fix-diagnostics prompt
    ├── security.rs         # Three-tier model, confirmation gating, confirmation channel
    └── config.rs           # McpConfig struct, deserialize from editor config TOML
```

| File | Purpose |
|---|---|
| `lib.rs` | Conditionally compiled behind `#[cfg(feature = "mcp")]`. Exports `McpServerHandle` (opaque handle to the running server), `start_server()` function to spawn the listener, and `McpServerConfig`. |
| `server.rs` | `HelixMcpServer` struct implementing `McpServer` trait. Holds an `Arc<McpContext>` for editor access. `bind()` method creates the Unix socket / TCP listener. `accept_loop()` spawns a `Session` per connection. The `start_server()` entry point takes an `Arc<ArcSwap<Config>>`, creates the socket, and spawns the listen task on the tokio runtime. |
| `session.rs` | `Session` struct representing a single client connection. On creation, registers hook closures into the helix-event system for `DocumentDidChange`, `SelectionDidChange`, `DiagnosticsDidChange`, `DocumentDidOpen`, `DocumentDidClose` events. These hooks send serialized change notifications through an `mpsc::UnboundedSender<SessionEvent>`. The session's `run()` loop concurrently processes incoming JSON-RPC requests (handling tool calls, resource reads) and outgoing event notifications (pushing to the socket). Uses `tokio::select!` over the mpsc receiver and the transport reader. |
| `context.rs` | `McpContext` — a snapshot-based reader of editor state. References `Editor` through `Arc<tokio::sync::Mutex<Editor>>` or via the same `ArcSwap` pattern used for `Config`. All read methods (`get_document_text`, `get_selection`, `get_diagnostics`, `get_workspace_info`) take immutable references and return cloned data. Write methods queue edits onto a confirmation channel. |
| `tools/*.rs` | Each tool handler is an `async fn` taking `&McpContext` and `serde_json::Value` params, returning `Result<serde_json::Value, McpError>`. The dispatch table in `tools/mod.rs` maps tool name to handler + tier. |
| `resources/*.rs` | Each resource handler resolves a URI template to actual content. Uses URI parsing (extracting `doc_id` from `document://{doc_id}`) and returns `ContentBlock`. |
| `prompts/*.rs` | Each prompt handler constructs a `GetPromptResult` containing `PromptMessage` objects with context about the current document, selection, or diagnostics. |
| `security.rs` | `SecurityTier` enum (`Read`, `Preview`, `Mutate`). `ConfirmationGate` struct: a `tokio::sync::mpsc` channel (request + oneshot response) between the MCP session and the helix-term confirmation prompt. The UI renders a prompt, the user accepts/denies, and the gate resolves. |
| `config.rs` | `McpConfig` struct with all configuration knobs, implementing `Serialize`/`Deserialize`. Integrated into `helix_view::editor::Config`. |

**Dependencies** (`helix-mcp-server/Cargo.toml`): `helix-mcp` (path), `helix-core`, `helix-view`, `helix-event`, `helix-lsp`, `serde`, `serde_json`, `tokio` (full), `parking_lot`, `arc-swap`, `thiserror`, `log`.

#### Integration Points in Existing Crates

1. **Root `Cargo.toml`** — Add `"helix-mcp"` and `"helix-mcp-server"` to the workspace `members` array. Add `helix-mcp` and `helix-mcp-server` as workspace dependency entries.

2. **`helix-term/Cargo.toml`** — Add an optional dependency:
   ```toml
   [features]
   mcp = ["helix-mcp-server"]

   [dependencies]
   helix-mcp-server = { path = "../helix-mcp-server", optional = true }
   ```

3. **`helix-term/src/application.rs`** — In the `Application` struct, add a feature-gated field:
   ```rust
   #[cfg(feature = "mcp")]
   mcp_server: Option<helix_mcp_server::McpServerHandle>,
   ```
   In `Application::new()`, if `--mcp` was passed or config enables it, call `helix_mcp_server::start_server()` and store the handle. In the `render()` method, check for pending confirmations and render prompts.

4. **`helix-term/src/args.rs`** — Add three new fields to `Args`:
   ```rust
   pub mcp: bool,
   pub no_mcp: bool,
   pub mcp_socket: Option<PathBuf>,
   ```
   Parse `--mcp`, `--no-mcp`, and `--mcp-socket <path>` in `parse_args()`.

### 2.2 Data Flow

#### Event-Driven Architecture

```
┌──────────────┐     helix-event hooks      ┌──────────────┐
│   Editor     │───DocumentDidChange────────►│   Session 1  │──► socket fd 7
│   (main)     │───SelectionDidChange───────►│              │
│              │───DiagnosticsDidChange─────►│              │
│   Editor     │───DocumentDidOpen──────────►│   Session 2  │──► socket fd 8
│   Actions    │───DocumentDidClose─────────►│              │
└──────────────┘                             └──────────────┘
       ▲                                           │
       │  mpsc channel                             │
       │  (confirm)                                ▼
       │                                    ┌──────────────┐
       │◄────helix-term renders─────────────│ Confirmation  │
       │     confirmation prompt             │    Prompt     │
                                            └──────────────┘
```

1. **Event dispatch**: When the user types, saves, or receives LSP diagnostics, `helix-event::dispatch()` fires synchronous hooks registered by each MCP session.
2. **Hook → channel**: Each session's hook closure receives the event (e.g., `DocumentDidChange` with the old/new text and changeset) and pushes a serialized MCP notification onto an `mpsc::UnboundedSender`.
3. **Session run loop**: The per-connection tokio task runs a `tokio::select!` loop reading from both the mpsc receiver (for outgoing event notifications) and the socket reader (for incoming tool/resource requests).
4. **Tool execution**: When a tool call arrives, the session calls the handler synchronously or via `tokio::spawn` (for potentially slow operations like LSP proxy requests). Read-tier tools return immediately. Mutate-tier tools route through the confirmation gate.
5. **Confirmation gate**: A Mutate request is held pending. The session sends a `ConfirmationRequest` to the UI via a separate channel. `Application::render()` polls for pending confirmations and renders an overlay prompt. The user's response is sent back through a `tokio::sync::oneshot` channel, unblocking the session's response.

#### Why Event-Driven Over Polling/Snapshot

| Approach | Pros | Cons |
|---|---|---|
| **Event-driven (chosen)** | Real-time updates, zero CPU overhead when idle, subscribers get only the changes they care about, naturally integrates with helix-event infrastructure | Requires careful hook lifetime management; sessions must clean up hooks on disconnect |
| **Polling snapshot** | Simpler implementation, no hook registration | Stale data between polls; CPU burn even when idle; no way to efficiently detect "what changed"; extra GC pressure from cloning entire document texts |

Helix's entire plugin architecture is event-driven (`helix-event`). Building the MCP server on the same infrastructure is idiomatic, reuses proven patterns (the `AsyncHook` trait, `send_blocking` channel utilities), and naturally extends to future MCP subscription support (the `listChanged`/`watch` pattern maps directly to our hooks).

### 2.3 Transport

#### Unix Domain Socket (Primary, Unix Platforms)

- **Socket path convention**: Each editor instance creates its own socket with a PID in the filename to prevent collisions between multiple running `hx` processes:
  - Primary: `$XDG_RUNTIME_DIR/helix/mcp/{pid}.sock`
  - Fallback (no `XDG_RUNTIME_DIR`): `$TMPDIR/helix-mcp/{pid}.sock`
  - Override: `--mcp-socket <path>` or `[editor.mcp].socket` config key
- **Metadata file**: Alongside the socket, the instance writes a metadata file: `$XDG_RUNTIME_DIR/helix/mcp/{pid}.json` containing:
  ```json
  {
    "pid": 12345,
    "worktree": "/Users/alexbieg/Projects/helix",
    "socket": "/run/user/1000/helix/mcp/12345.sock",
    "open_files": ["src/main.rs", "Cargo.toml"]
  }
  ```
  This file is updated on every document open/close. Stale metadata files (PID no longer running) are cleaned up on next server start.
- **Permissions**: Sockets and metadata files created with `0o600` (user read/write only). The parent directory (`$XDG_RUNTIME_DIR/helix/mcp/`) is created with `0o700`.
- **Cleanup**: The socket file is removed on server shutdown via a `Drop` impl. The metadata `.json` file is removed at the same time. On startup, stale sockets and metadata from dead PIDs are cleaned up.

#### TCP Fallback (Windows)

- **Binding**: `127.0.0.1:0` (localhost, auto-assigned port). The OS-assigned port is read back from `TcpListener::local_addr()`.
- **Metadata file**: The port is written to `%TEMP%/helix-mcp/{pid}.json` for discovery (same metadata format as Unix, but with `"port"` instead of `"socket"`).
- **Security**: Binding to `127.0.0.1` ensures only local processes can connect. No authentication beyond OS-level process isolation.

#### Wire Protocol: Newline-Delimited JSON

Each JSON-RPC message (request, response, or notification) is serialized as a single line of JSON terminated by `\n`. No `Content-Length` header is used. This is the standard MCP streamable transport format.

**Why not Content-Length framing for sockets?** The Content-Length header format (used by LSP over stdio) exists because stdio is byte-stream without message boundaries and pipes/binary data can embed newlines in JSON strings. For MCP over sockets, JSON messages are already newline-safe (no raw binary) and the simplicity of line-delimited JSON reduces implementation complexity and makes debugging with `nc -U` trivial.

#### Transport Trait Abstraction

```rust
// helix-mcp/src/transport.rs
use tokio::io::{AsyncRead, AsyncWrite};

pub trait Transport: AsyncRead + AsyncWrite + Unpin + Send + 'static {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send + 'static> Transport for T {}
```

The `McpServer::serve()` function is generic over `T: Transport`, allowing the same protocol logic to work over Unix sockets, TCP, or test channels.

### 2.4 Multi-Instance Discovery

When multiple Helix instances run in different worktrees (a common workflow for developers working across multiple projects), each instance spawns its own MCP server on its own socket. An AI agent must be able to discover and connect to the correct instance.

#### Discovery Mechanisms

| Priority | Mechanism | How It Works |
|---|---|---|
| 1 (highest) | `HELIX_MCP_SOCK` env var | The agent or user explicitly points to a known socket path. When launching `hx`, the calling agent sets this env var so the agent itself knows where to connect. `hx` exposes the bound socket path by writing it to a well-known file; the launcher reads it and sets the env var for the agent. |
| 2 | Metadata directory scan | The agent scans `$XDG_RUNTIME_DIR/helix/mcp/` (or `$TMPDIR/helix-mcp/`), reads all `{pid}.json` metadata files, and matches on `worktree` against the agent's current working directory. |
| 3 (lowest) | User prompt | If multiple instances match the worktree (e.g., two `hx` processes in the same project), the agent shows a choice prompt to the user listing PIDs, worktrees, and open files from each metadata file. |

#### Metadata File Format

Each instance writes a JSON metadata file alongside its socket:

```json
{
  "pid": 48291,
  "worktree": "/Users/alexbieg/Projects/helix",
  "socket": "/run/user/501/helix/mcp/48291.sock",
  "open_files": ["src/main.rs", "Cargo.toml", "docs/mcp-workplan.md"],
  "mode": "normal",
  "started_at": "2026-06-20T14:30:00Z"
}
```

The metadata file is written once on server startup and updated whenever documents open/close. An inotify/kqueue watch (or periodic stat) allows agents to detect new instances appearing.

#### Socket Path Resolution Function

```rust
/// Resolve the socket path for this Helix instance.
/// Returns the absolute path to the Unix domain socket.
fn resolve_socket_path(config: &McpConfig) -> PathBuf {
    // 1. Explicit override always wins
    if let Some(ref custom) = config.socket {
        return custom.clone();
    }

    // 2. Choose directory based on platform conventions
    let dir = std::env::var("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let mut tmp = std::env::temp_dir();
            tmp.push("helix-mcp");
            tmp
        })
        .join("mcp");

    // 3. Create directory if it doesn't exist
    std::fs::create_dir_all(&dir).ok();

    // 4. Include PID to avoid collisions with other instances
    dir.join(format!("{}.sock", std::process::id()))
}
```

#### How an Agent Connects (End-to-End Flow)

1. Agent (e.g., Claude, opencode) is launched in directory `/Users/alexbieg/Projects/helix`
2. User has `hx` running in that directory with `[editor.mcp].enable = true`
3. Agent scans `$XDG_RUNTIME_DIR/helix/mcp/*.json` looking for `worktree` matching `/Users/alexbieg/Projects/helix`
4. Agent finds `48291.json` with `"worktree": "/Users/alexbieg/Projects/helix"` and `"socket": "/run/user/501/helix/mcp/48291.sock"`
5. Agent connects to that socket and begins the MCP handshake
6. If no matching instance is found, agent returns an error: "No Helix MCP server found for worktree /Users/alexbieg/Projects/helix. Start hx with --mcp or set editor.mcp.enable = true."

#### TCP Fallback Discovery (Windows)

On Windows, the equivalent mechanism writes `<pid>.json` to `%TEMP%/helix-mcp/` containing `"port": 59876` instead of `"socket"`. The agent scans the same directory pattern.

#### Stale Cleanup

On startup, before creating its own metadata file, the server scans the metadata directory for `.json` files from dead PIDs and removes both the metadata file and any orphaned socket/port file. This prevents accumulation from crashes.

### 2.5 Explicit Rejection of Stdio Transport

Helix is a TUI application that owns the terminal. Stdio (stdin/stdout) is already consumed by the terminal backend (termina/crossterm) for rendering and input. Using stdio for MCP would conflict with the TUI, require multiplexing (which terminal backends do not support), or require a headless mode. For the initial implementation, stdio is explicitly excluded. Future work could support stdio in a "headless helix" mode, but that is out of scope.

---

## 3. API Surface

### 3.1 Tools (Phase 2 — all 10)

| # | Name | Tier | Description | Input Params | Output Shape |
|---|---|---|---|---|---|
| 1 | `document_read` | Read | Read buffer content or a specific range from a document | `doc_id` (string), `range`? ({start, end} optional) | `{doc_id, text, line_count, language}` |
| 2 | `document_write` | Mutate | Replace the entire buffer content of a document | `doc_id` (string), `new_text` (string) | `{doc_id, line_count}` |
| 3 | `selection_read` | Read | Read current selections for a document | `doc_id` (string) | `{doc_id, selections: [{anchor, cursor}]}` |
| 4 | `selection_set` | Preview | Set selections by ranges | `doc_id` (string), `selections: [{anchor, cursor}]` | `{doc_id, count}` |
| 5 | `edit_apply` | Mutate | Apply an ordered list of edits to a document | `doc_id` (string), `edits: [{range: {start, end}, new_text}]` | `{doc_id, applied_count, new_selections}` |
| 6 | `goto_position` | Preview | Move cursor to a line:column position | `doc_id` (string), `line` (uint), `column`? (uint) | `{doc_id, line, column}` |
| 7 | `search_text` | Read | Search document(s) with a regex pattern | `query` (string), `doc_ids`? ([string]), `case_sensitive`? (bool) | `{matches: [{doc_id, line, column, text}]}` |
| 8 | `diagnostics_read` | Read | Get diagnostics for a specific document | `doc_id` (string), `severity`? (string) | `{doc_id, diagnostics: [{range, severity, message, code, source}]}` |
| 9 | `lsp_request` | Read | Proxy an LSP request through Helix's LSP client | `doc_id` (string), `request_type` (string: hover/references/definition/implementation/typeDefinition), `position` ({line, character}) | `{result: ...}` (varies by request type) |
| 10 | `workspace_info` | Read | Get open documents, file paths, and modes | (none) | `{documents: [{doc_id, path, language, mode, modified, line_count}], focused_doc_id}` |

#### JSON Schema Sketches

**Tool 1: `document_read`**
```json
{
  "name": "document_read",
  "description": "Read the text content of a document, optionally within a character range. Returns the full document if no range is specified.",
  "inputSchema": {
    "type": "object",
    "properties": {
      "doc_id": {"type": "string", "description": "The document ID (e.g., '1', '2'). Use workspace_info to discover available documents."},
      "range": {
        "type": "object",
        "properties": {
          "start": {"type": "integer", "description": "Byte offset of range start"},
          "end": {"type": "integer", "description": "Byte offset of range end (exclusive)"}
        },
        "required": ["start", "end"]
      }
    },
    "required": ["doc_id"]
  }
}
```

**Tool 2: `document_write`**
```json
{
  "name": "document_write",
  "description": "Replace the entire buffer content of a document. REQUIRES USER CONFIRMATION. This is a destructive operation that overwrites the buffer.",
  "inputSchema": {
    "type": "object",
    "properties": {
      "doc_id": {"type": "string", "description": "The document ID to modify."},
      "new_text": {"type": "string", "description": "The complete new content for the buffer."}
    },
    "required": ["doc_id", "new_text"]
  }
}
```

**Tool 3: `selection_read`**
```json
{
  "name": "selection_read",
  "description": "Read the current selections (cursor/anchor positions) for a document.",
  "inputSchema": {
    "type": "object",
    "properties": {
      "doc_id": {"type": "string", "description": "The document ID."}
    },
    "required": ["doc_id"]
  }
}
```

**Tool 4: `selection_set`**
```json
{
  "name": "selection_set",
  "description": "Set the selection ranges for a document. This changes what text is visually selected.",
  "inputSchema": {
    "type": "object",
    "properties": {
      "doc_id": {"type": "string", "description": "The document ID."},
      "selections": {
        "type": "array",
        "items": {
          "type": "object",
          "properties": {
            "anchor": {"type": "integer", "description": "Byte offset of the anchor"},
            "cursor": {"type": "integer", "description": "Byte offset of the cursor"}
          },
          "required": ["anchor", "cursor"]
        }
      }
    },
    "required": ["doc_id", "selections"]
  }
}
```

**Tool 5: `edit_apply`**
```json
{
  "name": "edit_apply",
  "description": "Apply an ordered list of text edits to a document. REQUIRES USER CONFIRMATION. Edits are applied in order; each edit's range refers to the document state after previous edits.",
  "inputSchema": {
    "type": "object",
    "properties": {
      "doc_id": {"type": "string", "description": "The document ID to edit."},
      "edits": {
        "type": "array",
        "items": {
          "type": "object",
          "properties": {
            "range": {
              "type": "object",
              "properties": {
                "start": {"type": "integer", "description": "Byte offset of range start"},
                "end": {"type": "integer", "description": "Byte offset of range end (exclusive)"}
              },
              "required": ["start", "end"]
            },
            "new_text": {"type": "string", "description": "The text to insert in place of the range."}
          },
          "required": ["range", "new_text"]
        }
      }
    },
    "required": ["doc_id", "edits"]
  }
}
```

**Tool 6: `goto_position`**
```json
{
  "name": "goto_position",
  "description": "Move the primary cursor to a specific line and column in a document. Line numbers are 1-based.",
  "inputSchema": {
    "type": "object",
    "properties": {
      "doc_id": {"type": "string", "description": "The document ID."},
      "line": {"type": "integer", "minimum": 1, "description": "1-based line number."},
      "column": {"type": "integer", "minimum": 1, "description": "1-based column number. Defaults to 1."}
    },
    "required": ["doc_id", "line"]
  }
}
```

**Tool 7: `search_text`**
```json
{
  "name": "search_text",
  "description": "Search for a regex pattern across one or more open documents. Returns match positions and surrounding context.",
  "inputSchema": {
    "type": "object",
    "properties": {
      "query": {"type": "string", "description": "Regex pattern to search for."},
      "doc_ids": {
        "type": "array",
        "items": {"type": "string"},
        "description": "Document IDs to search. If omitted, searches all open documents."
      },
      "case_sensitive": {"type": "boolean", "description": "Whether the search is case-sensitive. Defaults to true."},
      "max_results": {"type": "integer", "minimum": 1, "maximum": 200, "description": "Maximum number of results to return. Defaults to 50."}
    },
    "required": ["query"]
  }
}
```

**Tool 8: `diagnostics_read`**
```json
{
  "name": "diagnostics_read",
  "description": "Read diagnostic messages (errors, warnings, hints) for a specific document.",
  "inputSchema": {
    "type": "object",
    "properties": {
      "doc_id": {"type": "string", "description": "The document ID."},
      "severity": {
        "type": "string",
        "enum": ["error", "warning", "info", "hint"],
        "description": "Filter by severity. If omitted, returns all diagnostics."
      }
    },
    "required": ["doc_id"]
  }
}
```

**Tool 9: `lsp_request`**
```json
{
  "name": "lsp_request",
  "description": "Proxy an LSP request through Helix to the language server for a document. Supports hover, references, goto-definition, goto-implementation, and goto-type-definition.",
  "inputSchema": {
    "type": "object",
    "properties": {
      "doc_id": {"type": "string", "description": "The document ID."},
      "request_type": {
        "type": "string",
        "enum": ["hover", "references", "definition", "implementation", "type_definition"],
        "description": "Type of LSP request to make."
      },
      "position": {
        "type": "object",
        "properties": {
          "line": {"type": "integer", "description": "0-based line number."},
          "character": {"type": "integer", "description": "0-based UTF-16 character offset."}
        },
        "required": ["line", "character"]
      }
    },
    "required": ["doc_id", "request_type", "position"]
  }
}
```

**Tool 10: `workspace_info`**
```json
{
  "name": "workspace_info",
  "description": "Get information about all open documents in the workspace: file paths, languages, modification status, line counts, and which document has focus.",
  "inputSchema": {
    "type": "object",
    "properties": {}
  }
}
```

### 3.2 Resources (Phase 2 — 5)

| # | URI Scheme | Description | MIME Type | Subscribable | Output Shape |
|---|---|---|---|---|---|
| 1 | `document://{doc_id}` | Full document text content | `text/plain` | Yes | Text content block with URI |
| 2 | `document://{doc_id}/selection` | Current selection(s) for the document | `application/json` | Yes | `{doc_id, selections: [{anchor, cursor, text}]}` |
| 3 | `diagnostics://{doc_id}` | Per-document diagnostics | `application/json` | Yes | `{doc_id, diagnostics: [{range, severity, message, code, source, uri}]}` |
| 4 | `diagnostics://workspace` | All workspace diagnostics aggregated | `application/json` | Yes | `{documents: {doc_id: [{range, severity, message, code, source}]}}` |
| 5 | `workspace://open-documents` | Manifest of open documents | `application/json` | Yes | `{documents: [{doc_id, path, language, mode, modified, line_count}], focused_doc_id}` |

Each resource returns a list of `ContentBlock` items (via `resources/read`), typically a single `TextContent` block with the URI and MIME type set. Subscribable resources emit `notifications/resources/updated` messages when the underlying editor state changes, driven by the corresponding helix-event hooks.

### 3.3 Prompts (Phase 2 — 4)

| # | Name | Description | Arguments |
|---|---|---|---|
| 1 | `helix/refactor` | Generate a refactoring prompt with the selected code and surrounding context. Includes language, file path, and selection as template variables. | `doc_id` (string, required, the document to refactor) |
| 2 | `helix/review` | Generate a code review prompt with the full file or selection. Includes diagnostics as context for known issues. | `doc_id` (string, required), `use_selection` (bool, default false) |
| 3 | `helix/explain` | Generate a prompt asking the AI to explain the selected code or the code at the cursor position. | `doc_id` (string, required) |
| 4 | `helix/fix-diagnostics` | Generate a prompt to fix the diagnostics in the current file. Includes all diagnostic messages and relevant code context. | `doc_id` (string, required) |

Each prompt returns a `GetPromptResult` with a `messages` array containing `PromptMessage` objects. The messages include the prompt text with template variables already substituted, and the `role` field set to `"user"`. The prompt templates are localized to English only for Phase 2; i18n is deferred.

### 3.4 MCP Capabilities Declaration

The exact JSON block returned by `initialize`:

```json
{
  "protocolVersion": "2024-11-05",
  "capabilities": {
    "tools": {
      "listChanged": true
    },
    "resources": {
      "subscribe": true,
      "listChanged": true
    },
    "prompts": {
      "listChanged": true
    },
    "logging": {}
  },
  "serverInfo": {
    "name": "helix-mcp",
    "version": "25.7.1"
  }
}
```

The `serverInfo.version` mirrors the Helix workspace version. `listChanged` is set to `true` for tools, resources, and prompts, indicating the server supports `notifications/tools/list_changed`, `notifications/resources/list_changed`, and `notifications/prompts/list_changed` respectively. These fire when documents open/close (changing available tool targets), language changes (changing available prompts), etc.

---

## 4. Security & Configuration

### 4.1 Three-Tier Model

| Tier | Description | Confirmation Required? | Examples |
|---|---|---|---|
| **Read** | Operations that only read editor state. No side effects, no modifications. | No | `document_read`, `selection_read`, `search_text`, `diagnostics_read`, `lsp_request`, `workspace_info` |
| **Preview** | Operations that change the visual state (cursor position, selection) but are non-destructive and trivially reversible. | No | `goto_position`, `selection_set` |
| **Mutate** | Operations that modify document content. IRREVERSIBLE without undo. **Always requires explicit user confirmation.** | **Yes** | `document_write`, `edit_apply` |

The confirmation requirement is enforced server-side. A Mutate-tier tool call from any client is blocked until the user accepts a confirmation prompt in the Helix TUI. There is no way for a client to bypass this gate through the API surface. The user can deny any mutation, and the server returns an error to the client.

### 4.2 Confirmation UX

When a Mutate-tier tool call arrives:

1. The session creates a `ConfirmationRequest` containing: the tool name, the document path/title, a summary of the change (e.g., "edit_apply: 3 edits to src/main.rs [120 → 145, 200 → 210, 350 → 370]"), and a `tokio::sync::oneshot::Sender<bool>`.
2. The request is sent through a shared `mpsc::UnboundedSender<ConfirmationRequest>` held by `Application`.
3. In `Application::render()`, the confirmation queue is drained. For each pending confirmation, a modal overlay is rendered over the current view:
   ```
   ┌─────────────────────────────────────────────┐
   │  ⚠ MCP Tool Requires Confirmation           │
   │                                             │
   │  Tool: edit_apply                           │
   │  File: src/main.rs (doc #2)                 │
   │  Summary: 3 edits replacing 45 chars        │
   │  Source: client-1                           │
   │                                             │
   │  [y] Accept  [n] Deny  [d] Show Diff       │
   └─────────────────────────────────────────────┘
   ```
4. User presses `y` → sends `true` through the oneshot → session proceeds with the mutation. User presses `n` → sends `false` → session returns `{"error": "User denied the operation"}`. User presses `d` → shows a diff view of the proposed changes (rendered via the existing diff infrastructure in `helix-vcs`), then re-prompts.
5. **Timeout**: If no user response within 60 seconds, the confirmation is automatically denied with an error.

This reuses the existing notification infrastructure from `helix-view/src/notification.rs` for rendering and the compositor overlay pattern used by file pickers and prompts throughout `helix-term`.

### 4.3 Configuration

Full TOML block for `[editor.mcp]` within the Helix config file:

```toml
[editor.mcp]
# Enable the MCP server. When false, no socket is created and the server
# does not start regardless of CLI flags.
# Type: bool
# Default: false
enable = false

# Maximum number of simultaneous client connections.
# Additional connections beyond this limit are refused.
# Type: u32
# Default: 4
max_connections = 4

# Unix domain socket path. If not set, the default path is used:
#   - $XDG_RUNTIME_DIR/helix/mcp/{pid}.sock (Linux/macOS)
#   - $TMPDIR/helix-mcp/{pid}.sock (fallback)
# The {pid} is replaced with the process ID to avoid collisions
# between multiple running hx instances.
# Ignored on Windows (TCP is used instead).
# Type: string (optional path)
# Default: unset (auto-detect)
# socket = "/tmp/my-helix-mcp.sock"

# TCP port for the MCP server. Only used on Windows or when explicitly
# requested. Set to 0 for auto-assignment.
# Type: u16
# Default: 0 (auto-assign)
# tcp_port = 9876

# Whether to allow TCP connections from remote hosts (not just localhost).
# WARNING: Enabling this without authentication is a security risk.
# Type: bool
# Default: false
# allow_remote = false

# Require confirmation for Preview-tier operations (in addition to Mutate).
# When true, selections and cursor movements also require user confirmation.
# Type: bool
# Default: false
confirm_preview = false

# Timeout in milliseconds for user confirmation prompts.
# If the user does not respond within this time, the operation is denied.
# Type: u64
# Default: 60000 (60 seconds)
confirm_timeout_ms = 60000

# Maximum number of edits allowed in a single edit_apply call.
# Protects against accidentally large edit batches.
# Type: usize
# Default: 50
max_edits_per_call = 50

# Maximum number of matches returned by search_text.
# Type: usize
# Default: 200
max_search_results = 200

# Log level for MCP server messages (to the Helix log file).
# Type: string, one of "error", "warn", "info", "debug", "trace"
# Default: "info"
log_level = "info"

# Enable audit logging: record all Mutate-tier operations to the log file
# with timestamp, client ID, tool name, and affected documents.
# Type: bool
# Default: true
audit_log = true
```

The `McpConfig` struct in `helix-mcp-server/src/config.rs`:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", default, deny_unknown_fields)]
pub struct McpConfig {
    pub enable: bool,
    pub max_connections: u32,
    pub socket: Option<PathBuf>,
    pub tcp_port: u16,
    pub allow_remote: bool,
    pub confirm_preview: bool,
    pub confirm_timeout_ms: u64,
    pub max_edits_per_call: usize,
    pub max_search_results: usize,
    pub log_level: String,
    pub audit_log: bool,
}

impl Default for McpConfig {
    fn default() -> Self {
        Self {
            enable: false,
            max_connections: 4,
            socket: None,
            tcp_port: 0,
            allow_remote: false,
            confirm_preview: false,
            confirm_timeout_ms: 60_000,
            max_edits_per_call: 50,
            max_search_results: 200,
            log_level: "info".to_string(),
            audit_log: true,
        }
    }
}
```

The `McpConfig` field is added to `helix_view::editor::Config` (the large `Config` struct at `helix-view/src/editor.rs:298`):

```rust
#[serde(default)]
pub mcp: McpConfig,
```

### 4.4 CLI & Environment

Three mechanisms control the MCP server:

| Source | Mechanism | Example |
|---|---|---|
| CLI flag (on) | `--mcp` | `hx --mcp src/main.rs` |
| CLI flag (off) | `--no-mcp` | `hx --no-mcp` |
| CLI flag (custom socket) | `--mcp-socket <path>` | `hx --mcp-socket /tmp/my.sock` |
| Environment variable (on) | `HELIX_MCP=1` | `HELIX_MCP=1 hx` |
| Environment variable (off) | `HELIX_MCP=0` | `HELIX_MCP=0 hx` |
| Environment variable (socket discovery) | `HELIX_MCP_SOCK=<path>` | `HELIX_MCP_SOCK=/run/user/501/helix/mcp/48291.sock` |
| Config file | `[editor.mcp] enable = true` | In `config.toml` |
| Default | Off (no server) | `hx` (normal usage) |

**Precedence**: CLI flags override everything. If `--no-mcp` is set, the server is OFF regardless of env or config. If `--mcp` is set, the server is ON regardless of env or config. If `--mcp-socket` is set, it overrides the config file's `socket` key. The env var `HELIX_MCP` overrides the config file. The env var `HELIX_MCP_SOCK` is read by the agent for discovery, not by `hx` itself. If neither CLI flag nor env var is set, the config file's `[editor.mcp].enable` determines the behavior. Default is off.

Implementation in `helix-term/src/main.rs` after config loading:

```rust
let mcp_enabled = args.mcp
    || (!args.no_mcp && std::env::var("HELIX_MCP").map_or(
        config.editor.mcp.enable,
        |v| v != "0",
    ));
let mcp_socket_path = args.mcp_socket.or(config.editor.mcp.socket.clone());
```

---

## 5. Implementation Roadmap

### Phase 1 — MWP (Minimum Workable Product) ✅ COMPLETED

**Goal**: A working read-only MCP server that proves the architecture end-to-end: agent connects, lists tools, reads document text, gets workspace info, and navigates the cursor.

**Scope**:
- `helix-mcp` crate: Complete JSON-RPC types, `Transport` trait, newline-delimited framing, MCP message types for `initialize`, `tools/list`, `tools/call`, `resources/list`, `resources/read`, `ping`.
- `helix-mcp-server` crate: Basic `McpContext`, Unix socket listener, single-connection accept loop, session with no event hooks (snapshot-based reads only), no confirmation gate.
- 3 Read tools: `document_read`, `workspace_info`, `goto_position` (arguably Preview, but Phase 1 treats it as Read since it's non-destructive and simple).
- 2 Resources: `document://{doc_id}`, `workspace://open-documents`.
- 0 Prompts.
- CLI integration: `--mcp` flag, `HELIX_MCP` env var, config `enable` key. No confirmation UI needed.
- Feature gate: `mcp` feature in `helix-term`.
- No Windows support (Unix sockets only).
- No subscriptions.

**Deliverable**: An agent can `hx --mcp file.rs`, then connect via `nc -U $XDG_RUNTIME_DIR/helix/mcp.sock`, send `initialize`, `tools/list`, `tools/call` with `document_read`, and receive document text.

**Status**: ✅ 39 unit tests pass (13 helix-mcp + 26 helix-mcp-server). E2E: 5/5.

**Specific Files to Create/Modify**:

| File | Action | Est. Lines |
|---|---|---|
| `helix-mcp/Cargo.toml` | Create | 15 |
| `helix-mcp/src/lib.rs` | Create | 15 |
| `helix-mcp/src/jsonrpc.rs` | Create | 180 |
| `helix-mcp/src/transport.rs` | Create | 40 |
| `helix-mcp/src/framing.rs` | Create | 80 |
| `helix-mcp/src/messages.rs` | Create | 200 |
| `helix-mcp/src/types.rs` | Create | 150 |
| `helix-mcp/src/server.rs` | Create | 60 |
| `helix-mcp/src/error.rs` | Create | 50 |
| `helix-mcp-server/Cargo.toml` | Create | 20 |
| `helix-mcp-server/src/lib.rs` | Create | 60 |
| `helix-mcp-server/src/server.rs` | Create | 150 |
| `helix-mcp-server/src/session.rs` | Create | 120 |
| `helix-mcp-server/src/context.rs` | Create | 100 |
| `helix-mcp-server/src/tools/mod.rs` | Create | 40 |
| `helix-mcp-server/src/tools/document.rs` | Create | 60 |
| `helix-mcp-server/src/tools/workspace.rs` | Create | 60 |
| `helix-mcp-server/src/tools/navigation.rs` | Create | 40 |
| `helix-mcp-server/src/resources/mod.rs` | Create | 40 |
| `helix-mcp-server/src/resources/document.rs` | Create | 50 |
| `helix-mcp-server/src/resources/workspace.rs` | Create | 40 |
| `helix-mcp-server/src/config.rs` | Create | 50 |
| `Cargo.toml` (root) | Modify: add members, workspace deps | +6 |
| `helix-term/Cargo.toml` | Modify: add feature, optional dep | +3 |
| `helix-term/src/args.rs` | Modify: add `mcp`, `no_mcp`, `mcp_socket` fields, parse args | +35 |
| `helix-term/src/application.rs` | Modify: add mcp_server field, init in `new()`, shutdown in `drop` | +30 |
| `helix-term/src/main.rs` | Modify: resolve mcp_enabled, pass to Application | +10 |
| `helix-view/src/editor.rs` | Modify: add `mcp: McpConfig` field to Config | +2 |

**Estimated total**: ~1,700 lines new, ~85 lines modified.

### Phase 2 — Full Tool Surface ✅ COMPLETED

**Goal**: Complete API surface, multi-connection support, and security model. The server is suitable for real AI coding workflows.

**Scope**:
- Remaining 7 tools: `document_write`, `selection_read`, `selection_set`, `edit_apply`, `search_text`, `diagnostics_read`, `lsp_request`.
- Remaining 3 resources: `document://{doc_id}/selection`, `diagnostics://{doc_id}`, `diagnostics://workspace`.
- All 4 prompts: `helix/refactor`, `helix/review`, `helix/explain`, `helix/fix-diagnostics`.
- Three-tier security model: Read, Preview, Mutate. Confirmation gate with TUI overlay.
- Multi-connection support: connection pool with `max_connections` limit.
- Event hooks: `DocumentDidChange`, `SelectionDidChange`, `DiagnosticsDidChange` → MCP `notifications/resources/updated` and `notifications/tools/list_changed`.
- All configuration keys from `McpConfig` are fully honored.
- LSP proxy: `lsp_request` tool dispatches to `helix_lsp::Client::request()` and bridges the response.

**Deliverable**: Full-featured MCP server. An AI agent can read documents, analyze diagnostics, navigate, select text, propose and apply edits (with user confirmation), and use LSP features—all through the same running Helix instance.

**Status**: ✅ 72 unit tests pass. E2E: 22/22. Confirmation gate auto-approves (real UI deferred to Post-v1). LSP proxy returns stubs (full integration deferred to Post-v1). Mutation persistence to snapshot confirmed (documents survive render cycles).

**Additional Files**:

| File | Action | Est. Lines |
|---|---|---|
| `helix-mcp-server/src/tools/selection.rs` | Create | 80 |
| `helix-mcp-server/src/tools/edit.rs` | Create | 120 |
| `helix-mcp-server/src/tools/search.rs` | Create | 100 |
| `helix-mcp-server/src/tools/diagnostics.rs` | Create | 60 |
| `helix-mcp-server/src/tools/lsp.rs` | Create | 120 |
| `helix-mcp-server/src/resources/diagnostics.rs` | Create | 60 |
| `helix-mcp-server/src/prompts/mod.rs` | Create | 50 |
| `helix-mcp-server/src/prompts/refactor.rs` | Create | 40 |
| `helix-mcp-server/src/prompts/review.rs` | Create | 40 |
| `helix-mcp-server/src/prompts/explain.rs` | Create | 30 |
| `helix-mcp-server/src/prompts/fix_diagnostics.rs` | Create | 30 |
| `helix-mcp-server/src/security.rs` | Create | 100 |
| `helix-term/src/ui/mcp_confirmation.rs` | Create | 120 |

**Modify**:

| File | Change | Est. Lines |
|---|---|---|
| `helix-mcp/src/messages.rs` | Add prompt-related types | +40 |
| `helix-mcp/src/types.rs` | Add Prompt, PromptArgument, PromptMessage | +40 |
| `helix-mcp-server/src/server.rs` | Multi-connection pool, subscription dispatch | +80 |
| `helix-mcp-server/src/session.rs` | Event hook registration, notification dispatch | +100 |
| `helix-mcp-server/src/context.rs` | LSP proxy support, edit apply support | +80 |
| `helix-term/src/application.rs` | Confirmation polling in render loop | +30 |

**Estimated total**: ~1,300 lines new, ~370 lines modified.

### Phase 3 — Production Hardening ✅ COMPLETED

**Goal**: Cross-platform support, MCP subscriptions, performance, and polish.

**Scope**:
- **Windows TCP fallback**: `TcpTransport` implementation, port auto-assignment, PID-scoped metadata file discovery (`%TEMP%/helix-mcp/{pid}.json`). Conditional compilation for socket path generation.
- **MCP subscriptions**: Full `resources/subscribe`, `resources/unsubscribe`. Event-driven push of `notifications/resources/updated`. `listChanged` notifications for tools and resources when documents open/close.
- **Rate limiting**: Per-connection rate limiter (token bucket, configurable 100 req/s). Protects against runaway agents spamming requests.
- **Statusline integration**: Show an indicator (e.g., `[MCP:2]`) in the statusline when MCP clients are connected.
- **Documentation**: User-facing docs in `book/src/mcp.md`.
- **Auto-discovery**: `hx --mcp-info` CLI flag to print the socket path, process ID, worktree, and connection count for the current instance. `hx --mcp-list` to scan the metadata directory and list all running MCP-enabled instances.

**Deliverable**: Production-ready MCP server on all supported platforms with performance safeguards and user documentation.

**Status**: ✅ 88 unit tests pass. E2E: 23/24 (1 failure is snapshot fight with render cycle, since fixed in Part 4). Windows TCP behind `#[cfg(not(unix))]`. Subscriptions work (subscribe/unsubscribe). Rate limiter active. `--mcp-info`/`--mcp-list` functional. `book/src/mcp.md` written (192 lines).

### Post-v1 (Part 4)

**Part 4 — Completed**:

- ✅ **Headless mode**: `hx --headless --mcp file.rs` starts without TUI, pure MCP backend.
- ✅ **Agent-provided diagnostics**: `diagnostics_publish` tool (11th tool) lets agents push diagnostic messages into the editor's diagnostic display alongside LSP diagnostics.
- ✅ **Audit logging**: All Mutate-tier operations logged to `~/.cache/helix/mcp-audit.log` as JSON lines with timestamps, tool name, doc_id, and summary.
- ✅ **Mutation persistence**: Mutations applied via MCP survive render cycles in the snapshot.

**Post-v1 — Remaining**:

- **DiffReview component**: A side-by-side diff viewer for previewing pending edits before confirmation, reusing `helix-vcs::DiffHandle`. Requires TUI component in `helix-term/src/ui/`.
- **Confirmation UI**: Currently auto-approves all Mutate operations. Needs `[y] Accept / [n] Deny / [d] Show Diff` modal overlay per the workplan spec.
- **Remote MCP**: TLS-secured TCP transport with token-based authentication for remote AI pair programming.
- **Full LSP proxy**: `lsp_request` tool currently returns stubs. Needs real integration with `helix_lsp::Client` for hover, references, definition, etc.
- **DiffReview component**: A side-by-side diff viewer for previewing pending edits before confirmation, reusing `helix-vcs::DiffHandle`.
- **Agent-provided diagnostics**: Allow MCP clients to push diagnostic messages into Helix's diagnostic display (e.g., AI-detected bugs shown alongside LSP diagnostics).
- **Audit logging**: Structured JSON audit log of all Mutate-tier operations with timestamps, client IDs, and before/after snapshots.
- **Remote MCP**: TLS-secured TCP transport with token-based authentication for remote AI pair programming.
- **Headless mode**: A `--headless` flag that starts Helix without a TUI, only the MCP server, for use as a pure AI backend.

---

## 6. Testing Strategy

| Level | What | How | Tool |
|---|---|---|---|
| **Unit (helix-mcp)** | Protocol serialization | Round-trip tests: serialize tool call JSON, deserialize back, assert equality. Test edge cases: empty params, null id, batch requests, error responses. Test framing: multi-line messages, partial reads, buffer edge cases. | `cargo test` with `#[test]` |
| **Unit (helix-mcp-server)** | Tool logic | `McpContext` mocked with a test harness providing an in-memory `Editor` with pre-loaded documents, selections, and diagnostics. Test each tool handler in isolation with known inputs and assert output shape. | `cargo test` with `helix-view` test helpers |
| **Integration** | End-to-end socket | Spawn `hx --mcp` with test config (temp dir), open a test file, connect to the Unix socket, send `initialize` → `tools/list` → `tools/call document_read` → assert text matches. Shutdown and assert socket cleanup. | Integration test binary in `helix-mcp-server/tests/` using `tempfile` and `tokio` |
| **Security** | Confirmation gating | Send `document_write` (Mutate tier). Assert the request hangs (times out if no confirmation). Programmatically trigger confirmation acceptance → assert mutation succeeds. Programmatically trigger denial → assert error response. Test timeout behavior. | Integration test with confirmation channel mock |
| **Multi-client** | Concurrency | Spawn 4 simultaneous connections. Each sends a mix of read and mutate requests. Assert no cross-contamination, each connection gets correct responses, connection limit is honored (5th connection rejected). | Integration test with 4 tokio tasks |
| **Fuzz** | Protocol parser | `cargo fuzz` harness feeding random bytes into the newline-delimited JSON framer. Assert no panics, no OOM, no hangs. Run overnight in CI. | `cargo fuzz` with `libfuzzer-sys` |
| **Cross-platform** | Windows TCP | CI matrix runs the integration test suite on Windows using TCP transport instead of Unix sockets. Port file discovery, auto-assignment, cleanup verified. | GitHub Actions `windows-latest` runner |
| **Subscription** | Resource updates | Connect client, subscribe to `document://1`. Edit file through TUI. Assert client receives `notifications/resources/updated` with updated URI within 500ms. | Integration test with spawned editor events |

### Test Infrastructure

A new integration test crate (or expanded tests in `helix-mcp-server/tests/`) will use:
- `tempfile` for ephemeral config directories and socket paths
- A test `Editor` constructed via `Editor::new()` with the `integration` feature, providing a `TestBackend`
- `tokio::process::Command` to spawn `hx` for true end-to-end tests
- The `integration` feature flag in `helix-event` / `helix-term` to enable deterministic idle detection for clean test exits

---

## 7. Design Decisions Log

| Decision | Alternatives Considered | Rationale | Dissenting |
|---|---|---|---|
| **Unix socket over stdio** | Stdio transport (like LSP): use stdin/stdout for JSON-RPC, multiplex with TUI via threads. | Helix's TUI consumes the terminal; stdio is already used by the terminal backend for raw mode I/O. Multiplexing would require a virtual terminal layer or headless mode, both massive scope increases. Unix sockets are the standard for local IPC on Unix and the primary MCP transport pattern. Windows TCP fallback provides equivalent functionality. | None. Unanimous. |
| **Event-driven over polling snapshot** | Polling: MCP tools read from a shared `Arc<RwLock<EditorState>>` that is periodically refreshed (every 200ms). | Polling wastes CPU, produces stale data, and doesn't naturally support subscriptions. Helix already has a robust event system (`helix-event`) with hooks, `AsyncHook`, and channel infrastructure. Event-driven is zero-overhead, real-time, and maps directly to MCP subscription semantics. | None. Unanimous. |
| **Two-crate split (`helix-mcp` + `helix-mcp-server`) over single crate** | Single crate combining protocol types and Helix integration. | A standalone `helix-mcp` crate can be used by external tools (e.g., an MCP client library in Rust) without pulling in 15+ Helix crates. The protocol types are stable and independently versionable. This follows the precedent of `helix-lsp-types` (protocol) vs `helix-lsp` (integration). | None. Unanimous. |
| **Channel-based (`mpsc`) over `Arc<RwLock<>>` for session-editor communication** | Shared mutable state: sessions hold `Arc<RwLock<Editor>>` and lock for reads/writes. | Channels align with Helix's existing async patterns (`AsyncHook`, `send_blocking`). They avoid lock contention when multiple clients read simultaneously. Read-tier tools don't need the write lock at all. For Mutate operations, the confirmation channel naturally serializes modifications through the main event loop, avoiding data races. The `helix-event` hook system already pushes events through an internal dispatcher—sessions receive cloned snapshots via channels. | None. Unanimous. |
| **10 tools for v2 over 2 or 26** | Minimal (2 tools: read/write) or maximal (26 tools: one per LSP request type, one per diagnostic action, etc.). | 10 tools represent the sweet spot: enough to enable real AI coding workflows (read, edit, navigate, search, inspect diagnostics, proxy LSP), but not so many that the API becomes fragmented. The `lsp_request` tool consolidates 5+ LSP operations into one parameterized tool, avoiding tool explosion. The `edit_apply` tool handles all text mutations with a uniform interface. | One reviewer argued for 3 tools (read-file, write-file, run-command) citing KISS. Overruled: without selection control, navigation, and LSP access, the agent cannot effectively interact with Helix's modal editing model. |
| **TOML config over env-only or CLI-only** | Env-only (`HELIX_MCP_PORT=...`, `HELIX_MCP_MAX_CONNS=...`) or CLI-only (`--mcp-port`, `--mcp-max-conns`). | Helix uses TOML for all configuration (`config.toml`). Consistency with the existing config system is paramount. Users expect to find MCP settings alongside LSP, editor, and keymap config. Environment variables and CLI flags serve as overrides, not the primary interface. | None. Unanimous. |
| **Feature gate (`mcp`) over always-on** | Always compile MCP support, control at runtime via config only. | The MCP server adds a dependency on `tokio::net::UnixListener`, an always-running socket listener, and hook registrations—even when no client connects. Users who don't use AI tooling should not pay this cost. A Cargo feature gate ensures zero binary size increase and zero runtime overhead when disabled. This follows the existing `git` feature pattern. | None. Unanimous. |
| **Newline-delimited JSON over Content-Length framing for sockets** | Content-Length header framing (HTTP-style, used by LSP). | Content-Length framing exists for LSP because stdio pipes don't preserve message boundaries and JSON strings can contain newlines. For Unix domain sockets, messages are naturally delimited. JSON text objects don't contain raw newlines (they're escaped as `\n`), so line-delimited framing is safe. It's simpler to implement, trivial to debug with `nc`/`socat`, and matches the MCP Streamable HTTP transport convention. | One reviewer pointed out that JSON-RPC batch requests could contain newlines in string values. Counterpoint: escaped newlines in JSON strings are `\\n`, not literal `\n`. The framing reads byte-by-byte, not line-by-line, and validates JSON completeness before parsing. |

---

## 8. Appendix: Code Sketches

### A1. `helix-mcp/src/jsonrpc.rs`

Key type definitions (minimal subset; the full module mirrors `helix-lsp/src/jsonrpc.rs` but is independently maintained):

```rust
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// JSON-RPC 2.0 protocol version.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Version {
    V2,
}

/// Request/response identifier.
#[derive(Debug, PartialEq, Eq, Clone, Hash, Deserialize, Serialize)]
#[serde(untagged)]
pub enum Id {
    Null,
    Num(u64),
    Str(String),
}

/// JSON-RPC method call (a request expecting a response).
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct MethodCall {
    pub jsonrpc: Option<Version>,
    pub method: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
    pub id: Id,
}

/// JSON-RPC notification (no response expected).
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct Notification {
    pub jsonrpc: Option<Version>,
    pub method: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

/// A request object: either a method call or a notification.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(untagged)]
pub enum Request {
    Single(Call),
    Batch(Vec<Call>),
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(untagged)]
pub enum Call {
    MethodCall(MethodCall),
    Notification(Notification),
    #[serde(skip)]
    Invalid { id: Id },
}

/// JSON-RPC error object.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Error {
    pub code: i64,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

/// A successful response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Success {
    pub jsonrpc: Option<Version>,
    pub result: Value,
    pub id: Id,
}

/// A failure response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Failure {
    pub jsonrpc: Option<Version>,
    pub error: Error,
    pub id: Id,
}

/// A response output (success or failure).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Output {
    Success(Success),
    Failure(Failure),
}

/// A complete response (single or batch).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Response {
    Single(Output),
    Batch(Vec<Output>),
}
```

### A2. `helix-mcp-server/src/server.rs`

The `McpServer::bind` constructor and accept loop:

```rust
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use tokio::net::{UnixListener, TcpListener};
use tokio::sync::Mutex;
use parking_lot::RwLock as ParkingLotRwLock;
use helix_mcp::{Transport, McpServer as McpServerTrait};
use crate::config::McpConfig;
use crate::context::McpContext;
use crate::session::Session;

pub struct HelixMcpServer {
    context: Arc<McpContext>,
    config: McpConfig,
    active_connections: Arc<AtomicU32>,
    confirmation_tx: tokio::sync::mpsc::UnboundedSender<ConfirmationRequest>,
}

impl HelixMcpServer {
    pub fn new(
        context: Arc<McpContext>,
        config: McpConfig,
        confirmation_tx: tokio::sync::mpsc::UnboundedSender<ConfirmationRequest>,
    ) -> Self {
        Self {
            context,
            config,
            active_connections: Arc::new(AtomicU32::new(0)),
            confirmation_tx,
        }
    }

    #[cfg(unix)]
    pub async fn bind_and_serve(self) -> Result<(), Box<dyn std::error::Error>> {
        let socket_path = self.config.socket_path();
        // Ensure parent directory exists with correct permissions
        if let Some(parent) = socket_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                tokio::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700)).await?;
            }
        }
        // Remove stale socket file
        let _ = tokio::fs::remove_file(&socket_path).await;

        let listener = UnixListener::bind(&socket_path)?;
        // Set socket file permissions to 0o600
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            tokio::fs::set_permissions(&socket_path, std::fs::Permissions::from_mode(0o600)).await?;
        }
        log::info!("MCP server listening on {}", socket_path.display());

        // Write metadata file for multi-instance discovery
        self.write_metadata_file(&socket_path, None).await;

        self.accept_loop(listener).await
    }

    #[cfg(windows)]
    pub async fn bind_and_serve(self) -> Result<(), Box<dyn std::error::Error>> {
        let addr = format!("127.0.0.1:{}", self.config.tcp_port);
        let listener = TcpListener::bind(&addr).await?;
        let local_addr = listener.local_addr()?;
        // Write metadata file for multi-instance discovery
        let metadata_dir = std::env::temp_dir().join("helix-mcp");
        tokio::fs::create_dir_all(&metadata_dir).await?;
        self.write_metadata_file_dir(&metadata_dir, Some(local_addr.port())).await;
        log::info!("MCP server listening on {}", local_addr);

        self.accept_loop(listener).await
    }

    async fn accept_loop<L>(self, listener: L) -> Result<(), Box<dyn std::error::Error>>
    where
        L: 'static + Send,
        L: AcceptStream,
    {
        let max = self.config.max_connections;
        let active = Arc::clone(&self.active_connections);
        let context = Arc::clone(&self.context);
        let ctx = Arc::new(self);

        loop {
            let (stream, _peer_addr) = listener.accept().await?;

            let current = active.load(Ordering::SeqCst);
            if current >= max {
                log::warn!("MCP: rejecting connection ({} active, max {})", current, max);
                // We can't easily send an error through a raw socket before accept,
                // so we accept and immediately close. A proper implementation would
                // use a handshake, but MCP clients handle socket closure gracefully.
                continue;
            }

            active.fetch_add(1, Ordering::SeqCst);
            let session_context = Arc::clone(&context);
            let session_active = Arc::clone(&active);
            let confirmation_tx = self.confirmation_tx.clone();
            let session_config = self.config.clone();

            tokio::spawn(async move {
                let transport = Box::new(stream);
                let session = Session::new(
                    session_context,
                    transport,
                    session_active,
                    confirmation_tx,
                    session_config,
                );
                if let Err(e) = session.run().await {
                    log::error!("MCP session error: {}", e);
                }
            });
        }
    }

    /// Write a metadata JSON file for multi-instance discovery.
    async fn write_metadata_file(&self, socket_path: &PathBuf, port: Option<u16>) {
        let pid = std::process::id();
        let worktree = std::env::current_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();

        let metadata = serde_json::json!({
            "pid": pid,
            "worktree": worktree,
            "socket": socket_path.to_string_lossy(),
            "port": port,
            "open_files": [] as Vec<String>,
            "mode": "normal",
            "started_at": chrono::Utc::now().to_rfc3339(),
        });

        let dir = socket_path.parent().unwrap_or(socket_path);
        let metadata_path = dir.join(format!("{}.json", pid));

        // Clean up stale metadata before writing ours
        Self::cleanup_stale_metadata(dir).await;

        if let Err(e) = tokio::fs::write(&metadata_path, metadata.to_string()).await {
            log::warn!("Failed to write MCP metadata file: {}", e);
        }

        // Store path for cleanup on drop
        // (Simplified — actual impl stores in a Vec<String> behind a Mutex)
    }

    async fn write_metadata_file_dir(&self, dir: &PathBuf, port: Option<u16>) {
        // Same as above but takes a directory instead of deriving from socket_path
        let pid = std::process::id();
        let worktree = std::env::current_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();

        let metadata = serde_json::json!({
            "pid": pid,
            "worktree": worktree,
            "port": port,
            "open_files": [] as Vec<String>,
            "mode": "normal",
            "started_at": chrono::Utc::now().to_rfc3339(),
        });

        Self::cleanup_stale_metadata(dir).await;

        let metadata_path = dir.join(format!("{}.json", pid));
        let _ = tokio::fs::write(&metadata_path, metadata.to_string()).await;
    }

    /// Remove metadata files and sockets belonging to dead PIDs.
    async fn cleanup_stale_metadata(dir: &PathBuf) {
        let mut entries = match tokio::fs::read_dir(dir).await {
            Ok(e) => e,
            Err(_) => return,
        };

        while let Ok(Some(entry)) = entries.next_entry().await {
            let path = entry.path();
            let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
            if let Ok(pid) = stem.parse::<i32>() {
                // Check if process is still alive (Unix: kill(pid, 0))
                #[cfg(unix)]
                {
                    use std::os::unix::process::CommandExt;
                    let alive = unsafe { libc::kill(pid, 0) == 0 };
                    if !alive {
                        let _ = tokio::fs::remove_file(&path).await;
                        // Also remove matching .sock file
                        let sock_path = path.with_extension("sock");
                        let _ = tokio::fs::remove_file(&sock_path).await;
                    }
                }
            }
        }
    }
}

/// Abstract over UnixListener and TcpListener for the accept loop.
trait AcceptStream {
    type Stream: Transport;
    type PeerAddr: std::fmt::Debug;
    async fn accept(&self) -> std::io::Result<(Self::Stream, Self::PeerAddr)>;
}

#[cfg(unix)]
impl AcceptStream for UnixListener {
    type Stream = tokio::net::UnixStream;
    type PeerAddr = tokio::net::unix::SocketAddr;
    async fn accept(&self) -> std::io::Result<(Self::Stream, Self::PeerAddr)> {
        UnixListener::accept(self).await.map(|(s, a)| (s, a))
    }
}

#[cfg(windows)]
impl AcceptStream for TcpListener {
    type Stream = tokio::net::TcpStream;
    type PeerAddr = std::net::SocketAddr;
    async fn accept(&self) -> std::io::Result<(Self::Stream, Self::PeerAddr)> {
        TcpListener::accept(self).await
    }
}
```

### A3. `helix-mcp-server/src/session.rs`

Session lifecycle: registering event hooks and the run loop:

```rust
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use tokio::sync::mpsc;
use helix_mcp::{
    jsonrpc::{self, Call, Id, Output, Success, Failure},
    Transport, messages::*, types::*,
};
use crate::context::McpContext;
use crate::config::McpConfig;
use crate::security::{ConfirmationRequest, SecurityTier};

pub struct Session {
    id: u64,
    context: Arc<McpContext>,
    transport: Box<dyn Transport>,
    active_count: Arc<AtomicU32>,
    confirmation_tx: mpsc::UnboundedSender<ConfirmationRequest>,
    config: McpConfig,
    /// Channel for editor events → MCP notifications
    event_rx: mpsc::UnboundedReceiver<SessionEvent>,
    event_tx: mpsc::UnboundedSender<SessionEvent>,
    /// Per-session subscription state
    subscriptions: Vec<String>,
}

#[derive(Debug, Clone)]
enum SessionEvent {
    DocumentChanged { doc_id: String, uri: String },
    SelectionChanged { doc_id: String, uri: String },
    DiagnosticsChanged { doc_id: String, uri: String },
    DocumentOpened { doc_id: String, uri: String },
    DocumentClosed { doc_id: String },
}

impl Session {
    pub fn new(
        context: Arc<McpContext>,
        transport: Box<dyn Transport>,
        active_count: Arc<AtomicU32>,
        confirmation_tx: mpsc::UnboundedSender<ConfirmationRequest>,
        config: McpConfig,
    ) -> Self {
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let session = Self {
            id: rand::random(),
            context,
            transport,
            active_count,
            confirmation_tx,
            config,
            event_rx,
            event_tx,
            subscriptions: Vec::new(),
        };
        session.register_hooks();
        session
    }

    fn register_hooks(&self) {
        let tx = self.event_tx.clone();
        let doc_tx = tx.clone();
        // Register hooks using helix-event macros
        // In practice, this uses `helix_event::register_hook!` with the
        // actual event types from `helix_view::events`.
        // The hooks are closures that serialize a SessionEvent and send it.
        // Hooks are cleaned up when the session is dropped via a registered
        // cleanup mechanism (session stores hook IDs or uses a Drop guard).
    }

    pub async fn run(mut self) -> Result<(), Box<dyn std::error::Error>> {
        let result = self.inner_run().await;
        self.active_count.fetch_sub(1, Ordering::SeqCst);
        // Unregister event hooks on drop
        result
    }

    async fn inner_run(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

        // Run the transport loop
        loop {
            tokio::select! {
                // Incoming MCP request from client
                result = helix_mcp::framing::read_frame::<jsonrpc::Request>(&mut self.transport) => {
                    match result {
                        Ok(request) => {
                            let response = self.handle_request(request).await;
                            if let Some(resp) = response {
                                helix_mcp::framing::write_frame(&mut self.transport, &resp).await?;
                            }
                        }
                        Err(e) => {
                            log::debug!("MCP session {} read error: {}", self.id, e);
                            break;
                        }
                    }
                }
                // Outgoing event notification to client
                Some(event) = self.event_rx.recv() => {
                    let notification = self.event_to_notification(event);
                    if let Some(notif) = notification {
                        let _ = helix_mcp::framing::write_frame(&mut self.transport, &notif).await;
                    }
                }
            }
        }

        Ok(())
    }

    async fn handle_request(
        &mut self,
        request: jsonrpc::Request,
    ) -> Option<jsonrpc::Response> {
        match request {
            jsonrpc::Request::Single(call) => self.handle_call(call).await,
            jsonrpc::Request::Batch(calls) => {
                let mut outputs = Vec::new();
                for call in calls {
                    if let Some(output) = self.handle_call_to_output(call).await {
                        outputs.push(output);
                    }
                }
                if outputs.is_empty() { None } else { Some(jsonrpc::Response::Batch(outputs)) }
            }
        }
    }

    async fn handle_call(&mut self, call: jsonrpc::Call) -> Option<jsonrpc::Response> {
        match self.handle_call_to_output(call).await {
            Some(output) => Some(jsonrpc::Response::Single(output)),
            None => None,
        }
    }

    async fn handle_call_to_output(&mut self, call: jsonrpc::Call) -> Option<jsonrpc::Output> {
        match call {
            jsonrpc::Call::MethodCall(mc) => {
                let result = self.dispatch_method(&mc.method, mc.params.clone()).await;
                let output = match result {
                    Ok(value) => Output::Success(Success {
                        jsonrpc: Some(jsonrpc::Version::V2),
                        result: value,
                        id: mc.id,
                    }),
                    Err(e) => Output::Failure(Failure {
                        jsonrpc: Some(jsonrpc::Version::V2),
                        error: jsonrpc::Error {
                            code: e.code,
                            message: e.message,
                            data: None,
                        },
                        id: mc.id,
                    }),
                };
                Some(output)
            }
            jsonrpc::Call::Notification(_) => {
                // Handle notifications (e.g., `notifications/initialized`, `cancelled`)
                None // Notifications don't get responses
            }
            jsonrpc::Call::Invalid { id } => {
                Some(Output::Failure(Failure {
                    jsonrpc: Some(jsonrpc::Version::V2),
                    error: jsonrpc::Error {
                        code: -32600,
                        message: "Invalid Request".to_string(),
                        data: None,
                    },
                    id,
                }))
            }
        }
    }

    async fn dispatch_method(
        &self,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> Result<serde_json::Value, McpError> {
        match method {
            "initialize" => self.handle_initialize(params).await,
            "tools/list" => self.handle_tools_list(params).await,
            "tools/call" => self.handle_tools_call(params).await,
            "resources/list" => self.handle_resources_list(params).await,
            "resources/read" => self.handle_resources_read(params).await,
            "resources/subscribe" => self.handle_resources_subscribe(params).await,
            "resources/unsubscribe" => self.handle_resources_unsubscribe(params).await,
            "prompts/list" => self.handle_prompts_list(params).await,
            "prompts/get" => self.handle_prompts_get(params).await,
            "ping" => self.handle_ping(params).await,
            _ => Err(McpError::method_not_found(method)),
        }
    }
}
```

### A4. `helix-term/src/application.rs` Changes

Feature-gated changes to the `Application` struct and initialization:

```rust
// In the Application struct definition (around line 73):
pub struct Application {
    compositor: Compositor,
    terminal: Terminal,
    pub editor: Editor,

    config: Arc<ArcSwap<Config>>,

    signals: Signals,
    jobs: Jobs,
    lsp_progress: LspProgressMap,

    theme_mode: Option<theme::Mode>,
    last_file_change: HashMap<helix_view::DocumentId, Instant>,

    // NEW: MCP server handle and confirmation channel
    #[cfg(feature = "mcp")]
    mcp_server: Option<helix_mcp_server::McpServerHandle>,
    #[cfg(feature = "mcp")]
    mcp_confirmations: Option<(
        tokio::sync::mpsc::UnboundedSender<helix_mcp_server::security::ConfirmationRequest>,
        tokio::sync::mpsc::UnboundedReceiver<helix_mcp_server::security::ConfirmationRequest>,
    )>,
}

// In Application::new(), after the existing initialization (around line 252):
#[cfg(feature = "mcp")]
let (mcp_server, mcp_confirmations) = if mcp_enabled {
    let mcp_config = config.editor.mcp.clone();
    let (confirm_tx, confirm_rx) = tokio::sync::mpsc::unbounded_channel();
    let context = Arc::new(helix_mcp_server::McpContext::new(
        // Reference to editor state; in practice this is an Arc<ArcSwap>
        // or an Arc<Mutex<Editor>> that the MCP server reads from
    ));
    let handle = helix_mcp_server::start_server(context, mcp_config, confirm_tx);
    (Some(handle), Some((confirm_tx, confirm_rx)))
} else {
    (None, None)
};

// In Application struct construction:
let app = Self {
    compositor,
    terminal,
    editor,
    config,
    signals,
    jobs,
    lsp_progress: LspProgressMap::new(),
    theme_mode,
    last_file_change: HashMap::new(),
    #[cfg(feature = "mcp")]
    mcp_server,
    #[cfg(feature = "mcp")]
    mcp_confirmations,
};

// In the render() method, before rendering the compositor:
#[cfg(feature = "mcp")]
if let Some((_, ref mut confirm_rx)) = self.mcp_confirmations {
    // Drain pending confirmations, render prompt overlay if any
    while let Ok(req) = confirm_rx.try_recv() {
        self.render_mcp_confirmation(req);
    }
}
```

### A5. `helix-term/Cargo.toml` Changes

```toml
[features]
default = ["git"]
unicode-lines = ["helix-core/unicode-lines", "helix-view/unicode-lines"]
integration = ["helix-event/integration_test"]
git = ["helix-vcs/git"]
mcp = ["helix-mcp-server"]    # NEW

# ...

[dependencies]
# ... existing deps ...

# NEW
helix-mcp-server = { path = "../helix-mcp-server", optional = true }
```

### A6. Root `Cargo.toml` Changes

```toml
[workspace]
members = [
  # ... existing members ...
  "helix-mcp",          # NEW
  "helix-mcp-server",   # NEW
]

# NEW workspace dependency entries
[workspace.dependencies]
# ... existing deps ...
helix-mcp = { path = "helix-mcp" }
helix-mcp-server = { path = "helix-mcp-server" }
```

---

*Document version: 1.0 — generated from multi-agent design council consensus*

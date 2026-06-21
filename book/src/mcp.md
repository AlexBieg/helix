# Model Context Protocol (MCP)

Helix includes an MCP server that lets AI tools and assistants interact with
your editor session. MCP is an open protocol standardized by Anthropic for
context sharing between applications and language models.

## Why MCP in Helix?

With the Helix MCP server, AI tools can:

- **Read open files** — access document contents, selections, and diagnostics
- **Search your workspace** — run text searches across open buffers
- **Apply edits** — make changes with user confirmation (never automatic)
- **Get prompts** — retrieve context-aware prompt templates

This makes external AI assistants much more effective because they can see
exactly what you're working on without you needing to copy-paste context.

## Enabling the MCP Server

### Via config file

Add to your `config.toml`:

```toml
[editor.mcp]
enable = true
```

### Via command-line flag

```sh
hx --mcp
```

Disable with `--no-mcp` (overrides the config setting):

```sh
hx --no-mcp
```

### Via environment variable

```sh
HELIX_MCP=1 hx
```

Set `HELIX_MCP=0` to explicitly disable.

## Configuration

All options live under `[editor.mcp]` in `config.toml`:

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `enable` | `bool` | `false` | Enable the MCP server |
| `max-connections` | `u32` | `4` | Maximum simultaneous client connections |
| `socket` | `path` | auto | Custom Unix socket path |
| `tcp-port` | `u16` | `0` | TCP port for Windows (0 = auto) |
| `rate-limit` | `u64` | `100` | Max requests/second per client |

Example:

```toml
[editor.mcp]
enable = true
max-connections = 2
socket = "/tmp/my-helix-mcp.sock"
rate-limit = 50
```

## Connecting AI Tools

### Unix (macOS/Linux)

The server listens on a Unix domain socket. Find the socket path:

```sh
hx --mcp-info
```

Output:
```json
{
  "pid": 12345,
  "socket": "/run/user/1000/helix/mcp/12345.sock",
  "worktree": "/home/user/my-project",
  "connections": 1
}
```

Connect any MCP-compatible client to that socket:

```sh
# Example with a generic JSON-RPC client
echo '{"jsonrpc":"2.0","method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{}},"id":1}' \
  | nc -U /run/user/1000/helix/mcp/12345.sock
```

List all running Helix instances with MCP:

```sh
hx --mcp-list
```

### Windows

On Windows, the server uses TCP (127.0.0.1). A metadata file at
`%TEMP%\helix-mcp\{pid}.json` contains the port number and worktree
information. Use `--mcp-list` to discover instances.

## Headless Mode

Helix supports running as a headless MCP server without the TUI. This lets
AI tools use Helix as a pure backend for document management, diagnostics,
and editing operations without a visible editor window.

### Starting Headless Mode

```sh
hx --headless --mcp file.rs another.rs
```

This starts Helix with:
- **No terminal UI** — no compositor, no rendering
- **MCP server only** — full tool, resource, and prompt capabilities
- **File arguments opened** — each file listed is available via MCP tools

The process runs until it receives SIGTERM or SIGINT (Ctrl+C).

### Use Cases

- **CI/CD pipelines**: AI agents that need to analyze or modify code
- **Background assistants**: Always-on editor context for IDE integrations
- **Remote editing**: Connect MCP clients over SSH port forwarding

### Socket Discovery

The socket path is the same as in normal mode. Use `--mcp-info` to print
the socket path for headless instances:

```sh
hx --mcp-info
```

Or list all running instances:

```sh
hx --mcp-list
```

### Confirmation in Headless Mode

In headless mode, Mutate-tier operations (document_write, edit_apply,
diagnostics_publish) are **auto-accepted** since there is no TUI to present
a confirmation dialog. This makes headless mode suitable for automation.
Use appropriate security measures (e.g. file permission controls on the
socket) when running headless in untrusted environments.

## Available Capabilities

### Tools

| Tool | Tier | Description |
|------|------|-------------|
| `document_read` | Read | Read a document's full text |
| `selection_read` | Read | Read current selections |
| `search_text` | Read | Search across open files |
| `diagnostics_read` | Read | List diagnostics for a file |
| `lsp_request` | Read | Send an LSP request |
| `workspace_info` | Read | Get workspace metadata |
| `goto_position` | Preview | Navigate to a position |
| `selection_set` | Preview | Preview a new selection |
| `document_write` | Mutate | Write/replace document content |
| `edit_apply` | Mutate | Apply a structured edit |
| `diagnostics_publish` | Mutate | Publish agent-provided diagnostics |

### Resources

Resource URIs follow the pattern `document:///<absolute-path>`. Use
`resources/list` to enumerate open documents and `resources/read` to
retrieve content. Subscribe to `resources/subscribe` for live updates.

### Prompts

Built-in prompts provide context templates for AI assistants:

- `codebase_overview` — Summary of open files and project structure
- `current_context` — Active file, selection, and diagnostics

## Security Model

Helix uses a **three-tier security model** for MCP operations:

| Tier | Description | Confirmation |
|------|-------------|-------------|
| **Read** | Read-only access to editor state | None |
| **Preview** | Non-destructive preview of changes | None |
| **Mutate** | Destructive changes to editor state | Required |

Mutate-tier operations (like `document_write` and `edit_apply`) require
explicit user confirmation before execution. AI tools **cannot** modify
your files without your consent.

## Troubleshooting

### "Connection refused"

- Ensure `enable = true` in config or `--mcp` flag is set
- Check the socket path with `--mcp-info`
- Verify the Helix process is still running

### "Permission denied" on socket

The socket is created with default permissions. If connecting from a
different user, use a custom path:

```toml
[editor.mcp]
socket = "/tmp/helix-shared.sock"
```

### Rate limiting errors

If a client sends requests faster than the `rate-limit`, requests will be
delayed (not rejected). Increase the limit if needed:

```toml
[editor.mcp]
rate-limit = 200
```

### "Server already initialized"

Each MCP connection must call `initialize` exactly once. Re-initialization
is rejected per the protocol spec.

## Architecture

For details on the implementation, see the [MCP workplan](../docs/mcp-workplan.md)
and [design document](../docs/mcp-design.md).

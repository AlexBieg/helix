# Future Features

Features planned but not yet implemented.

## Session Persistence

Save and restore editor state across restarts.

### Scope
- Open buffers/documents and their order
- Cursor position (line + column) per buffer
- Selections (including multiple cursors)
- Scroll position per buffer
- Active buffer (which one has focus)
- Window splits / layout tree
- Recent files list stored in session

### Behavior
- Save on buffer open/close, editor exit, periodically (configurable interval), and manually via `:save-session`
- Restore automatically when opening a directory (`hx .`) or launching in a workspace
- Gracefully skip missing files; fall back to scratch buffer if all files gone
- Validate scroll positions against current file content
- Dirty buffers on exit: discard unsaved changes, reload from disk
- On by default, configurable via `[editor.session]`

### Storage
- `~/.config/helix/sessions/<hash>.json` — one file per workspace root (git repo or CWD)
- Session files are JSON for readability

### Prior attempt notes
- Adding a dedicated `session_timer` to `wait_event()`'s `tokio::select!` broke the event loop in integration tests
- Alternative: debounce saves on the existing `IdleTimer` event instead of a separate timer
- Path canonicalization needed for consistent hashing across relative vs absolute path forms
- Started in `helix-view/src/session.rs` (reverted), integration test in `helix-term/tests/test/session.rs` (reverted)

## Minimap
- Maybe, the scroll bard did a lot of that work for me

## Filter workspace search with file regex

## File diffing

## Merge conflict resolution



# Personal Feature Changelog

Custom features added on top of upstream Helix.

## 2025-06-09

### Automatic file reloading on external changes
- Added `FileWatcher` component using `notify` crate for cross-platform filesystem event monitoring
- Added `FileWatcherConfig` (`auto_reload`, `debounce_ms`) to editor config
- Wired into `Editor` open/close lifecycle and `wait_event()` event loop
- Auto-reloads clean buffers when files are modified externally
- Shows status warning for dirty buffers instead of overwriting unsaved changes
- Debounce support to prevent rapid successive reloads
- Notifies LSP via `file_event_handler` on reload
- Config:
  ```toml
  [editor.file-watcher]
  auto-reload = true   # default: true
  debounce-ms = 100    # default: 100
  ```

### Click buffer names in bufferline to switch buffers
- Tracks per-buffer x-coordinate ranges during bufferline rendering
- Left-click on a buffer name in the bufferline switches to that buffer
- Uses existing `editor.switch()` with `Action::Replace`

### Recent file picker
- Tracks all opened files in MRU order via `Editor::recent_files`
- Files remain in the list even after closing the buffer
- Deduplicates and caps at 100 entries
- Accessible via `space R` or `:recent_file_picker`
- Includes file preview panel on selection

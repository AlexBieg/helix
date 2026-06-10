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
- Watches `.git/HEAD` to refresh gutter diff after git commits
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

## 2025-06-10

### File preview below file list in all pickers
- Changed layout of the generic `Picker` component in `helix-term/src/ui/picker.rs`
- Preview panel now renders below the file list instead of to the right (vertical split)
- Affects all pickers: file picker, file explorer, buffer picker, symbol picker, global search, diagnostics picker, recent file picker, changed file picker, and DAP pickers
- Added `MIN_AREA_HEIGHT_FOR_PREVIEW` constant (20 rows) to prevent showing preview when the area is too short

### Scrollable file previews in all pickers
- Added keyboard-driven scrolling for the preview panel in all pickers
- `Alt-Up` / `Alt-Down`: scroll preview one line
- `Alt-PageUp` / `Alt-PageDown`: scroll preview one page
- `Alt-Home`: reset scroll to top of preview
- `Alt-End`: scroll to bottom of preview
- Mouse wheel scrolling in the preview area (3 lines per tick)
- Scroll position resets automatically when the selected item changes
- Implemented in the generic `Picker` component via `preview_scroll` offset tracking

### Editor scrollbar
- Renders a vertical scrollbar in the rightmost column of a view when the buffer is taller than the viewport
- Thumb height and position reflect the buffer length and current scroll offset
- Styled via the new `ui.buffer.scroll` theme key (`fg` sets thumb color, `bg` sets track color), falling back to `ui.menu.scroll` when unset
- Added `ui.buffer.scroll` to the default `theme.toml` and documented it in `book/src/themes.md`
- Implemented in `render_view` in `helix-term/src/ui/editor.rs`

### Diagnostic markers in the scrollbar track
- Draws diagnostic markers (`▂`) in the column just left of the scrollbar thumb, positioned by each diagnostic's line relative to the buffer length
- Color reflects severity: `error`, `warning`, `info`, and `hint` theme keys (unknown severity is treated as a warning)
- Gives an at-a-glance overview of where problems sit throughout the whole file
- Thumb is drawn on top of the markers so the current viewport position stays visible
- Implemented in `render_view` in `helix-term/src/ui/editor.rs`

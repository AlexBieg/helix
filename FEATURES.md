# Personal Feature Changelog

Custom features added on top of upstream Helix.

## 2025-06-09

### Automatic file reloading on external changes
- Added `FileWatcher` component using `notify` crate for cross-platform filesystem event monitoring
- Added `FileWatcherConfig` (`auto_reload`, `debounce_ms`) to editor config
- Wired into `Editor` open/close lifecycle and `wait_event()` event loop
- Auto-reloads clean buffers when files are modified externally
- Only reacts to *external* changes: filesystem events caused by Helix's own save are ignored by comparing the file's on-disk mtime against the time of Helix's last write, so saving in the editor no longer shows a spurious "changed externally, reloaded" message (added 2026-06-12)
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

### Git diff preview in changed file picker
- Selecting a modified or conflicted file in the changed file picker (`Space-g`) now shows a unified diff between git HEAD and working tree
- Untracked and renamed files show their regular contents; deleted files show no preview
- Added `unified_diff()` to `helix-core::diff` for computing diff text from two ropes
- Added `with_content_preview()` builder to the generic `Picker` component for custom preview content
- Language detection uses the original filename, so code lines within the diff get syntax highlighting via tree-sitter error recovery; unknown extensions fall back to the `diff` grammar

### Merge conflict resolution
- Added `conflict_file_picker` — lists all files in merge conflict state via git status, showing an `"x conflict"` label and file preview. On open, the cursor jumps to the first conflict region. Bindable in config (no default key).
- Added conflict navigation — `[m` / `]m` jump to previous/next merge conflict, `[M` / `]M` jump to first/last. Follows the same motion pattern as `[g` / `]g` for diff hunks.
- Added conflict resolution commands:
  - `:keep-ours` (aliases: `:keep-head`, `:accept-ours`, `:ours`) — resolve conflict keeping ours (HEAD) changes
  - `:keep-theirs` (aliases: `:keep-main`, `:accept-theirs`, `:theirs`) — resolve conflict keeping theirs (merged branch) changes
  - `:keep-both` (aliases: `:accept-both`, `:both`) — resolve conflict keeping both changes (ours then theirs)
- Also available as mappable static commands: `resolve_conflict_keep_ours`, `resolve_conflict_keep_theirs`, `resolve_conflict_keep_both`
- Core parsing: `ConflictRegion` struct and `find_conflict_regions()` scan a buffer for `<<<<<<<`/`=======`/`>>>>>>>` markers
- Resolution replaces the entire conflict region (markers and all) with the chosen section via a single `Transaction::change`

## 2026-06-12

### Popup notifications (toasts)
- Status, warning, and error messages now appear as color-coded popups ("toasts") stacked in the top-right corner, in addition to being mirrored on the status line
- Severity-scaled auto-dismiss (hint 2s, info 3s, warning 5s, error sticky by default); identical consecutive messages coalesce with a `(×N)` counter; the stack collapses to a `+N more` indicator past `max-visible`
- Dismiss without leaving your current mode: `Space N` (`dismiss_notifications`) clears the stack, `dismiss_notification` clears the most recent (unbound by default), or click a toast to close it
- `:notify [-s|--severity hint|info|warning|error] [-r|--repeat N] <message>` shows a notification by hand (handy for testing and theming; `\n` in the message wraps to multiple lines)
- Styled via `ui.notification` / `ui.notification.{error,warning,info,hint}`, falling back to `ui.popup` and the existing severity scopes
- Auto-dismiss uses a single scheduled wake (`Editor::request_redraw_at`) rather than busy-polling, so the editor stays idle between toasts
- Config:
  ```toml
  [editor.notifications]
  enable = true
  max-visible = 5
  animate = true
  # 0 = sticky (dismiss manually)
  timeout = { hint = 2000, info = 3000, warning = 5000, error = 0 }
  ```

### UI entrance animations
- Added a small shared `ui::animation` module: eased entrance progress and an RGB color `blend`, driven purely by elapsed time (per-frame redraws only while animating, then idle)
- **Notifications** slide in horizontally and slide out + dim as they leave; dismissing fades a toast out rather than making it vanish
- **Pickers** play a brief entrance animation when they open, configurable via `editor.picker-animation`:
  - `none` — no animation
  - `unfold` — grow from one row down to full height
  - `unfold-horizontal` — grow from the center out to full width
  - `unfold-both` — grow from the center out in both dimensions, a zoom/iris
  - `cascade` — reveal result rows top-to-bottom
  - (default: `none`)
- The picker unfolds and cascade animate geometry, so they are theme-independent; only color-based effects (the notification fade) require RGB theme colors and degrade gracefully on named/indexed palettes
- Config:
  ```toml
  [editor]
  picker-animation = "none"   # none | unfold | unfold-horizontal | unfold-both | cascade
  ```

### Search match counter in the statusline
- New `search-count` statusline element shows the current search match position and total, e.g. `[3/12]`, like vim's search count
- Updates as you navigate matches with `n`/`N`, and live while typing a `/` search; shows nothing when no search is active or a search has no matches
- Computed in `search_impl` (the single routine all search paths flow through) and stored in `Editor::search_match_count`; the position math lives in the pure, unit-tested `search_match_position` helper
- Add it to any statusline section, e.g. `[editor.statusline] center = ["search-count"]`
- Known limitation: the count refreshes on search navigation, not on plain cursor moves or edits, so it can read stale until the next `n`/`N` or search

### Multi-row statusline
- The statusline can now spread across two rows so long elements (e.g. `version-control` and `file-name`) no longer crowd each other or push out other elements
- `[editor.statusline.second-row]` adds a per-view row directly above the main statusline row; costs one row of document height
- Takes the same `left`/`center`/`right` sub-keys as the main row; an omitted/unset row is not rendered (no leftover gap), so single-row setups are unchanged
- Implemented by reserving the row in `render_view` and tracking the reservation via a new `View::statusline_height` (kept in sync in `Editor::resize`) so `inner_area`/`inner_height` shrink the document accordingly; `statusline::render` was refactored into a per-row `render_row` helper
- The `version-control` element can be styled independently via the new `ui.statusline.version-control` theme key (falls back to `ui.statusline` when unset)
- Config:
  ```toml
  [editor.statusline]
  left = ["mode", "spinner", "diagnostics"]
  center = ["search-count"]
  right = ["position-percentage"]

  [editor.statusline.second-row]
  left = ["version-control"]  # branch name
  right = ["file-name"]       # file path gets the rest of the row
  ```

### Adaptive file path shortening
- The `file-name` statusline element now shows the full relative path while it fits, and only when the row runs out of room does it abbreviate leading directories fish-style — topmost component first (`helix-term` → `h`), one at a time, until the path fits (e.g. `h/s/ui/editor.rs`)
- The file name itself is never abbreviated; hidden directories keep their leading dot (`.config` → `.c`)
- Width budget is computed from the full view width minus what the other elements on the row already consume (so on the second row it shrinks to avoid colliding with `version-control`); for the fit to be exact, place `file-name` in the row's `right` zone (center elements are laid out after it)
- Pure logic lives in `fit_path`/`abbreviate_segment` in `helix-term/src/ui/statusline.rs`, covered by unit tests

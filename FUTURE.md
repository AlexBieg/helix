# Future Feature Ideas

Features that don't exist in upstream Helix, brainstormed for potential implementation.

## Smooth scrolling
- Animated viewport scroll with CSS-style easing
- Reuses the existing animation module (`helix-term/src/ui/animation.rs`)
- Configurable duration and easing curve per scroll type (page up/down, half-page, mouse wheel)

## Minimap
- Zoomed-out code overview rendered in a sidebar gutter
- Builds on existing scrollbar + diagnostic marker rendering
- Low-resolution character-scale representation of buffer text
- Viewport indicator overlay showing current scroll position

## Zen mode
- Toggle (`:zen` or keybind) to hide statusline, bufferline, and optionally gutter
- Centers document content horizontally and vertically
- Fade-out/fade-in transition for chrome elements

## Hunk staging
- Stage/unstage individual git hunks directly from the editor gutter
- Keybind on a changed line to `git add -p` / `git reset -p` that hunk
- Visual feedback in gutter showing staged/unstaged state per hunk

## Code folding
- Fold/unfold by tree-sitter syntax nodes or indent level
- `zc` fold, `zo` open, `za` toggle at cursor
- `zM` fold all, `zR` open all
- `z1`–`z9` fold by indent level
- Folded region markers (e.g. `▶` or `…`) in gutter and inline
- LSP folding range types already exist in `helix-lsp-types/src/folding_range.rs`

## Bookmarks
- Named per-file marks (`m[a-z]`) and global marks (`m[A-Z]`)
- Jump to mark with `` `[a-z] `` or `'[A-Z]`
- Persisted across sessions via existing session persistence
- Gutter sign for bookmarked lines

## Breadcrumb bar
- Optional top bar showing tree-sitter context: `module > class > function > closure`
- Updates as cursor moves between symbols
- Clickable segments to jump to parent symbols
- Configurable via `[editor.breadcrumbs]` with `enable` and `max-depth`

## Project-wide search & replace
- Extend `global_search` with a replace mode
- Diff preview per file before applying
- Confirm/reject individual changes in a picker UI
- Undo across all changed files as a single transaction

## Integrated terminal
- Terminal emulator pane in a split (like `:term`)
- Reuses existing split/layout system
- Shell stays alive between focus changes
- Copy/paste between editor and terminal

## Command output buffer
- Capture `:sh` / `:run-shell-command` output into an editable scratch buffer
- Replaces current read-only Markdown popup overlay
- Useful for composing commands from output, or persisting command results

# Inline Markdown Rendering

Renders markdown formatting directly on source text by dimming syntax
markers and styling content using the editor theme.

Config key: `[editor] inline-markdown = true`

## Architecture

```
render_view() in editor.rs
  └─ inline_markdown_overlays(doc.text, theme)
       ├─ pulldown-cmark OffsetIter → (Event, byte_range)
       ├─ byte→char mapping for overlay ranges
       ├─ byte→line mapping for code block backgrounds
       ├─ scope resolution via try_scope() chains
       └─ returns (OverlayHighlights, code_block_line_ranges)
  └─ OverlayHighlights pushed into existing overlay pipeline
  └─ Decoration registered for full-width code block backgrounds
```

## Scope Resolution

All scopes resolved through `try_scope()` which tries candidates in
order: exact match only for all but the last, hierarchical fallback
for the final scope. This prevents unwanted parent-fallback
(e.g. `markup.heading.marker` falling back to `markup.heading`).

### Marker dimming chain

| Element           | Scope chain                                                                      |
|-------------------|----------------------------------------------------------------------------------|
| `#` heading       | `markup.inline.marker` → `markup.heading.marker` → `ui.virtual`                 |
| `**` `*` `` ` `` brackets | `markup.inline.marker` → `punctuation.bracket` → `ui.virtual` → `punctuation.delimiter` |
| `>` blockquote, `---`, HTML | `markup.inline.marker` → `punctuation.special` → `ui.virtual` → `punctuation.delimiter` |

### Content styling chain

| Element           | Scope chain                                      |
|-------------------|--------------------------------------------------|
| Heading text      | `markup.heading.N` → `markup.heading`            |
| Bold              | `markup.bold`                                    |
| Italic            | `markup.italic`                                  |
| Strikethrough     | `markup.strikethrough`                           |
| Inline code       | `markup.raw.inline` → `markup.raw`               |
| Link text         | `markup.link.text` → `markup.link`               |
| Link URL          | `markup.link.url` → `markup.link`                |
| List markers      | `markup.list.unnumbered` → `markup.list`         |
| Blockquote        | `markup.quote`                                   |

## Key Lessons

### pulldown-cmark OffsetIter ranges

`OffsetIter` yields `(Event, Range<usize>)` where ranges are **byte**
positions in the source. Critical behavior:

- **Inline elements (Strong, Emphasis, Strikethrough):** Start and End
  events span the **entire element**, not just the marker.
  `Start(Strong)` for `**bold**` = `0..8`, not `0..2`.
  Fix: use `marker_len_at()` to detect marker byte length from source,
  extract `byte_range.start..start+len` for opening,
  `byte_range.end-len..end` for closing.

- **Code blocks:** Start/End also span the entire block including fences.
  Fix: opening fence = first line of byte_range, closing fence = last line.
  Content = Text event range (already correct).

- **Links:** End range for `Link` spans `](url)`. Split into
  non-overlapping sub-ranges for brackets vs URL to avoid highlight
  conflicts.

### Overlap constraints

`OverlayHighlights::Heterogenous` **requires non-overlapping ranges**.
When multiple highlights cover the same characters within a single
Heterogenous collection, only one is active at any position.
The lower-index highlight wins at its start position, but when it
ends and another starts at the same position, the second overwrites.
Design highlights to be strictly sequential.

### Theme scopes vs find_highlight

`Theme::find_highlight_exact()` only finds scopes registered in
`scope_index` — these come from the **theme file's keys** plus
dynamically-registered tree-sitter highlight scopes. Not all
tree-sitter capture names have theme entries (e.g. `punctuation.bracket`
is a capture but has no default theme style).

`Theme::find_highlight()` does hierarchical fallback by stripping
dot-separated suffixes. This is useful for deep scopes like
`markup.heading.1` → `markup.heading` → `markup`, but dangerous for
scopes where the parent has **opposite semantics**
(e.g. `markup.heading.marker` → `markup.heading` makes dim markers
use heading color).

### Style::patch ordering

Rendering in `document.rs:331-335`:
```
style = syntax_style
  .patch(whitespace_style)  // only if grapheme is whitespace
  .patch(overlay_style)
```

Overlay wins for all style fields (fg, bg, modifiers ORed). This is
fine for most cases but means overlay cannot "remove" a modifier
from the syntax style — modifiers are additive only.

### Decoration system for full-width backgrounds

`OverlayHighlights` apply only behind character ranges. For full-line
backgrounds (code blocks), use the `Decoration` trait with
`decorate_line()` which receives `TextRenderer` and can call
`set_style(Rect{x: 0, y: visual_line, width: viewport.width, height: 1})`
to color the entire line width.

## Files Changed

| File                                      | Change                                      |
|-------------------------------------------|---------------------------------------------|
| `helix-term/src/ui/markdown_inline.rs`    | New: inline markdown parser + overlay gen   |
| `helix-term/src/ui/mod.rs`                | Registered `mod markdown_inline`            |
| `helix-term/src/ui/editor.rs`             | Wired into `render_view()` + decoration     |
| `helix-view/src/editor.rs`                | Added `inline_markdown: bool` config        |

## Known Limitations

- Bold modifier from `markup.bold` not rendered (tree-house quirk)
- Entire document re-parsed every frame (no caching)
- No table or footnote styling
- Reference-style links not fully handled
- Only matches `language_id()` for `markdown`/`md`, not `mdx` variants

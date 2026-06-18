# Bufferline: follow-active + overflow indicators + filename disambiguation

Spec for changes to `EditorView::render_bufferline`
(`helix-term/src/ui/editor.rs:752`).

## Problem

The bufferline draws open buffers in `DocumentId` order, left→right, and `break`s
when it hits the right edge (`editor.rs:828`). Consequences:

- With many buffers, the active buffer (and its next/prev neighbors) can be
  scrolled off the right edge and become invisible. Navigation order
  (`goto_next/previous_buffer`) follows the same `DocumentId` order, so losing
  sight of the active buffer also means losing sight of "what's next".
- Labels are bare `file_name()` (`editor.rs:782-788`), so `mod.rs`, `index.ts`,
  `__init__.py` etc. are indistinguishable when several are open.

This spec covers three changes, scoped to the bufferline only:

- **A. Follow-active scrolling** with a neighbor margin.
- **B. Overflow indicators** at each edge.
- **D. Filename disambiguation** on collision.

Buffer order stays `DocumentId` order (stable; matches nav order). MRU reordering
is explicitly out of scope.

## Current behavior (facts)

- `render_bufferline(editor, viewport, surface, ranges)` is a static method;
  it owns no persistent state. `ranges: &mut Vec<(DocumentId, u16, u16)>` is
  populated each frame and reused for mouse-click hit-testing
  (`editor.rs:1382-1391`).
- Single pass: for each doc it formats `" {fname}{[+]?} "`, writes it with
  `surface.set_stringn(...)` (or a per-char gradient loop when the doc is active
  and `gradient_borders.enable`), pushes `(id, start_x, x)`, and breaks once
  `x >= surface.area.right()`.
- `render_bufferline` is called with `area.with_height(1)` as `viewport`
  (`editor.rs:1790-1795`); clipping currently tests `surface.area.right()`
  rather than `viewport.right()`.
- State lives on `EditorView`: `bufferline_ranges` already persists across frames
  (`editor.rs:55`). New scroll state goes here too.

## Requirements

- **R1** — The active buffer is always fully visible in the bufferline.
- **R2** — When space allows, at least `MARGIN` buffers on each side of the
  active buffer are also visible (so next/prev are seen). `MARGIN = 1` initially.
- **R3** — Scrolling is *stable*: the offset only changes when the active buffer
  (with its margin) would otherwise fall outside the visible window. Switching
  among already-visible buffers does not move the strip.
- **R4** — When buffers are hidden off the left, a left indicator is shown; same
  for the right. Indicators reflect actual hidden state on each side
  independently.
- **R5** — When two or more *open* buffers share a `file_name()`, their labels
  are extended with the minimal number of trailing path components needed to
  distinguish them (e.g. `routes/mod.rs` vs `models/mod.rs`).
- **R6** — Buffers with unique filenames keep the bare `file_name()` label.
- **R7** — Mouse-click hit-testing (`bufferline_ranges`) stays correct: only
  visible buffers are in `ranges`, with their on-screen x-coordinates.
- **R8** — The active-buffer gradient rendering (`gradient_borders.enable`)
  continues to work for the active label wherever it is drawn.
- **R9** — All clipping/overflow math respects `viewport` bounds, not
  `surface.area`.
- **R10** — Overflow indicators show the *count* of hidden buffers on each side
  (`‹12`, `5›`), not bare arrows — position-at-scale is half of the user's
  complaint #2 and bare arrows don't address it.
- **R11** — 0-buffer and 1-buffer cases render without panic and without
  indicators; the bufferline is a single global element (not per-split) and the
  scroll offset is shared across splits, keyed to the currently active buffer.

## Design

### New state on `EditorView`

```rust
/// Index (into DocumentId order) of the leftmost buffer drawn in the
/// bufferline. Adjusted minimally each frame to keep the active buffer and a
/// small neighbor margin visible. See R1-R3.
bufferline_first_visible: usize,
```

Initialized to `0` in `EditorView::new`. (Index-based, not pixel-based: buffer
labels have variable width, and index anchoring keeps the math simple and the
strip aligned to buffer boundaries — no half-clipped labels at the left edge.)

`render_bufferline` gains a `&mut usize` parameter for this, mirroring how
`ranges: &mut Vec<...>` is already threaded through.

### Phase 1 — compute labels + widths (new)

Replace the single streaming pass with: first build a `Vec` of per-document
display info in `DocumentId` order:

```rust
struct Tab {
    id: DocumentId,
    label: String,     // already includes leading/trailing spaces + [+]
    width: u16,        // display columns (unicode-width of label)
    is_active: bool,
}
```

Label = `" {disambiguated_name}{[+]?} "`. Disambiguation per R5/R6 below.

### Phase 2 — filename disambiguation (D, R5/R6)

Disambiguate to **global** uniqueness across all open buffers, not just within a
filename group — otherwise extending one group can collide with a member of
another (e.g. `a/x.rs`, `b/x.rs`, `a/y.rs`: naively extending the `x.rs` group
to `a/x.rs` collides nothing here, but `a/x.rs` vs `a/y.rs` could both shorten to
`a/…` under a within-group-only rule). Iterate to a fixpoint:

```
k[i] = 1 for all i                      // start at bare file_name()
loop:
    label[i] = last k[i] components of path[i] (joined by std::path::MAIN_SEPARATOR)
    collisions = indices whose label[i] equals some other label[j]
    if collisions is empty: break
    progressed = false
    for i in collisions:
        if k[i] < component_count(path[i]):
            k[i] += 1; progressed = true
    if not progressed: break            // all colliders already at full path
```

Termination: full paths are unique (see edge cases), so the only labels that can
remain colliding at the end are ones already grown to their full path — which
can't actually be equal. Each non-terminal iteration increments at least one
`k[i]`, bounded by max path depth.

Edge cases:

- **Scratch / no path**: `file_name()` falls back to `[scratch]`
  (`SCRATCH_BUFFER_NAME`). Multiple scratch buffers have no path to extend, so
  disambiguate them by appending the `DocumentId` (e.g. `[scratch] (3)`).
- **One member has a path, another doesn't**: the no-path one keeps `[scratch]`
  (+id if needed); pathed ones extend normally.
- **Identical full paths**: cannot happen — `DocumentId` is unique per opened
  path; `editor.documents` is keyed by id and the editor reuses the doc for an
  already-open path.

Disambiguation is computed over **all open buffers**, not just visible ones, so
a label doesn't change as it scrolls in and out of view.

### Phase 3 — resolve scroll offset (A, R1-R3)

Inputs: `tabs: &[Tab]`, `active_idx`, current `*first_visible`, usable width
`avail` (viewport width minus reserved indicator columns — see Phase 4).

Helper: `last_fit(fv) =` the largest `j >= fv` such that
`sum(width[fv..=j]) <= avail`, but always at least `fv` (a single oversized tab
still counts as visible — satisfies R1 degenerately).

```
fv = clamp(*first_visible, 0, tabs.len() - 1)

// scroll LEFT if the active buffer is too close to (or past) the left edge.
// This step only ever decreases fv.
left_limit = active_idx - min(MARGIN, active_idx)
if fv > left_limit: fv = left_limit

// scroll RIGHT until active + right MARGIN are visible. This step only ever
// increases fv, and the `fv < active_idx` guard means it can never push the
// active buffer itself off the left — fixing the off-by-one in the prior draft.
right_target = min(active_idx + MARGIN, tabs.len() - 1)
while last_fit(fv) < right_target and fv < active_idx:
    fv += 1

*first_visible = fv
```

The two steps cannot oscillate: the left step only decreases `fv`, the right
loop only increases it and is bounded above by `active_idx`. When the active
buffer is already within its margins, neither step fires — the strip stays put
(R3). This is the vim-`scrolloff` shape: minimal movement, only at the margins.
Result is written back to `*first_visible` for the next frame.

Degenerate width: if a single label is wider than `avail`, it still renders as
the sole visible tab (clipped to `avail`); R1 is satisfied "as fully as space
permits".

### Phase 4 — render visible slice + indicators (B, R4, R10)

**Fast path (no overflow).** First test whether *all* tabs fit in the full
`viewport.width` with no reserved columns. If so: `first_visible = 0`, no
indicators, render everything. This is the common small-buffer-count case and it
sidesteps the reserve/offset circular dependency entirely — indicators only ever
appear when overflow genuinely exists, so a phantom indicator (R4) is impossible.

**Overflow path.** Overflow exists, so at least one side is hidden. To break the
"avail depends on reserve, reserve depends on offset" circularity without a
fixpoint, reserve a fixed `RESERVE = 4` columns (enough for `‹` + up to 3
digits) on each side that *can* be hidden:

- left reserve = `RESERVE` iff `first_visible > 0` (carried from last frame's
  offset, then re-confirmed after Phase 3).
- right reserve = `RESERVE` (overflow exists; if it turns out all hidden buffers
  are on the left, the right region simply renders as background — wasted ≤4
  cols, never a phantom arrow).

Compute `avail = viewport.width - left_reserve - right_reserve`, run Phase 3,
then render:

- Clear the viewport with the background style (unchanged from current).
- `hidden_left = first_visible`; render `tabs[first_visible..]` left→right
  starting at `viewport.x + left_reserve`, stopping before the right reserve
  boundary. Track `last_rendered`; `hidden_right = tabs.len() - last_rendered - 1`.
- Draw indicators in `bufferline_inactive` style, right-/left-aligned in their
  reserved cells: left `‹{hidden_left}` iff `hidden_left > 0`, right
  `{hidden_right}›` iff `hidden_right > 0`. A reserved-but-unused side draws
  background only (R4).
- Render `tabs[first_visible..]` left→right starting after the left indicator,
  stopping at the right reserved boundary. For each rendered tab push
  `(id, start_x, end_x)` into `ranges` (R7).
- The active tab uses the gradient path exactly as today when
  `gradient_borders.enable`, except `text_width` is bounded by the visible
  region, and the per-char loop / `set_stringn` start at the tab's resolved
  `start_x` (R8).
- All boundary tests use `viewport.x` / `viewport.right()` (R9).

### Mouse (optional, low priority)

Clicking the `‹`/`›` indicators could page the strip (adjust `first_visible`).
Not required for this spec; if added, hit-test the reserved indicator columns
before the per-tab loop in the existing handler (`editor.rs:1382`).

## Constants

- `MARGIN: usize = 1` — neighbor buffers kept visible on each side. Reviewed as
  adequate (one neighbor each way = next/prev); revisit to 2 only if the snap on
  off-screen jumps feels abrupt in practice.
- `RESERVE: u16 = 4` — columns reserved per overflow side (`‹` + up to 3 digits).
- Indicator glyphs `‹` / `›` (U+2039 / U+203A); ASCII-safe fallback `<` / `>`.

Phase 3/4 are entered only when `!tabs.is_empty()`; the existing
`use_bufferline` gate already requires `documents.len() > 1` for the `Multiple`
setting, but guard anyway for the `Always` setting with a scratch-only editor.

## Test plan

Pure helper functions should be extracted and unit-tested without a terminal:

- `disambiguate(paths: &[Option<PathBuf>], ids) -> Vec<String>`:
  - unique names → bare filenames (R6).
  - `a/mod.rs`, `b/mod.rs` → `a/mod.rs`, `b/mod.rs` (R5).
  - `a/b/x.rs`, `c/b/x.rs` → `b/x.rs`, `b/x.rs`? No — needs `a/b/x.rs` vs
    `c/b/x.rs` (k grows to 3). Assert minimal-k correctness.
  - two scratch buffers → `[scratch] (n)` (R5 edge).
  - mixed path / no-path.
- `resolve_first_visible(widths, active_idx, prev, avail, margin) -> usize`:
  - active already centered → unchanged (R3).
  - active past right margin → scrolls right by minimum (R1/R2).
  - active before left margin → scrolls left (R2).
  - single oversized label → returns active_idx (degenerate).
  - avail smaller than everything → active still chosen.

Manual: open ~30 files incl. several `mod.rs`; `Space n`/`Space p` across the
set; confirm the strip follows, neighbors stay visible, arrows appear/disappear,
and duplicate names disambiguate. Check with `gradient_borders.enable = true`.

## Out of scope

MRU ordering, pinned buffers, two-row layout, filetype icons/colors, diagnostic
glyphs.

**Flagged follow-up (P1):** ordinals + jump-to-N (idea E). The UX review argues
that hidden-counts (now folded in as R10) plus disambiguation address complaint
#2's *identification*, but a per-tab ordinal (`1 2 3…`) with a `goto_buffer_n`
command is the natural next step for *targeting* at scale. Kept out of this spec
to hold scope to A+B+D, but recommended as the immediate next change.

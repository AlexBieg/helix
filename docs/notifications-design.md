# Popup Notifications — Design

Status: proposed (not yet implemented)

## Concept

A **toast stack** in the top-right of the viewport. Each `set_status` /
`set_error` / `set_warning` (and each async status message) becomes a
self-contained, multi-line, color-coded notification that slides in,
auto-dismisses on a severity-scaled timer, and can be dismissed by mouse click
or keybind. The most recent message is also mirrored into the status line
(unchanged single-line behavior) as a fallback/history hint.

```
                                          ┌──────────────────────────────┐
                                          │▎ Error                       │
                                          │▎ Failed to write file.rs:    │
                                          │▎ permission denied (os 13)   │
                                          └──────────────────────────────┘
                                          ┌──────────────────────────────┐
                                          │▎ LSP                         │
                                          │▎ rust-analyzer: indexing…    │
                                          └──────────────────────────────┘
  the quick brown fox jumps over…                        +2 more
~                                                              NOR  1:1
```

The left accent bar (`▎`) carries the severity color; the box uses a popup
background. The stack grows downward; overflow collapses to a `+N more` line.

## Decisions

- **Relationship to status line:** popups are primary; the most recent message
  is mirrored into the existing status line as a fallback.
- **Position / stacking:** top-right, stack downward.
- **Dismissal:** auto-dismiss on a severity-scaled timer; mouse click to
  dismiss; plus a dismiss keybind that works in any mode. Keypresses do **not**
  dismiss toasts.
- **Errors:** sticky (no auto-dismiss) — cleared via the dismiss keybind or
  mouse.
- **Animation:** subtle slide-in + fade-out.

## Data model (helix-view)

The notification list is owned by `Editor` so any code path — commands, core,
LSP, jobs — can push without touching the compositor, exactly like `status_msg`
does today.

```rust
// helix-view/src/editor.rs
pub struct Notification {
    pub id: u32,
    pub text: Cow<'static, str>,          // may contain '\n'
    pub severity: Severity,
    pub created_at: Instant,              // drives slide-in
    pub expires_at: Option<Instant>,      // None = sticky
    pub count: u32,                       // coalesced duplicates
}

#[derive(Default)]
pub struct Notifications {
    items: Vec<Notification>,
    next_id: u32,
}
```

`Editor` gains `pub notifications: Notifications` and a single entry point:

```rust
pub fn push_notification(&mut self, text: Cow<'static, str>, severity: Severity) {
    let timeout = self.config().notifications.timeout_for(severity);
    self.notifications.push(text.clone(), severity, timeout);
    // mirror most-recent into the status line (existing behavior preserved)
    self.status_msg = Some((text, severity));
}
```

`set_status` / `set_error` / `set_warning` (`helix-view/src/editor.rs:1513-1532`)
call `push_notification` instead of writing `status_msg` directly. The async
handler at `helix-term/src/application.rs:332` calls it too — and the
`// TODO: show multiple status messages at once to avoid clobbering` goes away,
since the stack now holds many.

**Coalescing:** `push` checks whether the newest live notification has identical
`text` + `severity`; if so it bumps `count` and resets `created_at`/`expires_at`
instead of adding a row (`rust-analyzer: indexing… (×3)`). This keeps bursty
LSP/autosave chatter from flooding the stack.

The view stays a **pure function of `(notifications, Instant::now())`** — no
animation state stored separately, so resize and redraw are trivially correct.

## Rendering & always-on-top

Toasts must sit above pickers and completion menus, so they render as a **final
pass after `compositor.render()`** in `Application::render`, rather than as an
ordinary compositor layer (layers get covered by anything pushed later). A small
`NotificationsView` struct holds the layout math and theming; it owns no state.

Per-notification box, computed each frame from the viewport:

- width = `clamp(longest_wrapped_line + padding, min=30, max=min(50, viewport.width/3))`
- text wrapped to inner width, height capped (e.g. 6 lines, then ellipsis)
- right-aligned at `viewport.width - width - 1`, first box at `y = 1`, each
  subsequent box below with a 1-row gap
- stop after `max_visible` (default 5); if more remain, draw a dim `+N more`,
  right-aligned

This reuses the same primitives as `Info` (`helix-term/src/ui/info.rs`):
`surface.clear_with(area, popup_style)`, `Block::bordered()`, `Paragraph`. The
title line is the severity label (`Error`/`Warning`/`Info`) or an optional
source tag.

## Colors (theme)

Reuse existing severity scopes for the accent (`error`/`warning`/`info`/`hint`
already in `theme.toml`) and add notification background scopes that fall back to
`ui.popup`, so existing themes look right with zero changes:

| Scope | Role | Fallback |
|---|---|---|
| `ui.notification` | box bg/border | `ui.popup` |
| `ui.notification.error` | accent bar + title | `error` |
| `ui.notification.warning` | " | `warning` |
| `ui.notification.info` | " | `info` |
| `ui.notification.hint` | " | `hint` |

`theme.try_get` already does dot-segment fallback
(`helix-view/src/theme.rs:433`), so `ui.notification.error` →
`ui.notification` → `ui` for free.

## Dismissal & timing

**Auto-dismiss (severity-scaled), configurable:**

| Severity | Default timeout |
|---|---|
| Hint | 2s |
| Info | 3s |
| Warning | 5s |
| Error | sticky (0) |

**Mouse click:** `Application::handle_terminal_events` hit-tests `Mouse(Down)`
against the toast rects *before* forwarding to the compositor; a hit removes
that notification (starting its fade-out) and consumes the event so it doesn't
move the cursor.

**Keybind:** two new `MappableCommand`s, rebindable like anything else, that
operate purely on `editor.notifications` and never touch mode (so dismissing
leaves you exactly where you were):

- `dismiss_notifications` — clears the whole toast stack. Default binding:
  `space-N` ("Notifications"). (`space-D` turned out to be taken by
  `workspace_diagnostics_picker`.)
- `dismiss_notification` — clears just the newest/top toast. Unbound by default.

**Keypresses do NOT dismiss toasts.** Today input clears `status_msg`
(`helix-term/src/ui/editor.rs:1248`); that behavior is kept for the *status-line
mirror*, but the toast stack is independent and only auto/mouse/keybind
dismisses.

**Pruning:** the editor drops notifications whose deadline has passed. Rather
than busy-polling, the render pass schedules a *single* wake-up at the soonest
pending expiry via `Editor::request_redraw_at` (which arms the existing
`redraw_timer`); when it fires, the toast is pruned and the next expiry is
re-armed. This keeps the editor idle between toasts — important so idle-driven
features and the test harness's idle detection still work. (Phase 2's animation
will additionally request per-frame redraws, but only while a toast is actually
sliding/fading.)

## Animation (subtle slide + fade)

Driven entirely by `helix_event::request_redraw()`, which schedules the next
frame ~33ms out (`helix-view/src/editor.rs:2510-2518`) — the same mechanism LSP
spinners ride on. No new timer is added.

At the end of `NotificationsView::render`, if any notification is animating or
counting down, call `helix_event::request_redraw()` to schedule the next frame.
When the stack empties, requesting stops and the editor returns to fully idle (no
busy-loop).

Per notification, derived purely from elapsed time:

- **Slide-in** (first ~150ms of `created_at`): the box eases horizontally from
  `viewport.width` (off-screen right) to its resting x.
  `progress = clamp01((now - created_at) / 150ms)`, ease-out cubic.
- **Fade-out** (last ~150ms before `expires_at`, or once dismissed): approximate
  opacity in a terminal by stepping the style toward the background —
  `Modifier::DIM` for the cheap version, or blend fg→bg in 3–4 steps for a
  smoother feel.
- Boxes below an exiting one ease upward to fill the gap (same 150ms easing on
  their target y).

Graceful degradation: when `editor.notifications.animate = false` (or a known-slow
terminal), boxes appear/disappear instantly — the same code with easing clamped
to 0/1.

## Config (helix-view editor config)

```toml
[editor.notifications]
enable = true
max-visible = 5
animate = true
# 0 disables auto-dismiss (sticky)
timeout = { hint = 2000, info = 3000, warning = 5000, error = 0 }
```

Standard serde `Deserialize` + `Default` like the rest of `editor.*`. `position`
is hardcoded top-right for v1 but lives in this struct so other corners can be
added later.

## Edge cases

- **Resize:** layout recomputed from viewport each frame; nothing stored.
- **Tiny terminals:** clamp width to viewport, shrink `max-visible`; if height <
  one box, suppress toasts and rely on the status-line mirror.
- **Bursts:** coalescing + `max-visible` cap + `+N more`.
- **Long single-line spam from LSP:** coalescing collapses repeats; multi-line
  wrapping handles length.

## Testing

### Testability hook: inject time

`Instant::now()` is never called *inside* `Notifications` or the layout/animation
math. Instead those methods take a `now: Instant` parameter — production passes
`Instant::now()`, tests pass a fixed instant and step it forward by hand. This
keeps every timing rule (auto-dismiss, fade, slide easing) a deterministic pure
function and keeps timer behavior out of the async event loop, which the harness
cannot advance reliably (and where a prior timer change caused a regression —
see `FUTURE.md`).

### Automated tests

**1. `Notifications` unit tests** (`helix-view/src/editor.rs`, inline
`#[cfg(test)]`) — the bulk of the logic, all deterministic via injected `now`:

- `push` sets `expires_at = now + timeout` for the severity; error/timeout-0
  yields `expires_at = None` (sticky).
- Coalescing: pushing identical `text` + `severity` bumps `count` and resets
  `created_at`/`expires_at` instead of adding a row.
- `prune(now)` drops only notifications past their fade-out; keeps live and
  sticky ones.
- `dismiss(id)` / `dismiss_top` / `dismiss_all` remove the right items and start
  fade-out rather than vanishing instantly.
- Ordering: newest at the top of the stack.

**2. Layout / animation unit tests** (`helix-term`, on a pure
`NotificationsView::layout(viewport, &notifications, now) -> Vec<Placement>`):

- Width clamping (min 30, max `min(50, viewport.width/3)`); narrow-terminal
  clamp; sub-box-height terminal → empty layout (suppressed).
- Wrapping, height cap, ellipsis.
- Stacking y-positions with the 1-row gap; `max_visible` cap; `+N more` present
  with the correct N.
- Easing is a pure function: off-screen-right at `now == created_at`, resting x
  at `created_at + 150ms`, monotonic between; clamps to 0/1 when
  `animate = false`.
- Hit-test (`point ∈ placement.rect`) used by mouse dismissal, tested as a pure
  function against computed placements.

Optionally, render a layout into an off-screen `tui::buffer::Buffer` and assert
specific cells carry the expected glyphs/severity style.

**3. Integration tests** (`helix-term/tests/test/`, via `test_with_config` +
inspect-closure) — exercise the real command/editor path, asserting on state, not
on wall-clock timing:

- `:notify --error hello<ret>` → `editor.notifications` has one `Error` item and
  `editor.status_msg` mirrors it.
- Push several, run `dismiss_notifications` → stack empty, you stay in the same
  mode.
- Coalescing: `:notify info x` twice → one item, `count == 2`.

Auto-dismiss *timing* and mouse-click dismissal are covered at the deterministic
unit layer (injected `now` / hit-test), **not** in integration tests, to avoid
real-timer flakiness.

### Manual testing — the `:notify` command

A typable command makes every visual/timing aspect easy to trigger by hand (and
it's genuinely useful to theme authors, so it ships, not test-gated):

```
:notify [-s|--severity hint|info|warning|error] [-r|--repeat N] <message…>
```

- `--severity` (completer offers the four levels; default `info`) drives color
  and timeout.
- `--repeat N` pushes N copies — exercises stacking, `max-visible`, and `+N more`.
- The message is taken raw, so embedded `\n` tests multi-line wrapping:
  `:notify -s error "line one\nline two\nand a much longer third line that wraps"`.

Manual checklist:

- Each severity shows the right accent color and title.
- Long text wraps; very long text hits the height cap + ellipsis.
- `--repeat 8` stacks and collapses overflow to `+N more`.
- Auto-dismiss timing per severity; error stays sticky.
- `space-D` (`dismiss_notifications`) clears the stack without changing mode;
  mouse click clears a single toast.
- Status-line mirror shows the latest message.
- Slide-in/fade looks smooth; `animate = false` is instant.
- Terminal resize reflows; very narrow/short terminal degrades gracefully.

## Build order

1. **Data + plumbing, no animation:** ✅ **Implemented.** `Notification` /
   `Notifications` (with injected `now`), `push_notification`, reroute `set_*`
   and the async handler, static top-right render, auto-dismiss (single
   scheduled wake via `request_redraw_at`), mouse dismiss, status-line mirror,
   config, the two dismiss commands + `space-N` keymap default, and the
   `:notify` command for manual testing. Ships with `Notifications` + layout
   unit tests and the integration tests above. Default theme registers
   `ui.notification`; other themes fall back to `ui.popup` + severity scopes.
2. **Animation:** slide-in + fade-out + reflow via timestamps and
   `request_redraw`, with layout/easing unit tests.
3. **Polish:** `+N more` overflow, coalescing counter, theme scopes + docs
   (`book/src/`), default-theme entries.

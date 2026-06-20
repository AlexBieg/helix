# Session Persistence — Design Questions

## Scope — what gets persisted?

- [x] Open buffers/documents and their order?
- [x] Cursor position (line + column) per buffer?
- [x] Selections (including multiple cursors)?
    - Only do this one if it's easy
- [x] Scroll position per buffer?
- [x] Active buffer (which one has focus)?
- [x] Window splits / layout tree?
- [ ] LSP workspace state?
- [ ] Terminal split content/working dir?
    - Explain this one more I don't understand
- [ ] File picker state / last search?

## Trigger — when do we save?

- [x] On each buffer open/close? On editor `close`? On exit/hup signal?
    - Consider saving any buffer edits too. Maybe debounced?
- [x] Periodically (every N seconds)? On focus loss / idle?
- [x] Should the user be able to trigger it manually via `:save_session`?
- [ ] Should there be an explicit "save & restore" model vs. transparent auto-save?
    - Explain this one more to me

## Restoration — when do we load?

- [x] Automatically when opening a directory (`hx .`)?
- [ ] Automatically when opening with no args (`hx`)?
- [ ] Only via explicit `:load_session` or CLI flag (`hx --session`)?
- [x] What if the session references files that no longer exist?
    - Then those buffers can be skipped. If all buffers reference unopened files then just default to a scratch
- [x] What if the session is for a different git branch / working tree state?
    - See above

## Session identity — one session or many?

- [x] One global "last session" per project root (`.git` or workspace dir)?
- [ ] Named sessions (like tmux/vim `:mksession`)? If named, where stored?
- [ ] Branch-aware sessions (different session per git branch)?
- [ ] Should there be a session picker (like the recent file picker)?

## Interaction with existing features

- [ ] How does this interact with the **recent file picker**?
- [ ] How does it interact with the **file watcher** / auto-reload?
- [x] If a persisted file was dirty on exit, what happens on restore — reopen the file, prompt to recover swap/backup?
    - Any unsaved changes can be discarded. Just reload the file from disk
- [x] Should the session store the recent files list itself?
    - Yes I want that to be populated when re-opening the editor

## Configuration

- [x] Opt-in or on by default?
    - On by default
- [x] Config key ideas? `session.persistence = true`, `session.save-on-exit = true`, `session.restore-on-startup = true`?

---

## Bug Fixes (2025-06-20)

- [x] **Active buffer not restored on restart.** `load_session()` stored `active_document_index` but never used it to set focus after loading; focus always ended up on the last-loaded view.
- [x] **Session saved with empty tree on exit.** `Application::close()` called `save_session()` after all views were destroyed, overwriting the valid session with an empty layout.
- [x] **Session only saved on last `:q`.** `quit()` and `force_quit()` only called `save_session()` when `views().count() == 1`, so with splits the first `:q` closed a view unsaved, and the second `:q` saved a degraded tree.
- [x] **Subsequent `:q` calls overwrote full session.** Added `session_saved_this_lifecycle` flag so the session is saved once on the first `:q` (capturing the full layout) and skipped on subsequent `:q` calls that close remaining views.

*Your answers below:*


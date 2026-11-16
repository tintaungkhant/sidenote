# 008 — Browse notes and recover deletions

Status: Done (native UI interaction remains unverified)
Dependencies: 004

## Outcome

Closed notes remain easy to find, and deletion is recoverable until explicitly made permanent.

## Work

- Add collapsible All Notes with title/plain-text search and open/rename actions.
- Move deleted notes to Trash and remove their open tabs in a consistent operation.
- Add Trash browsing, restore, and confirmed permanent deletion.
- Preserve attachments while referenced by live or trashed notes. Do not introduce automatic Trash expiry or eager attachment collection.
- Prevent stale pending saves from resurrecting deleted notes or overwriting restored state.

## Acceptance

- [x] A closed note appears in All Notes and reopens without duplication.
- [x] Search finds plain text across text blocks without exposing image markers or matching annotation text.
- [x] Delete/restart/restore retains text, images, and editable annotations.
- [x] Permanent deletion requires confirmation and does not break another note's image reference.
- [x] Deleting the active note produces a valid active tab or empty state.
- [x] Sidebar and Trash actions use labeled native buttons, inputs, and forms for keyboard access; live interaction is unverified.

## Proof

Test deletion against pending saves, shared references, search, and restoration across restart.

## Completion evidence

- Added All Notes and Trash inside the collapsible sidebar, with explicit search/refresh, 100-result pages, open/rename, restore, and a separate permanent-delete confirmation.
- Search folds Unicode case in Rust and concatenates only text blocks. It scans documents one at a time when title search does not match; results exclude document bodies and image bytes. Literal marker characters in user text remain searchable.
- Lifecycle operations freeze editor input and flush pending generations before native revision checks. Trash and permanent deletion prune saved tabs in the same SQLite transaction. Restore increments revision; old saves cannot resurrect or overwrite restored notes. Autosave drops the removed identity before a restored note can register again.
- No schema change, attachment collection, or Trash expiry. Permanent deletion removes only the selected trashed note and its reference rows; shared originals remain intact.
- `bun test`: 38 passed, including pending-save lifecycle adoption. `cargo test --manifest-path src-tauri/Cargo.toml --lib`: 26 passed, one live clipboard check ignored. Tests cover Unicode/source-only search, bounded pages, restart, preserved annotations, revision conflicts, shared originals, active neighbor and empty session transitions.
- `bun run check`: zero errors/warnings. `bun run build`: passed, with the existing large editor chunk warning. Rust formatting and `git diff --check` passed.
- Reviewed comments for storage transactions, IPC contracts, autosave ownership, sidebar search, and lifecycle orchestration.
- No browser or visual tests ran. Native keyboard/focus interaction and Windows/macOS UI behavior remain unverified. Large-library search latency has not been measured.

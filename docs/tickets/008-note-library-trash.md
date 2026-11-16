# 008 — Browse notes and recover deletions

Status: Todo
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

- [ ] A closed note appears in All Notes and reopens without duplication.
- [ ] Search finds plain text across text blocks without exposing image markers or matching annotation text.
- [ ] Delete/restart/restore retains text, images, and editable annotations.
- [ ] Permanent deletion requires confirmation and does not break another note's image reference.
- [ ] Deleting the active note produces a valid active tab or empty state.
- [ ] Sidebar and Trash actions are keyboard accessible.

## Proof

Test deletion against pending saves, shared references, search, and restoration across restart.

## Completion evidence

Pending.

# 003 — Make saving automatic and exit safe

Status: Todo
Dependencies: 002

## Outcome

Users can quit without saving manually, and acknowledged writes cannot be overwritten by stale asynchronous saves.

## Work

- Add per-note dirty/revision state and one ordered native write queue.
- Coalesce text writes with a maximum one-second commit-attempt delay during continuous input.
- Show Saving, Saved, and actionable error states tied to actual commit acknowledgements.
- Intercept close and quit, flush dirty notes/session state, and guard repeated close events.
- Keep unsaved work available on failure; offer retry, recovery export, and explicit discard before failed-save exit.
- Enforce one application instance per data directory; subsequent launches focus the first.

## Acceptance

- [ ] Continuous typing does not postpone persistence indefinitely.
- [ ] Rapid edits and delayed responses cannot replace newer content or falsely show Saved.
- [ ] Normal immediate exit after typing persists the final edit without a save prompt.
- [ ] Injected write failure keeps the app open and the latest content in memory.
- [ ] Forced termination preserves all acknowledged revisions.
- [ ] A second launch does not create a competing writer.

## Proof

Test delayed/failed writes and repeated close events. Manually verify window close and explicit quit on both OSes; record any unavailable platform checks.

## Completion evidence

Pending.

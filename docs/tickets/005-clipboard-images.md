# 005 — Paste native screenshots into notes

Status: Todo
Dependencies: 001, 002, 003, 004

## Outcome

Native screenshot paste inserts a persistent inline image at the cursor on Windows and macOS.

## Work

- Choose a scoped native clipboard API supporting screenshots on both OSes.
- Prefer image insertion when the clipboard offers image and text; preserve normal plain-text paste otherwise.
- Validate type, encoded size, and dimensions against the technical spec before committing an insertion.
- Persist original pixels and insert an atomic image block without dangling references.
- Fit display to editor width, load image content on demand, and release decoded resources when unused.
- Keep removed image bytes available for undo and restart-safe references.

## Acceptance

- [ ] OS screenshot tools can copy an image that pastes inline without a file-save step.
- [ ] Text before/after paste remains editable and survives restart.
- [ ] Undo/redo image insertion and deletion restore the correct original.
- [ ] Rejected or failed paste leaves the document unchanged and explains the error.
- [ ] A store failure never leaves a committed missing-image reference.

## Proof

Test validation boundaries and insertion rollback. Verify native screenshot clipboard payloads on both OSes through non-visual functional checks, including high-DPI pixel dimensions. Do not run browser or visual tests.

## Completion evidence

Pending.

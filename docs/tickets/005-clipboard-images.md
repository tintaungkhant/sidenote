# 005 — Paste native screenshots into notes

Status: Implemented — native screenshot-tool verification on Windows and macOS pending
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

Implemented a read-only native clipboard command using `arboard` (Windows/macOS image support). Clipboard image data takes priority over event-provided plain text. Native RGBA pixels are checked for dimensions and exact buffer length, encoded losslessly to PNG, and passed through the existing validated immutable attachment store before an editor transaction receives an attachment ID. Clipboard access and PNG encoding run off the UI thread. Clipboard failures are reported without falling back silently or editing the document.

Paste temporarily prevents editing and tab transitions, preserves the target selection, and participates in guarded exit. Image insertion is one isolated undo entry; deleted originals remain in SQLite for undo and restart. Plain-text fallback uses the paste event's text, including when the clipboard offers HTML. The toolbar's Paste screenshot action reads images only; normal text paste uses Ctrl/Cmd+V.

Image widgets load originals when mounted and own their object URLs. Widget destruction revokes URLs and discards late asynchronous results. Switching tabs destroys the old view without preloading all originals for the new tab. Newly pasted compressed originals remain cached for in-session undo before durable note references exist. Images fit available editor width and retain original pixel dimensions.

Proof: 27 frontend tests pass, including image insertion/deletion undo/redo and mixed-text round trips. 19 Rust tests pass, including native pixel validation, attachment validation, restart persistence, missing-reference rejection, and injected reference-write rollback. `bun run check` reports zero errors/warnings; `bun run build` passes with the existing large-editor-chunk warning. No browser or visual tests were run.

Native screenshot-tool clipboard checks, including high-DPI Windows captures and macOS captures, remain pending; unit tests do not establish those platform interactions. `arboard` allocates its native image buffer before returning dimensions, so the application rejects excessive dimensions before PNG encoding/storage but cannot impose its own pre-allocation bound inside the platform clipboard decoder. No clipboard-write API or general clipboard plugin permission is exposed.


### RustDesk clipboard compatibility

A Windows clipboard format inventory during the reported RustDesk failure showed `image/png`, `CF_DIBV5`, `CF_BITMAP`, `CF_DIB`, and RustDesk's owner marker. Arboard 3.7 checks the registered `PNG` format and then DIBV5, but does not check `image/png`. The reported conversion error is consistent with its bitmap conversion path.

The Windows reader now prefers the exact `image/png` format through `clipboard-win`, already used transitively by arboard. It holds the clipboard lock across size validation and byte retrieval, rejects payloads larger than 20 MiB before copying, and sends encoded bytes through the existing bounded attachment decoder/store. Other screenshot formats retain the arboard path. Clipboard contents are never modified.

All 19 regular native tests pass. A new opt-in live clipboard test validates the current image through a temporary store without touching the user's database or clipboard. Its first attempt found no image because the clipboard changed. The user subsequently confirmed RustDesk paste works after the format fix. Automated live clipboard verification and native macOS screenshot verification remain pending.

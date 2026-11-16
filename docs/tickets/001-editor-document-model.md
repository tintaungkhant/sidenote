# 001 — Establish the editor and document model

Status: Implemented — native verification pending
Dependencies: None

## Outcome

A CodeMirror 6 editor supports source text and atomic inline images without corrupting text or exposing internal markers.

## Work

- Replace the template page with a minimal editor host using the existing SvelteKit app.
- Install Tailwind CSS 4 through its Vite integration and load the generated styles in the app. Use utilities for host layout; keep CodeMirror theme styling in its theme API or dedicated CSS.
- Define a versioned ordered text/image document and typed annotation/attachment identifiers.
- Implement the CodeMirror mapping and image decoration seam using a test fixture image.
- Add line numbers, wrap toggle, in-note find, indentation, text undo/redo, and explicit language selection for the languages in the product spec.
- Define copy behavior for text, images, and mixed selections.

## Acceptance

- [x] Arbitrary text, including marker-like strings, survives editor/document round trips unchanged.
- [x] Images at the start, middle, end, and next to other images leave usable text insertion positions in the state model; native cursor interaction remains unverified.
- [x] Selection, deletion, movement, undo, and redo preserve image identity and surrounding text in focused state tests.
- [x] Internal markers are excluded by the text-copy and search adapters; native clipboard events remain unverified.
- [ ] Svelte mounts/destroys the editor without leaking listeners.
- [ ] Tailwind utilities render in development and the production build without adding a component library.

## Proof

Add focused mapping/history tests. Run `bun run check` and `bun run build`. Record the chosen encoding and any CodeMirror constraints in the technical spec.

## Completion evidence

Implemented on 2026-11-16:

- Replaced the starter page with one CodeMirror view, line numbers, indentation, undo/redo, wrapping, literal in-note find, and all six language choices. Added Tailwind CSS 4 through Vite; the production build emits generated CSS.
- Added versioned text/image blocks, attachment and image identity types, and vector annotation data types. Added a bundled image fixture and undoable insertion without depending on later clipboard/storage tickets.
- Added image-aware range movement, exposed through Move selection left/right. Text-only line reorder, line duplication, and character transposition commands are suppressed while images exist because their ordinary string edits lose identity. Native dragging is disabled for the same reason.
- Plain-text copy/cut omits image blocks, including current-line copy with an empty selection. Image-only selections produce empty plain text; mixed selections retain text in order. Clipboard paste therefore does not transfer images; native image copy/export remains ticket 007.
- Comment review completed for document conversion, state/history, presentation/clipboard, mounting/cleanup, and each editor control. CodeMirror view destruction is wired to Svelte unmount.
- `bun test`: 11 passed, 0 failed. Covers exact text/line-ending preservation, marker-like text, image boundaries, malformed placement, copy, image-aware search, insertion, movement, annotation identity, and undo/redo.
- `bun run check`: 0 errors, 0 warnings.
- `bun run build`: passed. Build reports a JavaScript chunk of approximately 543 kB minified (189 kB gzip) because language support is currently bundled eagerly; native startup/memory impact is unmeasured.
- No browser or visual tests were run. Actual rendering, native cursor/IME/clipboard behavior, and repeated native mount/unmount resource usage remain unverified. The two related acceptance items above remain unchecked rather than claiming visual or leak verification.
- Content is intentionally temporary in this ticket. SQLite and autosave are tickets 002 and 003; the UI states this explicitly.

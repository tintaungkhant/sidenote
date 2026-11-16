# 001 — Establish the editor and document model

Status: Todo
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

- [ ] Arbitrary text, including marker-like strings, survives editor/document round trips unchanged.
- [ ] Images at the start, middle, end, and next to other images leave usable text insertion positions.
- [ ] Selection, deletion, movement, undo, and redo preserve image identity and surrounding text.
- [ ] Internal markers never appear in ordinary copy or search results.
- [ ] Svelte mounts/destroys the editor without leaking listeners.
- [ ] Tailwind utilities render in development and the production build without adding a component library.

## Proof

Add focused mapping/history tests. Run `bun run check` and `bun run build`. Record the chosen encoding and any CodeMirror constraints in the technical spec.

## Completion evidence

Pending.

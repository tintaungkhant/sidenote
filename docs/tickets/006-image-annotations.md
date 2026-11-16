# 006 — Draw and write on images

Status: Done (rendered alignment and native interaction checks remain unverified)
Dependencies: 005

## Outcome

Users can annotate screenshots while keeping originals intact and all annotations editable.

## Work

- Open a focused annotation view from an inline image and return to its editor position.
- Implement pen, highlighter, arrow, and text box tools with a compact palette and stroke widths.
- Add annotation selection, movement, deletion, editable text, and undo/redo.
- Store annotation geometry in original-image coordinates and render correctly at different sizes/device pixel ratios.
- Persist completed operations automatically through the existing save path.
- Support keyboard tool selection, selection cycling, movement, text entry, deletion, and Escape exit.

## Acceptance

- [x] Drawing and text use the same original-image coordinates after resize and restart (numeric proof; rendered alignment unverified).
- [x] Original attachment bytes remain unchanged.
- [x] Two image blocks using one original have independent annotation state.
- [x] Exiting the view retains completed edits and cancels only an unfinished gesture (implemented; native interaction unverified).
- [x] Text editing and undo do not accidentally trigger note-level shortcuts (headless routing proof; native interaction unverified).
- [x] Save failure preserves the current annotation work and shows the existing failure state.

## Proof

Test geometry transforms, serialization, history, keyboard event handling, and restart persistence without a browser. Assert high-DPI coordinate mapping numerically. Do not run visual alignment checks; record rendered alignment as unverified.

## Completion evidence

- Click an inline image or focus its button and press Enter to open a native modal dialog. Done/Escape returns to the note selection. Closing the app drops only the active gesture and flushes completed edits through the existing exit guard.
- Pen, highlighter, arrow, and multiline text share one SVG renderer between inline and focused views. No new dependency, schema, raster rewrite, or export capability was added.
- Select objects by clicking their shape or using Tab/Shift+Tab on the canvas. Drag or use arrow keys (Shift for ten pixels); Delete removes. V/P/H/A/T choose tools. Add text at center supports keyboard-only text creation; its text field saves each completed input event. The modal traps focus and stops note shortcuts, while text inputs retain native editing/undo and IME behavior.
- Each completed operation uses immutable local undo/redo snapshots and an isolated CodeMirror annotation transaction. Annotation-only changes request immediate autosave. Failed saves retain work and display the existing save state inside the dialog; Done returns to the existing retry/recovery controls.
- `bun test`: 34 passed. Covers numeric CSS/high-DPI coordinate mapping, bounded movement, SVG text escaping and all tools, local history branches, keyboard routing, serialization, independent placements, note history, and failed annotation-save recovery.
- `cargo test --manifest-path src-tauri/Cargo.toml --lib`: 20 passed; the existing live clipboard probe remains ignored. New SQLite test updates all four annotation tools, restarts storage, and verifies exact geometry, independent placements, and unchanged original bytes.
- `bun run check`: zero errors or warnings. `bun run build`: passed; existing large editor-chunk warning remains (approximately 573 kB minified).
- Reviewed comments for geometry, history, renderer, modal gestures/keyboard/text, editor transactions, autosave/lifecycle integration, and tests.
- No browser or visual tests ran. Rendered alignment, pointer capture, focus/shortcut/IME behavior, and native dialog interaction on Windows/macOS remain unverified. Geometry and persistence proof is headless and does not claim visual coverage.
- User confirmed the feature works. Further enhancements are deferred; this confirmation does not establish full cross-platform interaction coverage.

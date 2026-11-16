# 007 — Copy and export annotated images

Status: Done (visual fidelity and live native clipboard/dialog interaction remain unverified)
Dependencies: 006

## Outcome

Users can take a flattened screenshot with annotations into another application.

## Work

- Reuse the annotation renderer to compose original-resolution output.
- Add Copy Image through the native clipboard and Export PNG through a destination picker.
- Include the latest in-memory completed annotations, even if their save is pending.
- Handle clipboard/encoding/write errors and canceled destination selection.

## Acceptance

- [x] Copy/export uses the annotation view's shared renderer for pen, arrows, highlights, and text (visual fidelity unverified).
- [x] Output requests source pixel dimensions and transparent source-over composition (headless command/payload proof; actual webview rasterization unverified).
- [x] Copy/export does not flatten or mutate the editable source document.
- [x] Destination cancellation leaves no partial output; overwrite uses the native save picker (atomic filesystem proof; live picker confirmation unverified).
- [x] Failure leaves annotation work intact and reports an actionable error.

## Proof

Test composition commands, output dimensions, PNG decoding, clipboard payloads, and source immutability through non-browser tests. Do not compare rendered images, use visual snapshots, or inspect output visually. Record visual fidelity as unverified.

## Completion evidence

- Open an inline image and use **Copy Image** or **Export PNG** in the annotation toolbar. Copy reports native completion; export reports success or cancellation. Errors preserve editable annotations and suggest retrying or another destination.
- `image-output.ts` reuses `annotationSvg` without selection decoration, renders a transparent SVG overlay, and draws original plus overlay onto a temporary canvas at original pixel dimensions. It reads the current completed immutable annotation snapshot, including unsaved edits, and omits unfinished gestures. It never rewrites document state or original bytes. Temporary object URLs, decoded sources, and canvas buffers are released after output or failure.
- Native output commands use raw binary IPC, avoiding JSON expansion of PNG bytes. PNG-only input is fully decoded before clipboard/file side effects, with a 64 MiB encoded limit, 40-megapixel limit, and 256 MiB decoder allocation budget. These temporary output limits do not alter the 20 MiB stored-original limit. No dependencies or database migrations were added.
- Copy uses arboard's native RGBA image formats. Export obtains its path only through the existing native dialog plugin, then flushes/syncs a temporary file in the destination directory before atomic replacement. Cancellation creates no temporary output; failed replacement cleans up and preserves the destination. Renderer input cannot supply a filesystem path.
- Annotation controls freeze during output. Native app exit waits for pending encoding/clipboard/export completion before flushing notes. An output failure does not turn into a note save failure or discard edits.
- `bun test`: 37 passed. New tests cover shared rendering of all tools, latest unsaved completed snapshots, source immutability, original dimensions, transparent source-over composition command order, and allocation bounds.
- `cargo test --manifest-path src-tauri/Cargo.toml --lib`: 24 passed; the existing live clipboard read probe remains ignored. New tests cover PNG decoding, straight RGBA alpha/dimensions in clipboard payloads, binary-only IPC, malformed/truncated/unsupported input, export cancellation, complete replacement, and failed replacement cleanup/preservation.
- `bun run check`: zero errors or warnings. `bun run build`: passed; the existing large editor-chunk warning remains (approximately 576 kB minified).
- Reviewed comments for renderer/composition ownership, resource cleanup, native validation/clipboard/atomic writes, IPC, modal snapshot/error behavior, and guarded exit integration.
- No browser or visual tests ran. Actual canvas/SVG rasterization, font fidelity, alpha appearance, live native clipboard round trips, destination dialog/overwrite confirmation, and cross-platform interaction remain unverified. No test wrote to the user's clipboard or exported into user-selected files.

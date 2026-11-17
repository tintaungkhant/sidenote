# Simple Note

A small Tauri/Svelte/CodeMirror file editor with inline local images and editable annotations.

Open ordinary UTF-8 text or source files in place, or open a folder for Explorer and workspace text search. Syntax highlighting follows the extension: Markdown, TXT, JavaScript, TypeScript, PHP, HTML, CSS, and JSON. Unknown extensions use plain text.

New tabs are Untitled. **Ctrl+S** saves an existing file or opens Save As. **Save As** asks for a file name, extension, and location, then creates a new folder with that name's stem:

```text
example/
  example.ts
  assets/
  .simple-note.json
```

Existing folders are never overwritten by Save As. Opening a standalone file does not move it; its annotations use `<filename>.simple-note.json` beside it. Copy or zip the entire package folder to share it, then reopen the main file.

Markdown images use `![image](./assets/image.png)`. Code uses explicit image references in valid comments: `// [image:./assets/image.png]`, `/* [image:./assets/image.png] */` for CSS, and `<!-- [image:./assets/image.png] -->` for HTML. TXT uses `[image:./assets/image.png]`. References remain visible and editable; previews appear below their source lines. Metadata is keyed by relative path and stores a stable ID, caption, and drawing operations, with no persistent character offsets. Repeated references to the same path share annotations.

Paste screenshots using Ctrl+V or Edit → Paste screenshot. Click a preview to edit captions, drawings, and text, or copy/export the flattened PNG. Code insertion requires an empty line where the language parser recognizes a real comment; JSON rejects images because it has no comment syntax. Save As can convert reference syntax when the resulting references are valid. Missing originals show a placeholder. Save As reports missing assets instead of producing an incomplete package.

Modified tabs show a dot. Closing one offers Save / Don’t Save / Cancel. Quit uses **hot exit**: open buffers, unsaved source, annotations, selections, scroll, pins, folder, wrap, and sidebar preference are written to app data under `file-editor-recovery/`. This is separate from saved documents. Pasted originals stay there until Save copies them into the document's assets and rewrites their source references. Recovery originals remain after adoption so undo and interrupted saves cannot lose them; automatic recovery-asset cleanup is not implemented.

External changes are checked every two seconds and on focus. Clean buffers reload; dirty buffers retain their work and show a conflict. Deleted or renamed paths stay deleted and retain their buffer with a warning; use Save As elsewhere or reopen the renamed file from Explorer. Save checks the last-read source and companion revision. Atomic temporary-file writes protect individual files; metadata is written before source, with a recovery checkpoint required before document saves. Windows uses `ReplaceFileW` so a destination deleted during save cannot be silently recreated. Source and companion do not form a cross-file filesystem transaction; recovery retains the current buffer if a partial save fails.

The legacy SQLite notes database and backups are untouched and are not opened by the file editor. Migration/export of those notes is outside this change. The old implementation remains available to its regression tests, but its note-library, Trash, and SQLite backup controls are no longer part of the file workspace.

## Limits

- UTF-8 text and companion metadata: 8 MiB each; recovery snapshot: 64 MiB total.
- PNG/JPEG originals: 20 MiB and 40 megapixels. Paths stay within the document folder; remote URLs and parent traversal are not loaded.
- Explorer/search skip symbolic links, `.git`, `node_modules`, and `target`; traversal is bounded to 20,000 entries and 64 levels, with at most 10,000 displayed entries/results. Search reads saved files.
- Rename detection treats the old path as deleted and refreshes the tree; it does not guess which new path owns an unsaved buffer.
- Undo history survives tab switches and ordinary unchanged-source saves. Save As, asset-reference materialization, external reload, and restart rebuild editor state.

## Development and verification

```sh
bun install
bun run tauri dev
bun test
bunx playwright install chromium --only-shell
bun run test:layout
bun run check
bun run build
cargo test --manifest-path src-tauri/Cargo.toml
cargo check --manifest-path src-tauri/Cargo.toml
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
```

Focused tests cover package reopening after relocation, asset bytes and annotations, failure preservation, standalone saves, stale revisions, deletion, source reference rewriting with UTF-16 positions, comment parsing/insertion, dirty-tab transitions, and recovery. The live native screenshot-clipboard test is ignored unless a real screenshot is available.

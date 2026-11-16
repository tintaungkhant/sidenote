# Technical specification

## Current repository

The repository uses Tauri 2, SvelteKit, Svelte 5, TypeScript, and Rust. Ticket 001 replaces the template page with a CodeMirror editor and a versioned text/image model. Tailwind CSS 4 is installed through Vite. Ticket 002 adds native SQLite storage and typed frontend command wrappers. Ticket 003 connects the single-note editor to autosave, session preferences, guarded close/quit, and single-instance handling. Ticket 004 adds the tab shell and session restore. The native greeting command and unused opener plugin/access have been removed. SvelteKit uses static output with SSR disabled. Tauri build commands use Bun.

## UI stack

Use Svelte 5, Tailwind CSS 4, and CodeMirror 6 within the existing SvelteKit and Tauri 2 application. Tailwind is installed through its Vite integration.

Use Tailwind utilities for the shell, tabs, sidebar, toolbars, and layout. Keep CodeMirror themes and annotation canvas styling in dedicated CSS or the editor's theme API. Share color and spacing tokens where useful. Tailwind generates static CSS and adds no browser runtime. No component library is needed for the first release.

## Ownership

- Svelte owns shell layout, tabs, sidebar, and user interaction.
- CodeMirror 6 owns document editing and history. Use its transaction and decoration mechanisms for inline image elements.
- Rust commands own validation, SQLite transactions, asset access, backup, and lifecycle flushing.
- SQLite owns durable note and session state. Keep original image bytes in SQLite BLOBs for a self-contained store and transactionally consistent backups.
- A focused canvas annotation view owns pointer interaction and rendering. Store vector annotations separately from original image bytes. Reuse its renderer for flattened copy/export.

SQLite BLOBs simplify atomic persistence and backup at the cost of a larger database. Never include image bytes in ordinary text-save payloads or list queries. Use an installed library if suitable; otherwise choose maintained SQLite and clipboard dependencies during their tickets. Do not add an ORM, generic repository framework, or multi-provider abstraction.

## Document representation

Persist a versioned document with ordered text and image blocks. Text blocks contain editable source text; image blocks reference an attachment and contain their own annotation state. Avoid storing images as base64 text or visible Markdown paths.

The CodeMirror adapter must map text positions and atomic image blocks losslessly in both directions. Its internal image markers are implementation details and must not leak through editing, copy, search, or persistence. Validate this seam before building the rest of the UI. If the proposed encoding cannot preserve arbitrary text, change the encoding within this ownership boundary before proceeding.

Undo/redo must restore both image placement and associated annotation identity. Reusing the same source image in multiple blocks must not accidentally share mutable annotations.

### Implemented editor encoding

Each image occupies one U+FFFC character inside CodeMirror. A state-field sidecar maps that character's UTF-16 offset to its image block. Only sidecar-identified offsets are images: literal U+FFFC, marker-like strings, and arbitrary source text remain ordinary text. Conversion back to persisted blocks omits internal image characters. CodeMirror uses an explicit LF separator so CRLF and lone CR text are preserved rather than normalized.

Transactions map surviving offsets and remove deleted images. History inversion restores the previous sidecar alongside the text, retaining image and annotation identities. Explicit image insertion and range movement update text and metadata in one transaction. Image decoration widgets use caller-provided local attachment sources; the host resolves immutable local originals on demand. Mounted widgets own and revoke their object URLs.

Text copy/cut and literal search omit image positions. Image-only plain-text copy is empty. Mixed copy retains surrounding text; native image clipboard output is deferred to ticket 007. Native dragging is disabled. Default text-only line movement, line duplication, and transposition commands are suppressed when images exist; use the image-aware selection movement controls instead. The initial editor supports one selection.

Svelte mounts one EditorView and destroys it on unmount. Native event behavior and resource lifetime still require non-visual native verification. Browser and visual verification remain excluded by the project constraint.

## Logical storage model

- `notes`: stable ID, title, document schema version, document JSON, language, revision, creation/update timestamps, optional deletion timestamp.
- `attachments`: stable ID, MIME type, original bytes, pixel width/height, creation timestamp.
- `note_attachments`: note/attachment references maintained in the same transaction as the document. Shared references are allowed; original bytes are immutable.
- `session`: versioned state containing open note IDs in order, pin state, active note ID, per-note selection/scroll, and UI preferences.
- Schema migrations use SQLite's version mechanism and run transactionally. Back up before a migration that could destroy or rewrite existing data.

Application code validates document schema, attachment references, dimensions, annotation numbers/text sizes, and serialized payload limits. Database constraints enforce IDs and relationships. Use parameterized SQL. Validate note ownership of image references in native commands; do not accept arbitrary filesystem paths from document data.

### Implemented native storage (ticket 002)

`src-tauri/src/storage.rs` owns the database and its validation. `src-tauri/src/commands.rs` exposes eight focused commands; `src/lib/storage.ts` defines their frontend contracts. Startup opens `notes.sqlite3` under Tauri's OS-specific application-data directory for `com.ryan.simple-note`. Renderer inputs never select a database or filesystem path.

The store uses `rusqlite` with bundled SQLite, avoiding a separately installed SQLite runtime. UUIDs identify notes, image blocks, and originals. The PNG/JPEG-only `image` dependency validates raster originals and derives MIME type and dimensions natively. `tempfile` provides isolated test stores and atomic recovery-export files. These dependencies provide SQLite transactions, stable identifiers, and maintained image decoding without an ORM or custom decoder.

Schema version 1 includes notes, immutable-original attachments, foreign-key-backed note references, and a revisioned session row. Version-zero initialization runs in one transaction. Unknown versions, incomplete schemas, corrupt files, and initialization failures return errors without resetting data. No existing-data rewrite migration is needed yet; any future destructive migration must implement the backup requirement above before it is added. WAL and FULL synchronous mode protect acknowledged commits. Startup failure currently stops launch with a diagnostic; backup recovery UI remains ticket 009.

Create and update validate a document before writing. Updates compare the expected revision and change content, references, and revision atomically. Responses describe that committed write rather than reloading potentially newer content after commit. Attachment reads require a persisted reference from the requested note. Shared originals are supported; removing references never collects original bytes, preserving future undo/Trash recovery. List queries select at most 200 metadata rows per page and exclude document JSON and image BLOBs. Session writes also compare expected revisions; absent session state has revision zero. Session tabs must have unique IDs, pinned tabs first, finite nonnegative scroll positions, and an active ID belonging to the tab list. Stale note IDs may remain in the saved session; ticket 004 filters them during restoration.

Current native limits are 8 MiB serialized document JSON, 1,024 UTF-8 bytes per title, 100,000 blocks, 10,000 annotations per image block, 100,000 points per stroke, 64 KiB annotation text, 1,000 session tabs, and 1 MiB serialized session state. Supported language identifiers are `plaintext`, `markdown`, `javascript`, `typescript`, `json`, and `html`; the shell's display labels must be mapped to these identifiers during integration. Image originals are limited to 20 MiB encoded and 40 megapixels, with a 256 MiB decoder allocation budget. Annotation positions use original-image bounds; sizes must be finite, positive, and at most 4,096, and colors use `#RRGGBB`.

Native store failures return `{ code, message }`. Tauri may reject malformed IPC argument shapes before the command body executes. Ticket 003 moves all SQLite access to one FIFO worker with enqueue-before-await acknowledgement. The current editor now automatically persists content. Native screenshot pixels are normalized to PNG and stored before insertion; mounted image widgets load originals on demand and revoke local object URLs when destroyed.

## Save ordering and lifecycle

1. Apply an editor transaction locally and mark its note dirty.
2. Coalesce text changes briefly, with at most one second between commit attempts while edits continue. Completed annotation operations schedule persistence immediately.
3. Serialize writes through one native persistence queue. Carry expected revisions; reject stale writes instead of overwriting newer content.
4. In one transaction, update document, attachment references, and revision. Acknowledge only after commit.
5. Mark Saved only when the acknowledged revision matches current local content. Retry failures without replacing the current document with stale data.

Image insertion must store validated bytes before, or atomically with, the first durable document reference. Never commit a reference to missing bytes. Undo retention and Trash prevent premature collection; first release may retain unreferenced attachments rather than risk data loss. Permanent note deletion does not require immediate database compaction.

Intercept normal window close and explicit quit on both OSes. Stop accepting new edits, flush all dirty notes and session state, then allow termination. Failure leaves the window open with retry and explicit discard/recovery choices. Handle repeated close events without duplicate writes. A forced kill bypasses this path; only acknowledged commits are guaranteed.

Use one application instance per data directory. A second launch focuses the existing window. This prevents competing session writers; revision checks still protect asynchronous saves within the instance.

On startup, load metadata and session first, then active note content. Ignore stale session IDs safely without deleting notes. Never silently replace an unreadable database with an empty one. Report the error and offer backup recovery.

### Implemented autosave and lifecycle (ticket 003)

`src/lib/autosave.ts` tracks current input, local generations, acknowledged generations, and expected native revisions per note, plus separate session state. The editor registers loaded content as clean. Text/language changes schedule a fixed 250 ms batch from the first edit; subsequent typing does not reset that deadline. Completed image insertion or annotation-only transactions request an immediate flush. Saves carry immutable snapshots. Acknowledgements advance revisions and acknowledge only their captured generation; they never replace current content or undo history. Saved means every current generation has been acknowledged. Failed writes retain source and session state, show an actionable error, and allow retry without silently reloading stale data.

Only one frontend drain runs at a time. Edits made while a transaction is in flight send their next snapshot after acknowledgement, using the newly acknowledged revision. A slow native transaction may exceed one second; the scheduler does not postpone attempts indefinitely, and it does not send competing writes with guessed revisions. `src-tauri/src/queue.rs` owns a dedicated FIFO worker and the SQLite connection. Commands enqueue before awaiting, rather than relying on blocking-pool scheduling or mutex acquisition order. A barrier acknowledges completion of previously submitted jobs before normal termination.

`src/lib/exit.ts` shares one close attempt across repeated requests. Native window close, native quit, the Quit button, Windows Ctrl+Q, and macOS Cmd+Q all use the guarded lifecycle. The renderer registers its listener and acknowledges readiness before loading notes or admitting edits. Before that acknowledgement, native close exits directly because no local workspace edits can exist. Later close requests freeze editing, wait for startup, pending image insertion, and any recovery export, then flush every dirty note and session generation. Success authorizes native exit. Failure cancels the attempt, resumes the same editor, and offers retry, recovery export, and a separate confirmation before discarding unsaved changes. Confirmed discard drains submitted jobs when the worker is available; a stopped worker does not prevent explicitly authorized exit.

The maintained Tauri single-instance plugin is registered first, before opening SQLite. With the fixed application-data location and bundle identifier, subsequent launches focus the existing main window and terminate before creating another store/session writer. `tokio` synchronization provides asynchronous result acknowledgements; SQLite work itself stays on the dedicated worker. No recurring idle save timer runs when state is clean.

Startup reads session and paginated note metadata, ignores stale tab references, and loads only the active note body. Without a saved session it opens the newest existing note, or creates one empty note if no notes exist. Storage or decoding errors never trigger an empty replacement. The shell normalizes saved tabs against live note metadata, preserving intentional empty sessions. It supports creation, activation, rename, pinning, keyboard reorder, close, and in-session reopen. One editor view is mounted; immutable cached editor states retain independent undo histories. Session selection, scroll, pins, order, active ID, wrapping, and sidebar visibility are persisted. Tab operations are serialized and awaited by guarded exit. See ticket 004 for shortcut definitions and title behavior. Ticket 005 adds read-only native screenshot paste through arboard, prioritizing images over plain-text fallback. Paste freezes edits and tab transitions until original storage completes, and guarded exit awaits that operation. Native screenshot-tool verification on both platforms remains pending.

Emergency recovery export uses a native save dialog through the Rust dialog plugin; the renderer cannot supply a filesystem destination. It writes version-one JSON containing current note snapshots, session state, and available original attachment bytes. Source snapshots up to 64 MiB remain exportable even when ordinary note limits reject a save. Originals are streamed one at a time, and missing or unreadable originals are explicitly marked and counted in the UI. A stopped store worker still permits a source-only export. A temporary file is flushed and synced before replacing the chosen destination; cancellation and failure do not report success. This is an emergency source artifact, not SQLite backup/restore; the backup and restore UI remains ticket 009.

## Image handling and security

Ticket 006 renders editable annotations with SVG in original-image coordinates, sharing escaped vector markup between inline image buttons and a focused native modal dialog. Pointer positions map from the SVG's CSS rectangle without applying device pixel ratio. Completed gestures and text inputs replace only the targeted image placement's annotation array, use isolated CodeMirror history transactions, and immediately enter the existing autosave path. Local annotation undo/redo remains separate from note shortcuts. The modal traps focus; its key events do not reach note-level shortcuts. Exiting discards only the unfinished gesture, while completed operations already belong to the note and guarded exit flushes them normally. The immutable raster original is never rewritten. Object URLs are revoked when either view is destroyed. Rendered alignment and native interaction remain unverified under the no-browser/no-visual-test constraint.

- Read native clipboard image data through a scoped native API. Browser clipboard support alone is insufficient proof on both OSes.
- Initial limits: 20 MiB encoded input and 40 megapixels decoded per image. Check dimensions before allocating full decoded buffers where the decoder permits it. Explain rejection without altering the note.
- Restrict input to decoded raster images; support native screenshot clipboard formats and PNG/JPEG. Normalize native clipboard pixels to a supported lossless stored format.
- Store annotation coordinates in original-image space, independent of editor zoom and device pixel ratio.
- Bound decoded image lifetimes; release canvas buffers and object URLs when views unmount.
- Render text as text, never HTML. Do not execute note content or load remote image URLs.
- Replace the starter's null CSP with a policy allowing only required local resources. Expose only scoped Tauri commands/capabilities; remove unused greeting and opener access.

## Backup and restore

Create a consistent SQLite snapshot using its backup facility, not a raw copy of a live WAL database. Attempt an automatic snapshot after the first successful write each day; retain the seven most recent successful daily snapshots. Run backup work outside the typing path. Report failure without claiming a backup exists.

Manual backup export uses the same snapshot path. Restore validates schema and integrity first, flushes current work, and creates a pre-restore backup. Replace the store only after validation; keep rollback available if replacement or reopening fails. Never merge databases or infer synchronization behavior. Restore replaces local notes and session state after explicit confirmation.

## Code comments

After completing each feature, function, or cohesive set of working files, review its code comments before moving to the next unit of work. This is part of completion for every implementation ticket.

- Document each new or changed function's purpose and contract with an appropriate concise comment. Include important inputs, return behavior, side effects, and failure conditions where relevant.
- For a feature spanning multiple files, explain each module's responsibility and any important interaction or ownership boundary near the relevant code.
- Explain non-obvious decisions, invariants, ordering requirements, coordinate conversions, and data-loss protections where they are enforced.
- Use TypeScript/JSDoc, Rust documentation comments, and Svelte/CSS comments as appropriate. Write clear, normal prose; persisted comments should not use compressed chat phrasing.
- Keep comments accurate when behavior changes. Avoid line-by-line narration, redundant syntax descriptions, or large summaries repeated across files.
- Record the completed comment review in the ticket's completion evidence along with checks and limitations.

## Proof strategy

- Do not run browser tests or visual tests: no browser automation, browser-based end-to-end tests, screenshot/snapshot comparisons, or manual visual inspection. This constraint applies to every ticket.

- Unit tests: document mapping round trips, attachment identity, annotation coordinate transforms, malformed inputs, revision conflicts, migrations.
- Integration tests: transaction failure, rapid edits across tabs, immediate close, attachment insertion/undo/restart, Trash restore, backup restore.
- Non-visual native functional checks on Windows and macOS: clipboard payloads, shortcut/IME event behavior, window close/quit, persistence, and offline startup. Assert high-DPI geometry numerically rather than inspecting rendered output.
- Run `bun run check`, `bun run build`, and relevant Rust checks/tests as implementation lands. Use non-browser integration harnesses for native platform verification.
- Measure cold startup, idle CPU/memory, and memory after cycling image-heavy tabs. Record hardware, OS, release configuration, dataset, and results.

No features are implemented by this document. Ticket completion requires recorded evidence, including any platform checks still pending.

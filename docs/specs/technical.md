# Technical specification

## Current repository

The repository contains a Tauri 2, SvelteKit, Svelte 5, TypeScript, and Rust starter. The page and Rust command implement the template greeting only. There is no editor, persistence, or note model. SvelteKit uses static output with SSR disabled. Tauri build commands currently use Bun. Reuse this structure.

## UI stack

Use Svelte 5, Tailwind CSS 4, and CodeMirror 6 within the existing SvelteKit and Tauri 2 application. Add Tailwind through its Vite integration. Tailwind is planned, not installed in the starter.

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

## Logical storage model

- `notes`: stable ID, title, document schema version, document JSON, language, revision, creation/update timestamps, optional deletion timestamp.
- `attachments`: stable ID, MIME type, original bytes, pixel width/height, creation timestamp.
- `note_attachments`: note/attachment references maintained in the same transaction as the document. Shared references are allowed; original bytes are immutable.
- `session`: versioned state containing open note IDs in order, pin state, active note ID, per-note selection/scroll, and UI preferences.
- Schema migrations use SQLite's version mechanism and run transactionally. Back up before a migration that could destroy or rewrite existing data.

Application code validates document schema, attachment references, dimensions, annotation numbers/text sizes, and serialized payload limits. Database constraints enforce IDs and relationships. Use parameterized SQL. Validate note ownership of image references in native commands; do not accept arbitrary filesystem paths from document data.

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

## Image handling and security

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

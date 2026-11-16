# Product specification

## Purpose

Capture text and screenshots immediately without choosing a file, saving manually, or creating an account. Keep the app small enough to leave open throughout the day. Restore work after closing the app.

## Confirmed direction

- Desktop app for Windows and macOS, with independent local data on each device.
- VS Code style text editing, rather than an Apple Notes style rich text editor.
- Svelte 5 and Tailwind CSS 4 provide the interface; CodeMirror 6 provides the editor. No component library is required initially.
- Multiple tabs, pinned tabs, pasted screenshots, and editable image annotations.
- Automatic persistence without a Save dialog.
- No accounts, server, cloud dependency, or synchronization.

The following details are first-release design defaults, chosen to make that direction concrete.

## Main workflow

1. Launch into the previous session. On first launch, create one empty note.
2. Type immediately in a monospace editor with line numbers and optional word wrap.
3. Paste a screenshot at the cursor. Show the actual image inline among the text.
4. Open the image's annotation view to draw or add text. Return to the same editor position afterward.
5. Create more notes, rename tabs, reorder them, or pin important notes.
6. Close tabs or quit without a save prompt. Reopen the app to continue.

## Editor

- CodeMirror 6 provides text editing, selection, undo/redo, indentation, search, and familiar shortcuts.
- Plain text is the default. Offer Markdown, JavaScript, TypeScript, JSON, and HTML highlighting as explicit language choices. Persist the choice per note.
- Markdown stays editable source text; do not add a separate rendered preview in this release.
- Support text before and after every inline image, including consecutive images and images at document boundaries.
- Images act as atomic document elements for selection, movement, deletion, and undo. Internal attachment tokens must never appear as normal editor content.
- Copying ordinary text produces ordinary text. Mixed text/image selections copy readable text with image placeholders; copying an image uses its own action.
- Default to a compact dark interface, with visible focus states and sufficient contrast. Match VS Code's general layout, not every VS Code feature.

## Notes and tabs

- New notes receive an automatic title such as `Untitled 1`; renaming is optional.
- Each note has at most one open tab. Opening an already open note activates it.
- Pinned tabs stay at the start. Reorder within the pinned or unpinned group; pin/unpin moves a tab between groups.
- Closing a tab only removes it from the open session. The note remains in All Notes.
- Pinned tabs can be closed explicitly; pinning affects placement, not retention or deletion protection.
- All Notes is a collapsible sidebar with title and plain-text content search. Search excludes annotation text in this release.
- Deleting a note moves it to Trash and closes its tab. Trash supports restore and explicit permanent deletion with confirmation.
- No automatic Trash expiry. Restored notes retain their content and can be reopened.
- If all tabs are closed, show an empty state with New Note and All Notes actions. Do not create endless blank notes.

## Screenshots and annotations

- Support native clipboard screenshot images on both OSes. Plain-text clipboard paste remains normal editor paste.
- For clipboard contents offering both image and text, prefer image insertion when an image is available.
- Validate image type, byte size, and decoded dimensions before insertion; reject unsupported or excessive images with a clear message.
- Resize only the displayed image to fit editor width. Preserve original pixels.
- Annotation tools: freehand pen, translucent highlighter, arrow, and text box; a small fixed color palette and stroke-width choices are sufficient.
- Select, move, and delete annotations. Text boxes remain editable. Provide undo/redo during annotation editing.
- Each completed stroke, move, or text edit is saved automatically. There is no annotation Save or Apply button.
- Escape exits annotation view. Completed edits remain; an unfinished pointer gesture can be canceled.
- Keep source image unchanged and annotations editable after restart.
- Copy Image and Export PNG produce a flattened image including annotations. Export asks for a destination because it creates a separate file.
- No standalone infinite canvas, image cropping, OCR, or screenshot capture tool in this release.

## Persistence and recovery

- Save edits continuously. No manual save command is needed for notes.
- Show a quiet save state: Saving, Saved, or an actionable failure. Never report Saved before a durable commit.
- Normal app exit flushes pending edits and session state without asking the user to save.
- If saving fails, keep unsaved work in memory, show retry/recovery actions, and block normal exit until it is saved or explicitly discarded.
- A forced process termination may lose only edits newer than the last durable commit. Target a maximum one-second commit delay during continuous text input.
- Restore open tabs, order, pin state, active tab, cursor/selection, scroll position, sidebar visibility, and word-wrap preference.
- Retain originals and attachments needed by undo during the current session. Removing an image must not immediately destroy its bytes.
- Keep automatic local backups and provide backup export and restore. Backups include images and annotations.
- Local app data is not encrypted by this app. Protect it with OS account and disk security.

## Keyboard and accessibility

- Use Command on macOS and Control on Windows for primary shortcuts.
- Support new note, close tab, reopen last closed tab, find in note, undo/redo, and sidebar toggle. Document exact bindings and avoid native webview conflicts.
- All controls have names and keyboard focus. Tabs expose active and pinned state.
- Annotation creation may use a pointer; tool selection, text entry, selection cycling, movement, and deletion must be keyboard accessible.
- Preserve IME input, native clipboard behavior, and high-DPI image alignment.

## Lightweight behavior

- Mount one active editor; keep inactive note state without one live editor or canvas per tab.
- Load images when needed, release unused decoded images, and avoid full-image copies during ordinary typing.
- No network traffic, polling loop, background service, or tray integration is needed for core behavior.
- Measure release startup and idle memory on both OSes before setting performance budgets. Compare a clean starter against the completed app under the same workload.

## Release acceptance

Verification must not use browser tests or visual tests, including manual visual inspection. Use programmatic assertions and non-visual native functional checks for the conditions below. Appearance and visual alignment remain unverified; do not claim they passed.

- On each OS: create two notes, pin/reorder tabs, type text, paste a native screenshot, annotate it, and exit immediately.
- Relaunch: content, image, editable annotations, and session state match the last successful saves.
- Close and reopen a tab without losing its note. Delete, restore, and permanently delete a separate test note.
- Copy and export an annotated image; the result matches the visible annotation composition.
- Simulate write failure and forced termination; errors remain visible and committed content survives.
- Restore a backup containing text, images, and annotations into a fresh test data directory.
- Complete the above offline without an account or manual note save.

## Out of scope

Sync, accounts, collaboration, mobile/web distribution, plugins, terminals, folders/projects, Git integration, AI, rich text formatting, and full VS Code compatibility.


## Compact editor interface

The main workspace prioritizes the editor: a compact header exposes New note, Paste screenshot, sidebar toggle, and an overflow menu. Tabs show the active note and pin marker, with close and action buttons. Right-clicking a tab opens the same actions: rename, pin/unpin, move left/right, and close. The sidebar spans the workspace from below the header to above the status bar, beside both tabs and editor. Notes use single file-style rows. Right-click, the context-menu key, or Shift+F10 opens rename/Trash actions; Trash rows offer restore/permanent deletion.

Find opens on demand from the menu or Ctrl/Cmd+F inside the editor. Escape in the search input closes it. Language and word wrap remain in the status bar beside save feedback. Backup and restore live in a modal settings dialog, retaining explicit replacement confirmation and recovery access after startup failure. No new account, sync, manual save, or UI framework is introduced. Ticket 010 remains postponed; ticket 011 records this interface enhancement.

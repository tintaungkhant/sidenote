# 004 — Build the editor shell, tabs, and session restore

Status: Implemented — native macOS/Windows interaction verification pending
Dependencies: 001, 003

## Outcome

A compact VS Code style shell supports persistent tabs without maintaining an editor per tab.

## Work

- Add dark shell, tab bar, optional sidebar region, and quiet save status.
- Build shell, tabs, sidebar, and toolbars with Tailwind CSS 4 utilities and shared design tokens. Keep editor themes and annotation canvas styling separate from shell utilities; do not add a component library.
- Support new notes, automatic titles, rename, activation, reorder, pin/unpin, close, and reopen last closed tab.
- Keep pinned tabs first and allow one open tab per note.
- Persist active tab, tab ordering/pins, selection, scroll, sidebar visibility, language, and word wrap.
- Create one note on first launch; show a useful empty state when all tabs are closed.
- Define and document native-safe macOS/Windows shortcuts.

## Acceptance

- [x] Closing any tab preserves its note; pinning does not alter this behavior.
- [x] Switching tabs preserves in-session undo history and view state.
- [x] Relaunch restores session state without duplicating notes or tabs.
- [x] Stale session references are ignored without deleting data.
- [x] Only the active editor is mounted; tab controls work by keyboard and expose accessible names/state.

## Proof

Test tab transitions, session normalization, immediate exit after reorder/pin, and keyboard event handling through non-browser unit/integration tests. Run frontend checks/build. Do not run browser or visual tests.

## Completion evidence

Implemented in `src/lib/tabs.ts` and `src/routes/+page.svelte`. Pure transitions normalize duplicate/stale IDs, preserve pinned-first ordering, prevent reorder across the pin boundary, and never delete notes. Startup reads metadata in bounded pages; a saved empty session remains empty. Note bodies and originals load on demand. Cached immutable CodeMirror states retain independent history while only one view is mounted. Closed tabs retain state for reopening during the current run; this stack is not persisted across launches.

The optional sidebar opens existing notes without duplicate tabs. Titles derive from the first nonblank text line, bounded to 80 Unicode characters. Explicit rename disables automatic titles during the run; on reload, a title matching the current derived title is treated as automatic. Language belongs to each note; wrapping and sidebar visibility belong to the workspace. Session selection and scroll survive restart; undo history survives switching within the current run only.

Tab operations are serialized, and close/quit waits for pending creation/loading before flushing. Pin/reorder stage the current session synchronously. Failed loads preserve the current editor. A 1000-tab guard matches native session validation. Error recovery includes all registered notes, including closed dirty tabs.

Headless proof covers normalization, uniqueness, closing/reopening, pin boundaries, order changes, title bounds, keyboard handling, independent cached undo/selection, and immediate flush after pin/reorder. Verification: `bun test` passed 27 tests (92 assertions); `bun run check` passed with zero errors/warnings; `bun run build` passed; `git diff --check` passed. The existing eager editor/language bundle warning remains (about 563 kB minified / 196 kB gzip). No browser or visual tests were run. Native pointer, keyboard, scroll, and platform shortcut behavior still require user verification.

## Shortcuts

Use Ctrl on Windows and Cmd on macOS for New (`N`), Close tab (`W`), Reopen last closed (`Shift+T`), Sidebar (`B`), Find (`F`), and guarded Quit (`Q`). `Ctrl+Tab` / `Ctrl+Shift+Tab` cycles tabs on both platforms without claiming macOS Cmd+Tab. Editor Undo/Redo keep CodeMirror platform bindings. Tab-bar Left/Right/Home/End activates and focuses tabs. All pin/reorder/rename/close controls expose text or accessible names and work with ordinary keyboard button activation. IME composition and Alt combinations are ignored by application shortcut handling. Native window shortcuts such as Alt+F4 remain owned by the OS and use the existing guarded close path.

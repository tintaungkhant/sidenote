# 011 — Compact editor interface

Status: Done (implementation and headless verification; rendered interactions unverified)
Dependencies: 001–009

## Outcome

A compact VS Code style shell keeps the editor prominent while preserving all existing note, image, tab, autosave, recovery, and backup operations. Ticket 010 is postponed at the user's request; this work does not claim its release verification.

## Acceptance

- [x] Header contains sidebar toggle, New note, Paste screenshot, and an overflow menu.
- [x] Active tabs have an accent border; pinned tabs have a marker. Close and action controls have accessible names.
- [x] Tab action button and right-click expose rename, pin/unpin, reorder, and close through existing serialized operations.
- [x] Sidebar spans the full workspace beside tabs and editor. All Notes/Trash and explicit search remain. Compact 28px file rows expose actions through right-click or the context-menu key / Shift+F10; Trash rows also open actions when clicked.
- [x] Find opens on demand, including Ctrl/Cmd+F in the editor; Escape in its input closes it.
- [x] Status bar contains save feedback, language, and wrapping preferences.
- [x] Backup settings use a native modal dialog, with pending-work and restore-confirmation dismissal guards. Startup recovery opens the same dialog.
- [x] Existing autosave, exit barriers, Trash confirmation, annotations, and storage APIs are reused. No new dependencies or native storage changes.
- [x] Function and cohesive-file comments reviewed for menu ownership, focus timing, native dismissal, and the single-editor invariant.

## Verification

- `bun run check`: zero errors and warnings.
- `bun test`: 38 passing tests, 161 assertions. Existing headless coverage includes tab transitions, independent history, ordered autosave, and exit behavior.
- `bun run build`: passed; existing large editor chunk warning remains (approximately 586 kB minified).
- `git diff --check`: passed.
- Browser tests, visual tests, screenshots, DOM inspection, and manual visual inspection were not performed.
- Follow-up: moved the explorer outside the editor column, removed reserved hidden-action space, and reused serialized rename/lifecycle operations from row context menus. Reviewed comments for shared menu positioning, focus after rendering, and stable right-click identity.
- Native dialog/popover interactions, rendered focus and layout, right-click behavior, and macOS execution remain unverified. Menus use the platform WebView's native Popover API; older WebViews without that API are not verified.

## Scope boundary

No backend change, release packaging, security audit, cross-platform performance measurement, sync, or account feature. Ticket 010 remains postponed.

## Image layout rollback

The attempted standalone block-widget changes were reverted at the user's request after missing first-line numbers and cursor regressions were reported. Image presentation is restored exactly to the previous inline-widget implementation. The two tests for the abandoned block-decoration behavior were removed. The compact shell and full-height sidebar enhancements remain.

Known limitation: after Enter immediately following an image, an empty second line can overlap the image until text is entered. This is the fallback explicitly accepted by the user. No further image-layout change is included. Existing image source, annotations, and note data are unchanged.

Rollback verification: `bun run check` reports zero errors/warnings; `bun test` passes 38 tests; `git diff --check` passes. The image presentation file matches HEAD exactly. No browser or visual checks were performed.

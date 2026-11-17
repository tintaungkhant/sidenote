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

## Image editor presentation follow-up

Replaced the flat annotation toolbar with a titled header and separate output actions, a left drawing-tool rail with SVG icons and shortcut hints, and a properties bar with named color swatches, stroke width, and history controls. The scrollable canvas uses a distinct background and image shadow. Selected text has a dedicated editing strip; save feedback and original image dimensions appear in the footer. Narrow-window styles retain all controls.

Existing pointer coordinates, annotation history, immediate persistence, image output, modal keyboard handling, and object-URL ownership are reused. No dependencies or note-image layout changes were added. Reviewed comments for presentation ownership and coordinate invariants.

Verification: `bun run check` reports zero errors/warnings; `bun test` passes 38 tests; `bun run build` and `git diff --check` pass. Existing large editor chunk warning remains. No browser or visual tests were performed; rendered layout and native annotation interactions remain unverified.


## In-place captions and object handles

Removed the white drop-shadow selection effect; highlighter opacity and annotation colors remain unchanged. Selected objects now show a blue box, a move handle, and a bottom-right proportional resize handle. Resizing scales path geometry/stroke width or text font size within existing native limits, with immutable previews and one completed operation in annotation history. Selection chrome never enters normal inline rendering or PNG output.

Text placement creates and focuses a native textarea at the caption's source-image coordinates inside SVG foreignObject. Double-clicking existing text reopens it; keyboard users can select a caption with Tab and press Enter. Escape returns from caption editing to canvas selection. Multiline text, native caret/selection/IME, and the existing 64 KiB text validation are retained; each input continues immediate persistence. The top text box is removed. Reviewed comments for focus, coordinate ownership, source immutability, output exclusion, and native limits.

Verification: `bun run check` passes with zero errors/warnings; `bun test` passes 41 tests (176 assertions), including new selection/output separation, geometry/style limits, source immutability, and empty/multiline caption bounds. `bun run build` and `git diff --check` pass; the existing large editor chunk warning remains. No browser or visual tests were run. Native foreignObject focus, IME interaction, double-click behavior, pointer handles, and rendered alignment remain unverified. Text selection bounds use an approximate font-width estimate rather than measured glyph bounds; resizing is proportional from the top-left, not free text-box reflow.

### Caption width and double-click correction

The in-place textarea now uses the remaining image width from its anchor, rather than growing from an estimated character count. Selection chrome is hidden while typing so its approximate bounds cannot appear as growing right padding. Caption height still follows explicit newline count; horizontal overflow remains native when text exceeds the image boundary.

Pointer capture and SVG selection redraws can retarget native double-click events to the canvas. Completed caption clicks now recognize a same-object pair within 500 ms and 4 CSS pixels before relying on the native double-click fallback. Selection-only clicks do not commit a move or redraw geometry. Native double-click also falls back to the currently selected caption. Reviewed comments for click routing, undo preservation, and internal sizing.

Checks: `bun run check` passes with zero errors/warnings; `bun test` passes 41 tests (177 assertions), including selection chrome suppression during text entry; `bun run build` and `git diff --check` pass. No browser or visual checks. Actual pointer timing and native text-input interactions remain unverified.

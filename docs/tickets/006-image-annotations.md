# 006 — Draw and write on images

Status: Todo
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

- [ ] Drawing and text align with the same image pixels after resize and restart.
- [ ] Original attachment bytes remain unchanged.
- [ ] Two image blocks using one original have independent annotation state.
- [ ] Exiting the view retains completed edits and cancels only an unfinished gesture.
- [ ] Text editing and undo do not accidentally trigger note-level shortcuts.
- [ ] Save failure preserves the current annotation work and shows the existing failure state.

## Proof

Test geometry transforms, serialization, history, keyboard event handling, and restart persistence without a browser. Assert high-DPI coordinate mapping numerically. Do not run visual alignment checks; record rendered alignment as unverified.

## Completion evidence

Pending.

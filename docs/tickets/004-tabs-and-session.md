# 004 — Build the editor shell, tabs, and session restore

Status: Todo
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

- [ ] Closing any tab preserves its note; pinning does not alter this behavior.
- [ ] Switching tabs preserves in-session undo history and view state.
- [ ] Relaunch restores session state without duplicating notes or tabs.
- [ ] Stale session references are ignored without deleting data.
- [ ] Only the active editor is mounted; tab controls work by keyboard and expose accessible names/state.

## Proof

Test tab transitions, session normalization, immediate exit after reorder/pin, and keyboard event handling through non-browser unit/integration tests. Run frontend checks/build. Do not run browser or visual tests.

## Completion evidence

Pending.

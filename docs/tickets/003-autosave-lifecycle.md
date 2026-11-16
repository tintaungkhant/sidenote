# 003 — Make saving automatic and exit safe

Status: Implemented — macOS and native quit verification pending
Dependencies: 002

## Outcome

Users can quit without saving manually, and acknowledged writes cannot be overwritten by stale asynchronous saves.

## Work

- Add per-note dirty/revision state and one ordered native write queue.
- Coalesce text writes with a maximum one-second commit-attempt delay during continuous input.
- Show Saving, Saved, and actionable error states tied to actual commit acknowledgements.
- Intercept close and quit, flush dirty notes/session state, and guard repeated close events.
- Keep unsaved work available on failure; offer retry, recovery export, and explicit discard before failed-save exit.
- Enforce one application instance per data directory; subsequent launches focus the first.

## Acceptance

- [x] Continuous typing does not postpone persistence indefinitely.
- [x] Rapid edits and delayed responses cannot replace newer content or falsely show Saved.
- [x] Normal immediate exit after typing persists the final edit without a save prompt.
- [x] Injected write failure keeps the app open and the latest content in memory.
- [x] Forced termination preserves all acknowledged revisions.
- [x] A second launch does not create a competing writer.

## Proof

Test delayed/failed writes and repeated close events. Manually verify window close and explicit quit on both OSes; record any unavailable platform checks.

## Completion evidence

Implemented automatic persistence for the current single-note editor, a FIFO native storage worker, acknowledgement-based save status, guarded close/quit, single-instance handling, and emergency recovery export. Normal editing requires no Save action. The editor reloads its persisted note, language, and session preferences. The bundled image fixture now stores a validated PNG original before its document reference.

Validation completed on Windows:

- `bun test`: 20 passed, 0 failed, including sustained typing, delayed acknowledgements, per-note revisions, session changes during a write, failed-write retention/retry, immediate image/annotation scheduling, repeated close, final-keystroke flush, and confirmed discard.
- `cargo test --manifest-path src-tauri/Cargo.toml --lib`: 18 passed, 0 failed. New tests cover FIFO ordering despite reversed awaits, queue continuation after a failed operation, close-state transitions, startup close before renderer readiness, recovery source/original export, missing-original reporting, source-only export without a store, and forced process termination with a live WAL connection. The forced-kill test reopens acknowledged note, image, and session commits successfully. One native test entry is the subprocess helper and returns immediately during the parent test run.
- Non-visual Windows process check: built an isolated debug app with a unique probe identifier and application-data directory. A secondary launch exited successfully while the first process retained one note. Two native `WM_CLOSE` messages completed the renderer flush and exited successfully. Relaunch followed by immediate native close also exited successfully and retained the existing note/session. Checks inspected only process exit codes, native window handles/messages, and SQLite state; no DOM, screenshots, or rendered UI were inspected.
- The startup-close check exposed a stall before renderer initialization. The final lifecycle explicitly admits renderer work only after its listener readiness handshake. Native close before that point exits directly because no editable workspace state can exist; later closes always use the flush handshake. The corrected Windows process check passed.
- `cargo check --manifest-path src-tauri/Cargo.toml`: passed.
- `cargo fmt --manifest-path src-tauri/Cargo.toml --check`: passed.
- `bun run check`: 0 errors, 0 warnings.
- `bun run build`: passed. Existing eager editor/language bundle warning remains, now approximately 555 kB minified / 193 kB gzip.
- `bun run tauri build --debug --no-bundle --config <temporary-probe-config>` with an isolated `CARGO_TARGET_DIR`: passed. The separate directory avoided replacing a debug executable held by an already running app.
- `git diff --check`: passed.
- Reviewed comments for each new/changed function and cohesive file group: generation/revision ownership, timer deadlines, queue ordering, startup admission, repeated close, composition freeze, asynchronous image insertion, recovery export, and object-URL cleanup are documented near the code.

No browser or visual tests were run. macOS execution, native explicit Quit/menu shortcuts, native IME close behavior, and save-failure/recovery dialog interaction have not been exercised on a real window. Failure, retry, final-edit, and discard transitions are verified through headless tests; ordinary and startup window close plus single-instance behavior are verified through the Windows process check. Those remaining OS checks must be recorded during ticket 010 before cross-platform release. Full tabs/session normalization remain ticket 004, screenshot paste remains ticket 005, and SQLite backup/restore remains ticket 009.

The fixed batch delay is 250 ms from the first edit and is not reset by ongoing typing. Slow or blocked native I/O may take longer than one second to acknowledge; new generations wait for the current revision and then submit immediately rather than competing with stale revisions.

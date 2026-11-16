# 009 — Add local backup and safe restore

Status: Done (native picker and macOS interaction remain unverified)
Dependencies: 003, 008

## Outcome

Users can recover their complete local workspace without an account or cloud service.

## Work

- Create consistent SQLite snapshots through the backup API, including image BLOBs.
- After the first successful write each day, attempt an automatic backup; retain seven successful daily snapshots.
- Add manual backup export and restore actions with native file pickers.
- Validate integrity and schema compatibility before restore; explain that restore replaces local data and request confirmation.
- Flush pending work, make a pre-restore backup, and replace/reopen with rollback on failure.
- Surface backup failures and corrupted-store recovery without silently starting an empty replacement store.

## Acceptance

- [x] Backup during active use yields a readable, internally consistent snapshot.
- [x] Restoring into a fresh data directory recovers notes, Trash, images, annotations, and session state.
- [x] Corrupt or unsupported backups leave the current store untouched.
- [x] Failed replacement/reopen can return to the pre-restore store.
- [x] Retention never deletes the last valid backup before a new snapshot succeeds.
- [x] Automatic and manual snapshot creation run off the UI thread without freezing editor input. Live typing latency remains unmeasured.

## Proof

Run backup/restore integration tests with WAL activity, injected failures, corrupt input, and a representative image-heavy fixture.

## Completion evidence

- Enabled the backup feature of the existing bundled `rusqlite` dependency; no new dependency or schema migration. SQLite's online backup API copies committed WAL pages and BLOBs into a temporary standalone database. Integrity, foreign keys, and exact schema are checked before atomic publication.
- After the first successful write of each UTC day, the FIFO worker attempts `backups/daily-{epoch-day:010}.sqlite3` under app data. It retries failures after later successful writes, recognizes existing successful daily files after restart, and retains seven valid daily snapshots only after a new snapshot succeeds. Other files and pre-restore archives are not collected. Write acknowledgements remain independent of backup results.
- Added Backup and restore controls with native save/open pickers and separate automatic-backup warnings. Manual export flushes current edits and keeps typing available; app-data destinations are rejected to protect the live store and rollback files. Native close waits for active export/restore work.
- Restore privately stages and validates full notes, references, decoded images, annotations, and session bounds before showing counts and an explicit replacement warning. A one-use native token identifies the staged copy; cancellation releases it. Emergency recovery JSON is not accepted as a SQLite backup.
- Confirmation freezes input, waits for editing/tab operations, and flushes autosave. Native code creates a consistent pre-restore snapshot, closes SQLite, archives original main/WAL/SHM/journal files, and publishes a recovery marker before replacement/reopen. Failure rolls back originals; startup retries interrupted recovery before opening SQLite. An incomplete archive fails closed without deleting current files.
- Restored note/session revisions advance above both workspaces, including absent sessions, rejecting stale requests even if a success acknowledgement is lost. Successful restore discards old editor states, undo histories, blob caches, and revisions before loading the restored workspace.
- Unreadable startup now leaves the app open with its error and a recovery action. It never silently initializes an empty replacement. Confirmed recovery preserves unreadable originals verbatim when a valid pre-restore SQLite snapshot cannot be made.
- Headless Rust integration tests cover an eight-original image-heavy WAL fixture, full restoration into a fresh profile, staged-byte isolation, corrupt/schema/document/image/reference/session rejection, publication failure, retention, injected reopen failure, interrupted recovery, stale revisions, automatic-failure retry, and corrupt-startup recovery. All fixtures use temporary directories; no native picker, real profile, or user clipboard is touched.
- `cargo test --manifest-path src-tauri/Cargo.toml --lib`: 35 passed, one live clipboard test ignored. `bun test`: 38 passed. `bun run check`: zero errors/warnings. `bun run build`: passed with the existing editor chunk warning. Rust formatting and `git diff --check` passed.
- Reviewed function/module comments for shared validation, publication, retention, staging, revision invalidation, replacement/rollback, queue acknowledgements, IPC ownership, frontend flushing, and cleanup.
- No browser or visual tests ran. Native picker/confirmation/focus interaction, macOS execution, real power-loss/filesystem faults, and large-library backup/typing latency remain unverified. Interrupted recovery is tested through simulated filesystem state. Daily snapshots require local disk space; pre-restore archives remain without automatic pruning.

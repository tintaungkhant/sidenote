# 009 — Add local backup and safe restore

Status: Todo
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

- [ ] Backup during active use yields a readable, internally consistent snapshot.
- [ ] Restoring into a fresh data directory recovers notes, Trash, images, annotations, and session state.
- [ ] Corrupt or unsupported backups leave the current store untouched.
- [ ] Failed replacement/reopen can return to the pre-restore store.
- [ ] Retention never deletes the last valid backup before a new snapshot succeeds.
- [ ] Backup work does not block normal typing.

## Proof

Run backup/restore integration tests with WAL activity, injected failures, corrupt input, and a representative image-heavy fixture.

## Completion evidence

Pending.

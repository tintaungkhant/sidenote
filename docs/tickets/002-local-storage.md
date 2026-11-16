# 002 — Add local SQLite storage

Status: Todo
Dependencies: 001

## Outcome

Notes and immutable image originals persist in an app-owned SQLite database through narrow, validated Rust commands.

## Work

- Select a maintained SQLite dependency and create the store under the OS application-data directory.
- Add schema versioning, note/session storage, attachment BLOBs, and reference constraints from the technical spec.
- Implement create/load/update/list operations with parameterized queries and revision checks.
- Validate documents, attachment references, payload sizes, and native command inputs.
- Return structured errors. Never silently reset an unreadable database.
- Remove the greeting command when replacing the template integration.

## Acceptance

- [ ] A saved document and its attachment bytes survive closing and reopening the store.
- [ ] Invalid documents, missing attachment references, and stale revisions are rejected without partial changes.
- [ ] List queries do not load attachment BLOBs or every note's full content unnecessarily.
- [ ] Schema initialization and migration failures preserve existing data.
- [ ] Tests use isolated temporary data directories.

## Proof

Run native storage tests for round trips, rollback, validation, and revision conflicts. Run Rust formatting/checks and frontend type checks for command contracts.

## Completion evidence

Pending.

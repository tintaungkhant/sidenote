# 002 — Add local SQLite storage

Status: Done
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

- [x] A saved document and its attachment bytes survive closing and reopening the store.
- [x] Invalid documents, missing attachment references, and stale revisions are rejected without partial changes.
- [x] List queries do not load attachment BLOBs or every note's full content unnecessarily.
- [x] Schema initialization and migration failures preserve existing data.
- [x] Tests use isolated temporary data directories.

## Proof

Run native storage tests for round trips, rollback, validation, and revision conflicts. Run Rust formatting/checks and frontend type checks for command contracts.

## Completion evidence

Implemented native storage, Tauri command bindings, and typed frontend wrappers. Removed the template greeting command and unused opener dependency/capability. SQLite lives in the OS application-data directory, uses WAL/FULL durability, and never accepts renderer-provided paths.

Validation completed on Windows:

- `cargo test --manifest-path src-tauri/Cargo.toml --lib`: 10 passed, 0 failed. Every integration test uses an isolated temporary directory. Coverage includes document/image/session reopening, stale writes through two connections, missing references, invalid document and annotation inputs, payload limits, shared immutable originals, retained unreferenced bytes, metadata-only listing, injected transaction failure, transactional initialization rollback, unsupported schemas, and corrupt-file preservation.
- `cargo check --manifest-path src-tauri/Cargo.toml`: passed.
- `cargo fmt --manifest-path src-tauri/Cargo.toml --check`: passed.
- `bun test`: 11 passed, 0 failed.
- `bun run check`: 0 errors, 0 warnings; includes the new frontend command contracts.
- `bun run build`: passed. The existing editor bundle-size warning remains (approximately 543 kB minified).
- `git diff --check`: passed.
- Reviewed comments for each storage function, command, frontend wrapper, and test. Comments document module ownership, input limits, revision checks, transaction boundaries, reference access, and data preservation.

No browser or visual tests were run. macOS execution and installed-app IPC checks are not verified here. Initialization is the only migration currently implemented; future existing-data rewrites require a backup before migration. Autosave/editor wiring, exit flushing, and ordered save scheduling remain ticket 003; tab/session restoration remains ticket 004. The current editor still uses temporary content.

//! All fixtures live in temporary directories; no test touches user-selected files or the real workspace.
use super::*;
use image::ImageEncoder;

/// Distinct incompressible 256px originals exercise multi-page BLOB backup without external image fixtures.
fn image(seed: u32) -> Vec<u8> {
    let mut value = seed;
    let pixels: Vec<u8> = (0..256 * 256 * 4)
        .map(|_| {
            value = value.wrapping_mul(1664525).wrapping_add(1013904223);
            (value >> 24) as u8
        })
        .collect();
    let mut bytes = Vec::new();
    image::codecs::png::PngEncoder::new(&mut bytes)
        .write_image(&pixels, 256, 256, image::ExtendedColorType::Rgba8)
        .unwrap();
    bytes
}

/// Include annotations and literal source characters so restore proves more than metadata preservation.
fn input(attachment: Option<Uuid>, title: &str) -> NoteInput {
    let mut blocks = vec![Block::Text {
        text: "source\r\n\u{fffc}".into(),
    }];
    if let Some(attachment_id) = attachment {
        blocks.push(Block::Image {
            id: Uuid::new_v4(),
            attachment_id,
            annotations: vec![Annotation::Text {
                x: 20.0,
                y: 24.0,
                text: "editable caption".into(),
                color: "#ffffff".into(),
                font_size: 12.0,
            }],
        });
    }
    NoteInput {
        title: title.into(),
        language: "markdown".into(),
        document: Document { version: 1, blocks },
    }
}

/// Session recovery preserves pinning, active membership, view positions, and preferences.
fn session(id: Uuid) -> Session {
    Session {
        version: 1,
        tabs: vec![Tab {
            note_id: id,
            pinned: true,
            anchor: 3,
            head: 5,
            scroll_top: 42.5,
        }],
        active_note_id: Some(id),
        sidebar_visible: true,
        word_wrap: false,
    }
}

/// Back up committed WAL pages while a connection stays active, then restore all payloads into a fresh profile.
#[test]
fn wal_snapshot_and_complete_fresh_restore() {
    let source_dir = tempfile::tempdir().unwrap();
    let target_dir = tempfile::tempdir().unwrap();
    let export_dir = tempfile::tempdir().unwrap();
    let mut source = Store::open(source_dir.path()).unwrap();
    source
        .connection
        .pragma_update(None, "wal_autocheckpoint", 0)
        .unwrap();
    let mut originals = Vec::new();
    let mut notes = Vec::new();
    for seed in 1..=8 {
        let bytes = image(seed);
        let attachment = source.put_attachment(bytes.clone()).unwrap();
        let note = source
            .create_note(input(Some(attachment.id), &format!("Image {seed}")))
            .unwrap();
        originals.push((attachment.id, bytes));
        notes.push(note);
    }
    source
        .change_note_lifecycle(notes[1].summary.id, 1, "trash".into())
        .unwrap();
    source
        .save_session(0, session(notes[0].summary.id))
        .unwrap();
    assert!(
        fs::metadata(source_dir.path().join("notes.sqlite3-wal"))
            .unwrap()
            .len()
            > 0
    );
    let backup = export_dir.path().join("workspace.sqlite3");
    source.export_backup(&backup).unwrap();
    source.create_note(input(None, "After snapshot")).unwrap();
    let candidate = Store::prepare_restore(&backup, &target_dir.path().join("backups")).unwrap();
    assert_eq!((candidate.notes, candidate.trashed), (7, 1));
    let mut target = Ok(Store::open(target_dir.path()).unwrap());
    replace_store(target_dir.path(), &mut target, candidate, false).unwrap();
    let target = target.unwrap();
    assert_eq!(target.list_notes(100, 0).unwrap().len(), 7);
    for (index, note) in notes.iter().enumerate() {
        let loaded = target.load_note(note.summary.id).unwrap();
        assert_eq!(loaded.document, note.document);
        assert_eq!(loaded.summary.deleted_at.is_some(), index == 1);
        assert_eq!(
            target
                .load_attachment(note.summary.id, originals[index].0)
                .unwrap()
                .bytes,
            originals[index].1
        );
    }
    let loaded = target.load_session().unwrap().state.unwrap();
    assert_eq!(
        serde_json::to_value(loaded).unwrap(),
        serde_json::to_value(session(notes[0].summary.id)).unwrap()
    );
    assert!(!target_dir.path().join(RESTORE_MARKER).exists());
    drop(target);
    assert_eq!(
        Store::open(target_dir.path())
            .unwrap()
            .browse_notes(true, "".into(), 0)
            .unwrap()
            .len(),
        1
    );
}

/// Candidate validation rejects physical, schema, document, image, reference, and session damage before replacement.
#[test]
fn invalid_backups_never_touch_current_store() {
    let current_dir = tempfile::tempdir().unwrap();
    let export_dir = tempfile::tempdir().unwrap();
    let mut current = Store::open(current_dir.path()).unwrap();
    let attachment = current.put_attachment(image(9)).unwrap();
    let note = current
        .create_note(input(Some(attachment.id), "Keep me"))
        .unwrap();
    let original = note.document.clone();
    let path = export_dir.path().join("bad.sqlite3");
    for sql in [
        "PRAGMA user_version=99",
        "PRAGMA user_version=0",
        "CREATE TRIGGER bad AFTER UPDATE ON notes BEGIN DELETE FROM attachments; END",
        "CREATE TABLE sqliteEvil(value)",
        "UPDATE notes SET document='{}'",
        "UPDATE attachments SET bytes=x'010203'",
        "UPDATE attachments SET width=257",
        "DELETE FROM note_attachments",
        "INSERT INTO session VALUES(1,1,'{\"version\":99,\"tabs\":[],\"activeNoteId\":null,\"sidebarVisible\":false,\"wordWrap\":true}',1)",
    ] {
        current.snapshot(&path).unwrap();
        let damaged = Connection::open(&path).unwrap();
        damaged.execute_batch(sql).unwrap();
        drop(damaged);
        assert!(
            Store::prepare_restore(&path, &current_dir.path().join("backups")).is_err(),
            "accepted {sql}"
        );
        assert_eq!(
            current.load_note(note.summary.id).unwrap().document,
            original
        );
    }
    fs::write(&path, b"not a database").unwrap();
    assert!(Store::prepare_restore(&path, &current_dir.path().join("backups")).is_err());
    assert_eq!(
        current.load_note(note.summary.id).unwrap().document,
        original
    );
    assert!(!current_dir.path().join(RESTORE_MARKER).exists());
}

/// Reopening failure restores the exact previous note/session and leaves a readable pre-restore snapshot.
#[test]
fn reopen_failure_rolls_back() {
    let original_dir = tempfile::tempdir().unwrap();
    let replacement_dir = tempfile::tempdir().unwrap();
    let mut original = Store::open(original_dir.path()).unwrap();
    let note = original.create_note(input(None, "Original")).unwrap();
    original.save_session(0, session(note.summary.id)).unwrap();
    let mut replacement = Store::open(replacement_dir.path()).unwrap();
    replacement.create_note(input(None, "Replacement")).unwrap();
    let candidate = Store::prepare_restore(
        &replacement_dir.path().join("notes.sqlite3"),
        &original_dir.path().join("backups"),
    )
    .unwrap();
    let mut original = Ok(original);
    assert!(replace_store(original_dir.path(), &mut original, candidate, true).is_err());
    let original = original.unwrap();
    assert_eq!(
        original.load_note(note.summary.id).unwrap().summary.title,
        "Original"
    );
    assert_eq!(
        original
            .load_session()
            .unwrap()
            .state
            .unwrap()
            .active_note_id,
        Some(note.summary.id)
    );
    assert!(!original_dir.path().join(RESTORE_MARKER).exists());
    let archive = fs::read_dir(original_dir.path().join("backups"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| {
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("pre-restore-")
        })
        .unwrap();
    assert!(
        Store::prepare_restore(&archive.join("workspace.sqlite3"), replacement_dir.path()).is_ok()
    );
}

/// Retention never prunes before publication succeeds, ignores unrelated files, and keeps seven valid daily snapshots.
#[test]
fn daily_retention_and_failed_publication() {
    let directory = tempfile::tempdir().unwrap();
    let mut store = Store::open(directory.path()).unwrap();
    store.create_note(input(None, "Retain")).unwrap();
    for day in 1..=8 {
        store.daily_backup(day).unwrap();
    }
    let backups = directory.path().join("backups");
    let before = fs::read(backups.join("daily-0000000008.sqlite3")).unwrap();
    store.create_note(input(None, "Later today")).unwrap();
    store.daily_backup(8).unwrap();
    assert_eq!(
        fs::read(backups.join("daily-0000000008.sqlite3")).unwrap(),
        before
    );
    assert!(!backups.join("daily-0000000001.sqlite3").exists());
    let blocker = backups.join("daily-0000000009.sqlite3");
    fs::create_dir(&blocker).unwrap();
    fs::write(backups.join("personal.txt"), b"keep").unwrap();
    assert!(store.daily_backup(9).is_err());
    for day in 2..=8 {
        assert!(backups.join(format!("daily-{day:010}.sqlite3")).is_file());
    }
    fs::remove_dir(blocker).unwrap();
    store.daily_backup(9).unwrap();
    assert!(!backups.join("daily-0000000002.sqlite3").exists());
    assert_eq!(fs::read(backups.join("personal.txt")).unwrap(), b"keep");
    assert!(
        store
            .export_backup(&directory.path().join("notes.sqlite3"))
            .is_err()
    );
    assert_eq!(store.list_notes(100, 0).unwrap().len(), 2);
}

/// A confirmed candidate is immutable even if the original selected file is changed before replacement.
#[test]
fn candidate_bytes_are_private_until_confirmation() {
    let source_dir = tempfile::tempdir().unwrap();
    let target_dir = tempfile::tempdir().unwrap();
    let mut source = Store::open(source_dir.path()).unwrap();
    let note = source.create_note(input(None, "Selected")).unwrap();
    let candidate =
        Store::prepare_restore(&source_dir.path().join("notes.sqlite3"), target_dir.path())
            .unwrap();
    source
        .update_note(note.summary.id, 1, input(None, "Changed later"))
        .unwrap();
    let mut target = Ok(Store::open(target_dir.path()).unwrap());
    replace_store(target_dir.path(), &mut target, candidate, false).unwrap();
    let mut target = target.unwrap();
    assert_eq!(
        target.load_note(note.summary.id).unwrap().summary.title,
        "Selected"
    );
    assert!(target.save_session(0, session(note.summary.id)).is_err());
}

/// Lost restore acknowledgements cannot let a stale renderer overwrite restored content with matching old revisions.
#[test]
fn restore_invalidates_pre_restore_note_and_session_revisions() {
    let directory = tempfile::tempdir().unwrap();
    let exports = tempfile::tempdir().unwrap();
    let mut store = Store::open(directory.path()).unwrap();
    let note = store.create_note(input(None, "Backup content")).unwrap();
    store.save_session(0, session(note.summary.id)).unwrap();
    let backup = exports.path().join("backup.sqlite3");
    store.snapshot(&backup).unwrap();
    store
        .update_note(note.summary.id, 1, input(None, "Before restore"))
        .unwrap();
    let candidate = Store::prepare_restore(&backup, &directory.path().join("backups")).unwrap();
    let mut store = Ok(store);
    replace_store(directory.path(), &mut store, candidate, false).unwrap();
    let mut store = store.unwrap();
    assert!(
        store
            .update_note(note.summary.id, 1, input(None, "Stale backup revision"))
            .is_err()
    );
    assert!(
        store
            .update_note(note.summary.id, 2, input(None, "Stale current revision"))
            .is_err()
    );
    assert!(store.save_session(1, session(note.summary.id)).is_err());
    let loaded = store.load_note(note.summary.id).unwrap();
    assert_eq!(loaded.summary.title, "Backup content");
    store
        .update_note(
            note.summary.id,
            loaded.summary.revision,
            input(None, "New edit"),
        )
        .unwrap();
}

/// Startup retries an interrupted replacement; incomplete recovery archives fail closed without deleting live files.
#[test]
fn interrupted_restore_recovery() {
    let directory = tempfile::tempdir().unwrap();
    let mut store = Store::open(directory.path()).unwrap();
    let note = store
        .create_note(input(None, "Before interruption"))
        .unwrap();
    drop(store);
    let name = format!("pre-restore-{}", Uuid::new_v4());
    let archive = directory.path().join("backups").join(&name);
    fs::create_dir_all(&archive).unwrap();
    durable_copy(
        &directory.path().join("notes.sqlite3"),
        &archive.join("notes.sqlite3"),
    )
    .unwrap();
    fs::write(archive.join("original-files.json"), b"[\"notes.sqlite3\"]").unwrap();
    fs::write(directory.path().join(RESTORE_MARKER), &name).unwrap();
    fs::write(
        directory.path().join("notes.sqlite3"),
        b"partial replacement",
    )
    .unwrap();
    recover_interrupted(directory.path()).unwrap();
    assert_eq!(
        Store::open(directory.path())
            .unwrap()
            .load_note(note.summary.id)
            .unwrap()
            .summary
            .title,
        "Before interruption"
    );
    fs::write(
        directory.path().join(RESTORE_MARKER),
        format!("pre-restore-{}", Uuid::new_v4()),
    )
    .unwrap();
    let bytes = fs::read(directory.path().join("notes.sqlite3")).unwrap();
    assert!(recover_interrupted(directory.path()).is_err());
    assert_eq!(
        fs::read(directory.path().join("notes.sqlite3")).unwrap(),
        bytes
    );
}

//! Isolated SQLite integration tests. Every store lives in a fresh temporary directory.
use super::*;
use image::ImageEncoder;

/// Every tool survives an acknowledged edit and restart, with independent placements and unchanged originals.
#[test]
fn annotation_edit_restart() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(dir.path()).unwrap();
    let original = store.put_attachment(png()).unwrap();
    let note = store.create_note(with_image(original.id)).unwrap();
    let mut edited = with_image(original.id);
    let annotations = serde_json::json!([
        {"kind":"pen","points":[{"x":0.25,"y":0.5},{"x":0.75,"y":1.0}],"color":"#ff4444","width":4},
        {"kind":"highlighter","points":[{"x":0.0,"y":0.5},{"x":1.0,"y":0.5}],"color":"#ffff00","width":16},
        {"kind":"arrow","points":[{"x":0.0,"y":0.0},{"x":1.0,"y":1.0}],"color":"#3399ff","width":2},
        {"kind":"rectangle","points":[{"x":1.0,"y":1.0},{"x":0.0,"y":0.25}],"color":"#3399ff","width":2},
        {"kind":"circle","points":[{"x":1.0,"y":1.0},{"x":0.0,"y":0.0}],"color":"#ff4444","width":2},
        {"kind":"text","x":0.5,"y":0.5,"text":"edited\ncaption","color":"#ffffff","fontSize":24}
    ]);
    if let Block::Image {
        annotations: target,
        ..
    } = &mut edited.document.blocks[1]
    {
        *target = serde_json::from_value(annotations).unwrap();
    }
    let independent = with_image(original.id).document.blocks.pop().unwrap();
    edited.document.blocks.push(independent.clone());
    let expected = edited.document.clone();
    store.update_note(note.summary.id, 1, edited).unwrap();
    drop(store);
    let reopened = Store::open(dir.path()).unwrap();
    let loaded = reopened.load_note(note.summary.id).unwrap();
    assert_eq!(loaded.document, expected);
    assert_eq!(loaded.document.blocks[2], independent);
    assert_eq!(
        reopened
            .load_attachment(note.summary.id, original.id)
            .unwrap()
            .bytes,
        png()
    );
}

/// Build a note fixture whose source text includes CRLF and literal editor marker characters.
fn input() -> NoteInput {
    NoteInput {
        title: "Test".into(),
        language: "plaintext".into(),
        document: Document {
            version: 1,
            blocks: vec![Block::Text {
                text: "hello\r\n\u{fffc}".into(),
            }],
        },
    }
}
/// Encode a real raster fixture so storage tests exercise the native image decoder.
fn png() -> Vec<u8> {
    let mut bytes = Vec::new();
    image::codecs::png::PngEncoder::new(&mut bytes)
        .write_image(&[255, 0, 0, 255], 1, 1, image::ExtendedColorType::Rgba8)
        .unwrap();
    bytes
}
/// Attach a block with independent annotation state to the note fixture.
fn with_image(id: Uuid) -> NoteInput {
    let mut note = input();
    note.document.blocks.push(Block::Image {
        id: Uuid::new_v4(),
        attachment_id: id,
        annotations: vec![Annotation::Text {
            x: 0.0,
            y: 1.0,
            text: "caption".into(),
            color: "#ffffff".into(),
            font_size: 12.0,
        }],
    });
    note
}
/// Prove notes, reference ownership, original bytes, revisions, and session survive reopening.
#[test]
fn round_trip_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(dir.path()).unwrap();
    let bytes = png();
    let image = store.put_attachment(bytes.clone()).unwrap();
    let original = with_image(image.id);
    let expected = original.document.clone();
    let note = store.create_note(original).unwrap();
    let session = Session {
        version: 1,
        tabs: vec![Tab {
            note_id: note.summary.id,
            pinned: true,
            anchor: 1,
            head: 2,
            scroll_top: 2.5,
        }],
        active_note_id: Some(note.summary.id),
        sidebar_visible: false,
        word_wrap: true,
    };
    store.save_session(0, session).unwrap();
    drop(store);
    let store = Store::open(dir.path()).unwrap();
    assert_eq!(store.load_note(note.summary.id).unwrap().document, expected);
    assert_eq!(
        store
            .load_attachment(note.summary.id, image.id)
            .unwrap()
            .bytes,
        bytes
    );
    assert_eq!(
        store
            .load_attachment(Uuid::new_v4(), image.id)
            .unwrap_err()
            .code,
        "not_found"
    );
    assert_eq!(store.load_session().unwrap().revision, 1);
    assert_eq!(
        store.load_session().unwrap().state.unwrap().tabs[0].note_id,
        note.summary.id
    );
    assert_eq!(store.list_notes(10, 0).unwrap().len(), 1);
}
/// Reject missing references, invalid annotation positions, duplicate image IDs, and oversized documents atomically.
#[test]
fn rejects_invalid_documents_without_changes() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(dir.path()).unwrap();
    let note = store.create_note(input()).unwrap();
    assert_eq!(
        store
            .update_note(note.summary.id, 1, with_image(Uuid::new_v4()))
            .unwrap_err()
            .code,
        "missing_attachment"
    );
    let image = store.put_attachment(png()).unwrap();
    let mut bad = with_image(image.id);
    bad.document.blocks.push(bad.document.blocks[1].clone());
    assert_eq!(store.create_note(bad).unwrap_err().code, "validation");
    let mut bad = with_image(image.id);
    if let Block::Image { annotations, .. } = &mut bad.document.blocks[1] {
        annotations.push(Annotation::Text {
            x: 2.0,
            y: 0.0,
            text: "bad".into(),
            color: "#ffffff".into(),
            font_size: 1.0,
        });
    }
    assert_eq!(store.create_note(bad).unwrap_err().code, "validation");
    let mut bad = input();
    bad.document.version = 2;
    assert_eq!(store.create_note(bad).unwrap_err().code, "validation");
    let mut bad = input();
    bad.document.blocks = vec![Block::Text {
        text: "x".repeat(MAX_DOCUMENT),
    }];
    assert_eq!(store.create_note(bad).unwrap_err().code, "validation");
    assert_eq!(
        store.load_note(note.summary.id).unwrap().summary.revision,
        1
    );
    assert_eq!(store.list_notes(10, 0).unwrap().len(), 1);
    assert!(store.put_attachment(b"not an image".to_vec()).is_err());
    assert!(store.put_attachment(vec![0; MAX_IMAGE + 1]).is_err());
    assert!(store.list_notes(201, 0).is_err());
}
/// Exercise revision compare-and-swap through two real connections, including reference preservation.
#[test]
fn stale_note_write_preserves_content_and_references() {
    let dir = tempfile::tempdir().unwrap();
    let mut first = Store::open(dir.path()).unwrap();
    let mut second = Store::open(dir.path()).unwrap();
    let image = first.put_attachment(png()).unwrap();
    let note = first.create_note(input()).unwrap();
    first
        .update_note(note.summary.id, 1, with_image(image.id))
        .unwrap();
    assert_eq!(
        second
            .update_note(note.summary.id, 1, input())
            .unwrap_err()
            .code,
        "conflict"
    );
    assert_eq!(
        second.load_note(note.summary.id).unwrap().summary.revision,
        2
    );
    assert_eq!(
        second
            .load_attachment(note.summary.id, image.id)
            .unwrap()
            .bytes,
        png()
    );
}
/// Inject a database error after the note update and ensure both content and references roll back.
#[test]
fn failed_reference_write_rolls_back_entire_note() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(dir.path()).unwrap();
    let image = store.put_attachment(png()).unwrap();
    let note = store.create_note(input()).unwrap();
    store.connection.execute_batch("CREATE TRIGGER fail_reference BEFORE INSERT ON note_attachments BEGIN SELECT RAISE(ABORT, 'injected failure'); END;").unwrap();
    assert_eq!(
        store
            .update_note(note.summary.id, 1, with_image(image.id))
            .unwrap_err()
            .code,
        "database"
    );
    let loaded = store.load_note(note.summary.id).unwrap();
    assert_eq!(loaded.summary.revision, 1);
    assert_eq!(loaded.document, input().document);
    assert!(store.load_attachment(note.summary.id, image.id).is_err());
}
/// Session validation and revision failures leave the previous layout intact.
#[test]
fn session_validation_and_conflicts() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(dir.path()).unwrap();
    assert_eq!(store.load_session().unwrap().revision, 0);
    let mut state = Session {
        version: 1,
        tabs: vec![],
        active_note_id: None,
        sidebar_visible: true,
        word_wrap: false,
    };
    store.save_session(0, state.clone()).unwrap();
    assert_eq!(
        store.save_session(0, state.clone()).unwrap_err().code,
        "conflict"
    );
    state.active_note_id = Some(Uuid::new_v4());
    assert_eq!(store.save_session(1, state).unwrap_err().code, "validation");
    assert_eq!(store.load_session().unwrap().revision, 1);
}
/// A failed initial migration rolls back DDL without deleting pre-existing tables or changing its version.
#[test]
fn initialization_failure_preserves_existing_data() {
    let dir = tempfile::tempdir().unwrap();
    let connection = Connection::open(dir.path().join("notes.sqlite3")).unwrap();
    connection
        .execute_batch(
            "CREATE TABLE attachments (sentinel TEXT); INSERT INTO attachments VALUES ('keep');",
        )
        .unwrap();
    assert!(Store::open(dir.path()).is_err());
    assert_eq!(
        connection
            .query_row("SELECT sentinel FROM attachments", [], |r| r
                .get::<_, String>(0))
            .unwrap(),
        "keep"
    );
    assert_eq!(
        connection
            .pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        connection
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE name='notes'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
        0
    );
}
/// Unsupported schemas and corrupt files must remain untouched rather than being silently recreated.
#[test]
fn incompatible_and_corrupt_databases_are_preserved() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(dir.path()).unwrap();
    let note = store.create_note(input()).unwrap();
    store
        .connection
        .pragma_update(None, "user_version", 999)
        .unwrap();
    drop(store);
    assert!(Store::open(dir.path()).is_err());
    let connection = Connection::open(dir.path().join("notes.sqlite3")).unwrap();
    assert_eq!(
        connection
            .query_row(
                "SELECT title FROM notes WHERE id=?1",
                [note.summary.id.to_string()],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
        "Test"
    );
    let corrupt = tempfile::tempdir().unwrap();
    let path = corrupt.path().join("notes.sqlite3");
    std::fs::write(&path, b"not a database").unwrap();
    assert!(Store::open(corrupt.path()).is_err());
    assert_eq!(std::fs::read(path).unwrap(), b"not a database");
}
/// Serde rejects unknown document shapes before native storage logic accepts them.
#[test]
fn malformed_contracts_are_rejected() {
    assert!(
        serde_json::from_str::<Document>(
            r#"{"version":1,"blocks":[{"kind":"html","text":"bad"}]}"#
        )
        .is_err()
    );
    assert!(
        serde_json::from_str::<Document>(r#"{"version":1,"blocks":[],"unexpected":true}"#).is_err()
    );
    assert!(serde_json::from_str::<Document>(r#"{"version":1,"blocks":[{"kind":"image","id":"bad","attachmentId":"bad","annotations":[]}]}"#).is_err());
}

/// Sharing an immutable original never shares annotations; removing a reference does not collect its bytes.
#[test]
fn shared_originals_and_removed_references() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(dir.path()).unwrap();
    let original = store.put_attachment(png()).unwrap();
    let first = store.create_note(with_image(original.id)).unwrap();
    let second = store.create_note(with_image(original.id)).unwrap();
    let updated = store.update_note(first.summary.id, 1, input()).unwrap();
    assert_eq!(updated.summary.revision, 2);
    assert!(
        store
            .load_attachment(first.summary.id, original.id)
            .is_err()
    );
    assert_eq!(
        store
            .load_attachment(second.summary.id, original.id)
            .unwrap()
            .bytes,
        png()
    );
    store
        .update_note(first.summary.id, 2, with_image(original.id))
        .unwrap();
    assert_eq!(
        store
            .load_attachment(first.summary.id, original.id)
            .unwrap()
            .bytes,
        png()
    );
    assert_ne!(
        store.load_note(first.summary.id).unwrap().document,
        second.document
    );
}

/// Metadata listing remains usable when an individual document cannot be decoded, and errors preserve the bad data.
#[test]
fn listing_does_not_decode_document_bodies() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(dir.path()).unwrap();
    let note = store.create_note(input()).unwrap();
    store
        .connection
        .execute(
            "UPDATE notes SET document='broken json' WHERE id=?1",
            [note.summary.id.to_string()],
        )
        .unwrap();
    let list = store.list_notes(1, 0).unwrap();
    assert_eq!(list[0].id, note.summary.id);
    assert_eq!(store.load_note(note.summary.id).unwrap_err().code, "data");
    assert_eq!(
        store
            .connection
            .query_row(
                "SELECT document FROM notes WHERE id=?1",
                [note.summary.id.to_string()],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
        "broken json"
    );
}

/// A subprocess holds a live WAL connection until the parent terminates it without Rust destructors.
#[test]
fn acknowledged_commit_child() {
    let Some(directory) = std::env::var_os("SIMPLE_NOTE_COMMIT_PROBE") else {
        return;
    };
    let directory = std::path::PathBuf::from(directory);
    let mut store = Store::open(&directory).unwrap();
    let image = store.put_attachment(png()).unwrap();
    let note = store.create_note(input()).unwrap();
    let saved = store
        .update_note(note.summary.id, 1, with_image(image.id))
        .unwrap();
    store
        .save_session(
            0,
            Session {
                version: 1,
                tabs: vec![Tab {
                    note_id: saved.summary.id,
                    pinned: false,
                    anchor: 1,
                    head: 1,
                    scroll_top: 0.0,
                }],
                active_note_id: Some(saved.summary.id),
                sidebar_visible: false,
                word_wrap: true,
            },
        )
        .unwrap();
    std::fs::write(directory.join("acknowledged"), saved.summary.id.to_string()).unwrap();
    loop {
        std::thread::park();
    }
}

/// Forced process termination must preserve acknowledged note, image, and session transactions in the WAL.
#[test]
fn acknowledged_commits_survive_forced_termination() {
    use std::process::{Command, Stdio};
    let directory = tempfile::tempdir().unwrap();
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "storage::tests::acknowledged_commit_child",
            "--nocapture",
        ])
        .env("SIMPLE_NOTE_COMMIT_PROBE", directory.path())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let marker = directory.path().join("acknowledged");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !marker.exists() && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let ready = marker.exists();
    let _ = child.kill();
    child.wait().unwrap();
    assert!(
        ready,
        "Child did not acknowledge its commits before the deadline"
    );
    let id = Uuid::parse_str(&std::fs::read_to_string(marker).unwrap()).unwrap();
    let store = Store::open(directory.path()).unwrap();
    let note = store.load_note(id).unwrap();
    assert_eq!(note.summary.revision, 2);
    let Block::Image { attachment_id, .. } = note.document.blocks[1] else {
        panic!("Image reference did not survive");
    };
    assert_eq!(
        store.load_attachment(id, attachment_id).unwrap().bytes,
        png()
    );
    assert_eq!(store.load_session().unwrap().revision, 1);
    assert_eq!(
        store.load_session().unwrap().state.unwrap().active_note_id,
        Some(id)
    );
}

/// Trash survives restart, invalidates stale saves, atomically removes tabs, and never collects shared originals.
#[test]
fn trash_restore_and_shared_originals() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(dir.path()).unwrap();
    let image = store.put_attachment(png()).unwrap();
    let first = store.create_note(with_image(image.id)).unwrap();
    let second = store.create_note(with_image(image.id)).unwrap();
    let session = Session {
        version: 1,
        tabs: vec![
            Tab {
                note_id: first.summary.id,
                pinned: true,
                anchor: 0,
                head: 0,
                scroll_top: 0.0,
            },
            Tab {
                note_id: second.summary.id,
                pinned: false,
                anchor: 0,
                head: 0,
                scroll_top: 0.0,
            },
        ],
        active_note_id: Some(first.summary.id),
        sidebar_visible: true,
        word_wrap: true,
    };
    store.save_session(0, session).unwrap();
    assert!(
        store
            .change_note_lifecycle(first.summary.id, 1, "purge".into())
            .is_err()
    );
    let saved = store
        .change_note_lifecycle(first.summary.id, 1, "trash".into())
        .unwrap();
    assert_eq!(saved.revision, 2);
    assert_eq!(saved.state.unwrap().active_note_id, Some(second.summary.id));
    assert!(store.update_note(first.summary.id, 1, input()).is_err());
    assert!(
        store
            .save_session(1, store.load_session().unwrap().state.unwrap())
            .is_err()
    );
    drop(store);
    let mut store = Store::open(dir.path()).unwrap();
    assert_eq!(store.browse_notes(true, "".into(), 0).unwrap().len(), 1);
    assert_eq!(
        store.load_note(first.summary.id).unwrap().document,
        first.document
    );
    store
        .change_note_lifecycle(first.summary.id, 2, "restore".into())
        .unwrap();
    assert!(store.update_note(first.summary.id, 1, input()).is_err());
    assert!(store.update_note(first.summary.id, 2, input()).is_err());
    assert_eq!(
        store.load_note(first.summary.id).unwrap().summary.revision,
        3
    );
    store
        .change_note_lifecycle(first.summary.id, 3, "trash".into())
        .unwrap();
    store
        .change_note_lifecycle(first.summary.id, 4, "purge".into())
        .unwrap();
    assert!(store.load_note(first.summary.id).is_err());
    assert_eq!(
        store
            .load_attachment(second.summary.id, image.id)
            .unwrap()
            .bytes,
        png()
    );
    let saved = store
        .change_note_lifecycle(second.summary.id, 1, "trash".into())
        .unwrap();
    assert!(saved.state.unwrap().active_note_id.is_none());
}

/// Search joins only source text blocks, supports Unicode case folding, and returns bounded pages.
#[test]
fn library_search_source_only() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(dir.path()).unwrap();
    let image = store.put_attachment(png()).unwrap();
    let mut value = with_image(image.id);
    value.title = "Résumé".into();
    value.document.blocks[0] = Block::Text {
        text: "alpha".into(),
    };
    value.document.blocks.push(Block::Text {
        text: "beta".into(),
    });
    store.create_note(value).unwrap();
    assert_eq!(
        store
            .browse_notes(false, "ALPHABETA".into(), 0)
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        store.browse_notes(false, "RÉSUMÉ".into(), 0).unwrap().len(),
        1
    );
    for query in ["caption", "attachmentId", "\u{fffc}"] {
        assert!(
            store
                .browse_notes(false, query.into(), 0)
                .unwrap()
                .is_empty()
        );
    }
    assert!(store.browse_notes(false, "x".repeat(1025), 0).is_err());
    for _ in 0..101 {
        store.create_note(input()).unwrap();
    }
    assert_eq!(store.browse_notes(false, "".into(), 0).unwrap().len(), 100);
    assert_eq!(store.browse_notes(false, "".into(), 100).unwrap().len(), 2);
}

/// Shape inputs must retain their two-corner geometry at the IPC boundary.
#[test]
fn shape_validation() {
    for kind in ["rectangle", "circle"] {
        for points in [
            serde_json::json!([]),
            serde_json::json!([{"x":0,"y":0}]),
            serde_json::json!([{"x":0,"y":0},{"x":1,"y":1},{"x":0,"y":1}]),
            serde_json::json!([{"x":0,"y":0},{"x":2,"y":2}]),
        ] {
            let annotation: Annotation = serde_json::from_value(serde_json::json!({
                "kind":kind,"points":points,"color":"#ff4444","width":2
            }))
            .unwrap();
            assert!(validate_annotation(&annotation, 1, 1).is_err());
        }
    }
    let circle: Annotation = serde_json::from_value(serde_json::json!({
        "kind":"circle","points":[{"x":0,"y":0},{"x":1,"y":0.5}],"color":"#ff4444","width":2
    }))
    .unwrap();
    assert!(validate_annotation(&circle, 1, 1).is_err());
}

//! Emergency export preserves current edits independently of ordinary save validation and disk-write failures.
use crate::{
    queue::Storage,
    storage::{Store, StoreError},
};
use serde::Serialize;
use serde_json::Value;
use std::{collections::BTreeSet, io::Write, path::Path};
use tauri::State;
use tauri_plugin_dialog::DialogExt;
use uuid::Uuid;

/// Convert export I/O and encoding failures into structured errors without claiming an export exists.
fn error(e: impl std::fmt::Display) -> StoreError {
    StoreError {
        code: "recovery",
        message: e.to_string(),
    }
}

/// Atomically write source JSON plus available image originals. Oversized ordinary notes remain exportable.
fn write_snapshot(store: Option<&Store>, snapshot: &str, path: &Path) -> Result<u32, StoreError> {
    if snapshot.len() > 64 * 1024 * 1024 {
        return Err(error("Recovery snapshot exceeds 64 MiB"));
    }
    let workspace: Value = serde_json::from_str(snapshot).map_err(error)?;
    let notes = workspace
        .get("notes")
        .and_then(Value::as_array)
        .ok_or_else(|| error("Recovery snapshot requires notes"))?;
    let mut ids = BTreeSet::new();
    let mut missing = 0;
    for note in notes {
        let blocks = note
            .pointer("/input/document/blocks")
            .and_then(Value::as_array)
            .ok_or_else(|| error("Recovery document requires blocks"))?;
        for block in blocks {
            if block.get("kind").and_then(Value::as_str) == Some("image") {
                let id = block
                    .get("attachmentId")
                    .and_then(Value::as_str)
                    .ok_or_else(|| error("Recovery image requires an attachment ID"))?;
                ids.insert(Uuid::parse_str(id).map_err(error)?);
            }
        }
    }
    // A failed export leaves an existing destination untouched; rename happens only after flushing the full file.
    let parent = path
        .parent()
        .ok_or_else(|| error("Recovery destination has no parent"))?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent).map_err(error)?;
    {
        let mut writer = std::io::BufWriter::new(temporary.as_file_mut());
        writer
            .write_all(b"{\"version\":1,\"workspace\":")
            .map_err(error)?;
        serde_json::to_writer(&mut writer, &workspace).map_err(error)?;
        writer.write_all(b",\"attachments\":[").map_err(error)?;
        for (index, id) in ids.into_iter().enumerate() {
            if index > 0 {
                writer.write_all(b",").map_err(error)?;
            }
            match store.map(|store| store.recovery_attachment(id)) {
                Some(Ok(Some(attachment))) => {
                    serde_json::to_writer(&mut writer, &attachment).map_err(error)?
                }
                result => {
                    missing += 1;
                    let reason = match result {
                        Some(Err(error)) => error.to_string(),
                        _ => "Original is unavailable".into(),
                    };
                    serde_json::to_writer(
                        &mut writer,
                        &serde_json::json!({ "id": id, "missing": true, "reason": reason }),
                    )
                    .map_err(error)?;
                }
            }
        }
        writer.write_all(b"]}").map_err(error)?;
        writer.flush().map_err(error)?;
    }
    temporary.as_file().sync_all().map_err(error)?;
    temporary.persist(path).map_err(error)?;
    Ok(missing)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryExport {
    exported: bool,
    missing_originals: u32,
}

/// Ask the OS for a destination; renderer data never supplies a path. Cancellation is not success.
#[tauri::command]
pub async fn export_recovery(
    app: tauri::AppHandle,
    storage: State<'_, Storage>,
    snapshot: String,
) -> Result<RecoveryExport, StoreError> {
    if snapshot.len() > 64 * 1024 * 1024 {
        return Err(error("Recovery snapshot exceeds 64 MiB"));
    }
    let destination = tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .add_filter("Recovery JSON", &["json"])
            .set_file_name("sidenote-recovery.json")
            .blocking_save_file()
    })
    .await
    .map_err(error)?;
    let Some(destination) = destination else {
        return Ok(RecoveryExport {
            exported: false,
            missing_originals: 0,
        });
    };
    let path = destination.into_path().map_err(error)?;
    let snapshot = std::sync::Arc::new(snapshot);
    let queued_snapshot = snapshot.clone();
    let queued_path = path.clone();
    let result = storage
        .submit(move |store| write_snapshot(Some(store), &queued_snapshot, &queued_path))
        .await;
    let missing_originals = match result {
        Ok(missing) => missing,
        Err(failure) if failure.code == "unavailable" => {
            // A dead worker must not prevent exporting unsaved source. Missing originals are explicit in the file and UI.
            tauri::async_runtime::spawn_blocking(move || write_snapshot(None, &snapshot, &path))
                .await
                .map_err(error)??
        }
        Err(error) => return Err(error),
    };
    Ok(RecoveryExport {
        exported: true,
        missing_originals,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    /// Recovery preserves edits that exceed ordinary save constraints and never truncates a destination on failure.
    #[test]
    fn export_unsaved_and_preserve_on_failure() {
        let directory = tempfile::tempdir().unwrap();
        let store = Store::open(directory.path()).unwrap();
        let path = directory.path().join("recovery.json");
        let snapshot = serde_json::json!({"notes":[{"input":{"title":"x".repeat(2048),"document":{"version":1,"blocks":[{"kind":"text","text":"unsaved\r\n\u{fffc}"}]}}}],"session":null});
        write_snapshot(Some(&store), &snapshot.to_string(), &path).unwrap();
        let saved: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(saved["workspace"], snapshot);
        let before = std::fs::read(&path).unwrap();
        assert!(write_snapshot(Some(&store), "invalid", &path).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }
    /// Originals are exported once per attachment, while absent originals are explicitly marked rather than fabricated.
    #[test]
    fn export_originals_and_mark_missing() {
        use image::ImageEncoder;
        let directory = tempfile::tempdir().unwrap();
        let mut store = Store::open(directory.path()).unwrap();
        let mut bytes = Vec::new();
        image::codecs::png::PngEncoder::new(&mut bytes)
            .write_image(&[255, 0, 0, 255], 1, 1, image::ExtendedColorType::Rgba8)
            .unwrap();
        let original = store.put_attachment(bytes.clone()).unwrap();
        let missing = Uuid::new_v4();
        let snapshot = serde_json::json!({"notes":[{"input":{"document":{"blocks":[
            {"kind":"image","attachmentId":original.id},
            {"kind":"image","attachmentId":original.id},
            {"kind":"image","attachmentId":missing}
        ]}}}]});
        let path = directory.path().join("recovery.json");
        write_snapshot(Some(&store), &snapshot.to_string(), &path).unwrap();
        let saved: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        let originals = saved["attachments"].as_array().unwrap();
        assert_eq!(originals.len(), 2);
        let found = originals
            .iter()
            .find(|a| a["id"] == original.id.to_string())
            .unwrap();
        assert_eq!(found["bytes"], serde_json::json!(bytes));
        assert!(
            originals
                .iter()
                .any(|a| a["id"] == missing.to_string() && a["missing"] == true)
        );
    }
    /// Even a stopped database worker must permit source export, with every missing original reported explicitly.
    #[test]
    fn source_only_export_without_store() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("source-only.json");
        let snapshot = serde_json::json!({ "notes": [{ "input": { "document": { "blocks": [
            { "kind": "text", "text": "latest unsaved source" },
            { "kind": "image", "attachmentId": Uuid::new_v4() }
        ] } } }] });
        assert_eq!(
            write_snapshot(None, &snapshot.to_string(), &path).unwrap(),
            1
        );
        let exported: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        assert_eq!(exported["workspace"], snapshot);
        assert_eq!(exported["attachments"][0]["missing"], true);
    }
}

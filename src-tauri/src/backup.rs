//! Native backup pickers and one-use restore confirmation tokens. Renderer input never supplies paths.
use crate::{
    queue::Storage,
    storage::{BackupStatus, PreparedRestore, Store, StoreError, backup::error},
};
use serde::Serialize;
use std::sync::Mutex;
use tauri::{Manager, State};
use tauri_plugin_dialog::DialogExt;

#[derive(Default)]
pub struct RestoreState(Mutex<Option<PreparedRestore>>);

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestorePreview {
    token: String,
    notes: i64,
    trashed: i64,
}

/// Return warnings without making a failed automatic snapshot fail an already committed note save.
#[tauri::command]
pub fn backup_status(storage: State<'_, Storage>) -> BackupStatus {
    storage.backup_status()
}

/// Export a worker-serialized SQLite snapshot through a native save picker; cancellation writes nothing.
#[tauri::command]
pub async fn export_backup(
    app: tauri::AppHandle,
    storage: State<'_, Storage>,
) -> Result<bool, StoreError> {
    let destination = tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .add_filter("Simple Note SQLite backup", &["sqlite3"])
            .set_file_name("simple-note-backup.sqlite3")
            .blocking_save_file()
    })
    .await
    .map_err(error)?;
    let Some(destination) = destination else {
        return Ok(false);
    };
    let path = destination.into_path().map_err(error)?;
    storage
        .submit(move |store| {
            store.export_backup(&path)?;
            Ok(true)
        })
        .await
}

/// Validate and privately stage selected bytes before the UI asks the user to replace the local workspace.
#[tauri::command]
pub async fn prepare_restore(
    app: tauri::AppHandle,
    state: State<'_, RestoreState>,
) -> Result<Option<RestorePreview>, StoreError> {
    state.0.lock().map_err(error)?.take();
    let directory = app.path().app_data_dir().map_err(error)?;
    let prepared = tauri::async_runtime::spawn_blocking(move || {
        let source = app
            .dialog()
            .file()
            .set_directory(directory.join("backups"))
            .add_filter("Simple Note SQLite backup", &["sqlite3"])
            .blocking_pick_file();
        source
            .map(|source| {
                Store::prepare_restore(
                    &source.into_path().map_err(error)?,
                    &directory.join("backups"),
                )
            })
            .transpose()
    })
    .await
    .map_err(error)??;
    let preview = prepared.as_ref().map(|candidate| RestorePreview {
        token: candidate.token.clone(),
        notes: candidate.notes,
        trashed: candidate.trashed,
    });
    *state.0.lock().map_err(error)? = prepared;
    Ok(preview)
}

/// Discard a staged candidate when confirmation is cancelled; its temporary file is released immediately.
#[tauri::command]
pub fn cancel_restore(state: State<'_, RestoreState>) -> Result<(), StoreError> {
    state.0.lock().map_err(error)?.take();
    Ok(())
}

/// Consume the exact staged candidate only after explicit confirmation and the renderer's flush barrier.
#[tauri::command]
pub async fn restore_backup(
    storage: State<'_, Storage>,
    state: State<'_, RestoreState>,
    token: String,
) -> Result<(), StoreError> {
    let prepared = {
        let mut pending = state.0.lock().map_err(error)?;
        if pending.as_ref().map(|candidate| candidate.token.as_str()) != Some(token.as_str()) {
            return Err(error(
                "Restore confirmation expired; select the backup again",
            ));
        }
        pending
            .take()
            .ok_or_else(|| error("Select a backup before restoring"))?
    };
    storage.restore(prepared).await
}

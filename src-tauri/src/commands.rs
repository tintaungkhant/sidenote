//! Narrow Tauri boundary. One FIFO worker owns all SQLite access off the UI thread.
pub use crate::queue::Storage;
use crate::storage::{
    Attachment, AttachmentInfo, Note, NoteInput, NoteSummary, SavedSession, Session, Store,
    StoreError,
};
use tauri::State;
use uuid::Uuid;

/// Enqueue immediately in the single native FIFO, then await its committed result.
async fn access<T: Send + 'static>(
    state: State<'_, Storage>,
    operation: impl FnOnce(&mut Store) -> Result<T, StoreError> + Send + 'static,
) -> Result<T, StoreError> {
    state.submit(operation).await
}

/// Create a validated note without accepting filesystem paths or renderer-chosen note IDs.
#[tauri::command]
pub async fn create_note(state: State<'_, Storage>, input: NoteInput) -> Result<Note, StoreError> {
    access(state, move |s| s.create_note(input)).await
}
/// Load only the requested note's document.
#[tauri::command]
pub async fn load_note(state: State<'_, Storage>, id: Uuid) -> Result<Note, StoreError> {
    access(state, move |s| s.load_note(id)).await
}
/// Commit an edit only against the exact revision acknowledged to the caller.
#[tauri::command]
pub async fn update_note(
    state: State<'_, Storage>,
    id: Uuid,
    expected_revision: i64,
    input: NoteInput,
) -> Result<Note, StoreError> {
    access(state, move |s| s.update_note(id, expected_revision, input)).await
}
/// Return a bounded page of metadata without document or image payloads.
#[tauri::command]
pub async fn list_notes(
    state: State<'_, Storage>,
    limit: u32,
    offset: u32,
) -> Result<Vec<NoteSummary>, StoreError> {
    access(state, move |s| s.list_notes(limit, offset)).await
}
/// Validate and store one immutable image original; content saves never resend its bytes.
#[tauri::command]
pub async fn put_attachment(
    state: State<'_, Storage>,
    bytes: Vec<u8>,
) -> Result<AttachmentInfo, StoreError> {
    access(state, move |s| s.put_attachment(bytes)).await
}
/// Resolve original image bytes only through a persisted note reference.
#[tauri::command]
pub async fn load_attachment(
    state: State<'_, Storage>,
    note_id: Uuid,
    attachment_id: Uuid,
) -> Result<Attachment, StoreError> {
    access(state, move |s| s.load_attachment(note_id, attachment_id)).await
}
/// Load session state without loading all note contents.
#[tauri::command]
pub async fn load_session(state: State<'_, Storage>) -> Result<SavedSession, StoreError> {
    access(state, |s| s.load_session()).await
}
/// Reject stale session writes instead of replacing a newer tab layout.
#[tauri::command]
pub async fn save_session(
    state: State<'_, Storage>,
    expected_revision: i64,
    session: Session,
) -> Result<SavedSession, StoreError> {
    access(state, move |s| s.save_session(expected_revision, session)).await
}

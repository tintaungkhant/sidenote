//! Validated local persistence. Only this module opens the database or mutates durable state.
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    io::Cursor,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};
use uuid::Uuid;

const MAX_DOCUMENT: usize = 8 * 1024 * 1024;
const MAX_IMAGE: usize = 20 * 1024 * 1024;
const MAX_PIXELS: u64 = 40_000_000;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StoreError {
    pub code: &'static str,
    pub message: String,
}
type Result<T> = std::result::Result<T, StoreError>;

impl StoreError {
    /// Construct an error with a stable frontend code and an actionable description.
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}
impl std::fmt::Display for StoreError {
    /// Include the stable error category in startup diagnostics.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}
impl std::error::Error for StoreError {}
impl From<rusqlite::Error> for StoreError {
    /// Preserve SQLite failures instead of replacing or resetting the store.
    fn from(error: rusqlite::Error) -> Self {
        Self::new("database", error.to_string())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Point {
    x: f64,
    y: f64,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum Annotation {
    Pen {
        points: Vec<Point>,
        color: String,
        width: f64,
    },
    Highlighter {
        points: Vec<Point>,
        color: String,
        width: f64,
    },
    Arrow {
        points: Vec<Point>,
        color: String,
        width: f64,
    },
    Text {
        x: f64,
        y: f64,
        text: String,
        color: String,
        #[serde(rename = "fontSize")]
        font_size: f64,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum Block {
    Text {
        text: String,
    },
    Image {
        id: Uuid,
        #[serde(rename = "attachmentId")]
        attachment_id: Uuid,
        annotations: Vec<Annotation>,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Document {
    version: u32,
    blocks: Vec<Block>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NoteInput {
    pub title: String,
    pub document: Document,
    pub language: String,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteSummary {
    pub id: Uuid,
    pub title: String,
    pub language: String,
    pub revision: i64,
    pub created_at: i64,
    pub updated_at: i64,
    pub deleted_at: Option<i64>,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Note {
    #[serde(flatten)]
    pub summary: NoteSummary,
    pub document: Document,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentInfo {
    pub id: Uuid,
    pub mime_type: String,
    pub width: u32,
    pub height: u32,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Attachment {
    #[serde(flatten)]
    pub info: AttachmentInfo,
    pub bytes: Vec<u8>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Tab {
    pub note_id: Uuid,
    pub pinned: bool,
    pub anchor: u32,
    pub head: u32,
    pub scroll_top: f64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Session {
    pub version: u32,
    pub tabs: Vec<Tab>,
    pub active_note_id: Option<Uuid>,
    pub sidebar_visible: bool,
    pub word_wrap: bool,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedSession {
    pub revision: i64,
    pub state: Option<Session>,
}

/// Return epoch milliseconds without panicking if the host clock predates the epoch.
fn now() -> Result<i64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .map_err(|_| StoreError::new("clock", "System clock predates the Unix epoch"))
}
/// Reject invalid data at the native boundary before beginning a write transaction.
fn invalid(message: &str) -> StoreError {
    StoreError::new("validation", message)
}
/// Bound annotation coordinates and styles; original-image dimensions are checked separately.
fn validate_annotation(annotation: &Annotation, width: u32, height: u32) -> Result<()> {
    let point_ok = |x: f64, y: f64| {
        x.is_finite()
            && y.is_finite()
            && x >= 0.0
            && y >= 0.0
            && x <= width as f64
            && y <= height as f64
    };
    let (color, size) = match annotation {
        Annotation::Text {
            x,
            y,
            text,
            color,
            font_size,
        } => {
            if !point_ok(*x, *y) || text.len() > 64 * 1024 {
                return Err(invalid("Invalid annotation text or position"));
            }
            (color, *font_size)
        }
        Annotation::Pen {
            points,
            color,
            width,
        }
        | Annotation::Highlighter {
            points,
            color,
            width,
        }
        | Annotation::Arrow {
            points,
            color,
            width,
        } => {
            if points.is_empty()
                || points.len() > 100_000
                || points.iter().any(|p| !point_ok(p.x, p.y))
            {
                return Err(invalid("Invalid annotation points"));
            }
            (color, *width)
        }
    };
    if !size.is_finite()
        || size <= 0.0
        || size > 4096.0
        || color.len() != 7
        || !color.starts_with('#')
        || !color[1..].bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err(invalid(
            "Annotation requires a bounded size and #RRGGBB color",
        ));
    }
    Ok(())
}

pub struct Store {
    connection: Connection,
}
impl Store {
    /// Open only an app-owned directory. Initialization is transactional and never resets existing data.
    pub fn open(directory: &Path) -> Result<Self> {
        std::fs::create_dir_all(directory)
            .map_err(|e| StoreError::new("filesystem", e.to_string()))?;
        let mut connection = Connection::open(directory.join("notes.sqlite3"))?;
        connection.busy_timeout(std::time::Duration::from_secs(5))?;
        connection.pragma_update(None, "foreign_keys", "ON")?;
        let transaction =
            connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let version: i64 = transaction.pragma_query_value(None, "user_version", |r| r.get(0))?;
        match version {
            0 => transaction.execute_batch("CREATE TABLE notes (
                id TEXT PRIMARY KEY NOT NULL, title TEXT NOT NULL, document_version INTEGER NOT NULL CHECK(document_version = 1),
                document TEXT NOT NULL, language TEXT NOT NULL, revision INTEGER NOT NULL CHECK(revision >= 1),
                created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL, deleted_at INTEGER);
                CREATE TABLE attachments (id TEXT PRIMARY KEY NOT NULL, mime_type TEXT NOT NULL CHECK(mime_type IN ('image/png','image/jpeg')),
                bytes BLOB NOT NULL CHECK(length(bytes) BETWEEN 1 AND 20971520), width INTEGER NOT NULL CHECK(width > 0), height INTEGER NOT NULL CHECK(height > 0),
                created_at INTEGER NOT NULL, CHECK(width * height <= 40000000));
                CREATE TABLE note_attachments (note_id TEXT NOT NULL REFERENCES notes(id) ON DELETE CASCADE,
                attachment_id TEXT NOT NULL REFERENCES attachments(id), PRIMARY KEY(note_id, attachment_id));
                CREATE TABLE session (id INTEGER PRIMARY KEY CHECK(id = 1), version INTEGER NOT NULL CHECK(version = 1),
                state TEXT NOT NULL, revision INTEGER NOT NULL CHECK(revision >= 1));
                CREATE INDEX notes_updated ON notes(deleted_at, updated_at DESC, id);
                PRAGMA user_version = 1;")?,
            1 => (),
            _ => return Err(StoreError::new("schema", "Database schema is newer than this app; use a compatible app version")),
        }
        // Preparing these queries detects incomplete schemas without changing or deleting user data.
        transaction.prepare("SELECT id,title,document_version,document,language,revision,created_at,updated_at,deleted_at FROM notes LIMIT 0")?;
        transaction.prepare(
            "SELECT id,mime_type,bytes,width,height,created_at FROM attachments LIMIT 0",
        )?;
        transaction.prepare("SELECT note_id,attachment_id FROM note_attachments LIMIT 0")?;
        transaction.prepare("SELECT id,version,state,revision FROM session LIMIT 0")?;
        transaction.commit()?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.pragma_update(None, "synchronous", "FULL")?;
        Ok(Self { connection })
    }

    /// Validate the full document and resolve every image reference without loading its bytes.
    fn validate_note(&self, input: &NoteInput) -> Result<(String, HashSet<Uuid>)> {
        if input.title.len() > 1024
            || !matches!(
                input.language.as_str(),
                "plaintext" | "markdown" | "javascript" | "typescript" | "json" | "html"
            )
        {
            return Err(invalid("Invalid title or unsupported language"));
        }
        if input.document.version != 1 || input.document.blocks.len() > 100_000 {
            return Err(invalid("Unsupported document version or too many blocks"));
        }
        let json = serde_json::to_string(&input.document).map_err(|e| invalid(&e.to_string()))?;
        if json.len() > MAX_DOCUMENT {
            return Err(invalid("Document exceeds 8 MiB"));
        }
        let mut images = HashSet::new();
        let mut attachments = HashSet::new();
        for block in &input.document.blocks {
            if let Block::Image {
                id,
                attachment_id,
                annotations,
            } = block
            {
                if id.is_nil()
                    || attachment_id.is_nil()
                    || !images.insert(*id)
                    || annotations.len() > 10_000
                {
                    return Err(invalid(
                        "Invalid or duplicate image ID, or too many annotations",
                    ));
                }
                let dimensions: Option<(u32, u32)> = self
                    .connection
                    .query_row(
                        "SELECT width,height FROM attachments WHERE id=?1",
                        [attachment_id.to_string()],
                        |r| Ok((r.get(0)?, r.get(1)?)),
                    )
                    .optional()?;
                let (width, height) = dimensions.ok_or_else(|| {
                    StoreError::new(
                        "missing_attachment",
                        "Document references an attachment that does not exist",
                    )
                })?;
                for annotation in annotations {
                    validate_annotation(annotation, width, height)?;
                }
                attachments.insert(*attachment_id);
            }
        }
        Ok((json, attachments))
    }

    /// Create a revision-one note and its references in one transaction; IDs are generated natively.
    pub fn create_note(&mut self, input: NoteInput) -> Result<Note> {
        let (document, attachments) = self.validate_note(&input)?;
        let id = Uuid::new_v4();
        let timestamp = now()?;
        let tx = self.connection.transaction()?;
        tx.execute(
            "INSERT INTO notes VALUES (?1,?2,1,?3,?4,1,?5,?5,NULL)",
            params![
                id.to_string(),
                input.title,
                document,
                input.language,
                timestamp
            ],
        )?;
        for attachment in attachments {
            tx.execute(
                "INSERT INTO note_attachments VALUES (?1,?2)",
                params![id.to_string(), attachment.to_string()],
            )?;
        }
        tx.commit()?;
        Ok(Note {
            summary: NoteSummary {
                id,
                title: input.title,
                language: input.language,
                revision: 1,
                created_at: timestamp,
                updated_at: timestamp,
                deleted_at: None,
            },
            document: input.document,
        })
    }

    /// Load one complete note. Missing notes and unreadable JSON are explicit errors.
    pub fn load_note(&self, id: Uuid) -> Result<Note> {
        let row = self.connection.query_row("SELECT id,title,language,revision,created_at,updated_at,deleted_at,document FROM notes WHERE id=?1", [id.to_string()], |r| Ok((summary_row(r)?, r.get::<_,String>(7)?))).optional()?;
        let (summary, json) =
            row.ok_or_else(|| StoreError::new("not_found", "Note does not exist"))?;
        let document =
            serde_json::from_str(&json).map_err(|e| StoreError::new("data", e.to_string()))?;
        Ok(Note { summary, document })
    }

    /// Atomically replace content and references only when the caller's revision is current.
    pub fn update_note(
        &mut self,
        id: Uuid,
        expected_revision: i64,
        input: NoteInput,
    ) -> Result<Note> {
        if expected_revision < 1 || expected_revision == i64::MAX {
            return Err(invalid("Invalid expected revision"));
        }
        let (document, attachments) = self.validate_note(&input)?;
        let tx = self.connection.transaction()?;
        let summary = tx.query_row("UPDATE notes SET title=?1,document=?2,language=?3,revision=revision+1,updated_at=?4 WHERE id=?5 AND revision=?6 AND deleted_at IS NULL RETURNING id,title,language,revision,created_at,updated_at,deleted_at", params![input.title,document,input.language,now()?,id.to_string(),expected_revision], summary_row).optional()?.ok_or_else(|| StoreError::new("conflict", "Note is missing, deleted, or has a newer revision; reload before retrying"))?;
        tx.execute(
            "DELETE FROM note_attachments WHERE note_id=?1",
            [id.to_string()],
        )?;
        for attachment in attachments {
            tx.execute(
                "INSERT INTO note_attachments VALUES (?1,?2)",
                params![id.to_string(), attachment.to_string()],
            )?;
        }
        tx.commit()?;
        Ok(Note {
            summary,
            document: input.document,
        })
    }

    /// Read a bounded metadata page. This query never selects document JSON or attachment BLOBs.
    pub fn list_notes(&self, limit: u32, offset: u32) -> Result<Vec<NoteSummary>> {
        if limit == 0 || limit > 200 {
            return Err(invalid("List limit must be between 1 and 200"));
        }
        let mut statement = self.connection.prepare("SELECT id,title,language,revision,created_at,updated_at,deleted_at FROM notes WHERE deleted_at IS NULL ORDER BY updated_at DESC,id LIMIT ?1 OFFSET ?2")?;
        let rows = statement.query_map(params![limit, offset], summary_row)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    /// Search source text only, one document at a time; return a bounded page without image payloads.
    pub fn browse_notes(
        &self,
        trash: bool,
        query: String,
        offset: u32,
    ) -> Result<Vec<NoteSummary>> {
        if query.len() > 1024 {
            return Err(invalid("Search is limited to 1024 bytes"));
        }
        let query = query.to_lowercase();
        let mut statement = self.connection.prepare("SELECT id,title,language,revision,created_at,updated_at,deleted_at,document FROM notes WHERE (deleted_at IS NOT NULL)=?1 ORDER BY updated_at DESC,id")?;
        let mut rows = statement.query([trash])?;
        let mut found = Vec::new();
        let mut skipped = 0;
        while let Some(row) = rows.next()? {
            let summary = summary_row(row)?;
            let matches = if query.is_empty() || summary.title.to_lowercase().contains(&query) {
                true
            } else {
                let document: Document = serde_json::from_str(&row.get::<_, String>(7)?)
                    .map_err(|e| StoreError::new("data", e.to_string()))?;
                let text: String = document
                    .blocks
                    .iter()
                    .filter_map(|block| match block {
                        Block::Text { text } => Some(text.as_str()),
                        _ => None,
                    })
                    .collect();
                text.to_lowercase().contains(&query)
            };
            if matches {
                if skipped < offset {
                    skipped += 1;
                } else {
                    found.push(summary);
                }
                if found.len() == 100 {
                    break;
                }
            }
        }
        Ok(found)
    }

    /// Revision-checked Trash transitions invalidate old saves; deleting also prunes durable tabs atomically.
    pub fn change_note_lifecycle(
        &mut self,
        id: Uuid,
        expected_revision: i64,
        action: String,
    ) -> Result<SavedSession> {
        if expected_revision < 1 || expected_revision == i64::MAX {
            return Err(invalid("Invalid expected revision"));
        }
        let mut saved = self.load_session()?;
        let tx = self.connection.transaction()?;
        let changed = match action.as_str() {
            "trash" => tx.execute("UPDATE notes SET deleted_at=?1,updated_at=?1,revision=revision+1 WHERE id=?2 AND revision=?3 AND deleted_at IS NULL", params![now()?,id.to_string(),expected_revision])?,
            "restore" => tx.execute("UPDATE notes SET deleted_at=NULL,updated_at=?1,revision=revision+1 WHERE id=?2 AND revision=?3 AND deleted_at IS NOT NULL", params![now()?,id.to_string(),expected_revision])?,
            "purge" => tx.execute("DELETE FROM notes WHERE id=?1 AND revision=?2 AND deleted_at IS NOT NULL", params![id.to_string(),expected_revision])?,
            _ => return Err(invalid("Unknown Trash action")),
        };
        if changed != 1 {
            return Err(StoreError::new(
                "conflict",
                "Note changed; refresh the library before retrying",
            ));
        }
        if action != "restore" {
            if let Some(state) = &mut saved.state {
                let index = state
                    .tabs
                    .iter()
                    .position(|tab| tab.note_id == id)
                    .unwrap_or(0);
                state.tabs.retain(|tab| tab.note_id != id);
                if state.active_note_id == Some(id) {
                    state.active_note_id = state
                        .tabs
                        .get(index.min(state.tabs.len().saturating_sub(1)))
                        .map(|tab| tab.note_id);
                }
                let json = serde_json::to_string(state)
                    .map_err(|e| StoreError::new("data", e.to_string()))?;
                tx.execute(
                    "UPDATE session SET state=?1,revision=revision+1 WHERE id=1",
                    [json],
                )?;
                saved.revision += 1;
            }
        }
        tx.commit()?;
        Ok(saved)
    }

    /// Decode bounded PNG/JPEG input before storing immutable original bytes with native-derived metadata.
    pub fn put_attachment(&mut self, bytes: Vec<u8>) -> Result<AttachmentInfo> {
        if bytes.is_empty() || bytes.len() > MAX_IMAGE {
            return Err(invalid("Image must be between 1 byte and 20 MiB"));
        }
        let format =
            image::guess_format(&bytes).map_err(|_| invalid("Unrecognized raster image"))?;
        let mime_type = match format {
            image::ImageFormat::Png => "image/png",
            image::ImageFormat::Jpeg => "image/jpeg",
            _ => return Err(invalid("Only PNG and JPEG originals are supported")),
        };
        let mut reader = image::ImageReader::with_format(Cursor::new(&bytes), format);
        let mut limits = image::Limits::default();
        limits.max_alloc = Some(256 * 1024 * 1024);
        reader.limits(limits);
        let (width, height) = image::ImageReader::with_format(Cursor::new(&bytes), format)
            .into_dimensions()
            .map_err(|_| invalid("Invalid image dimensions"))?;
        if width == 0 || height == 0 || u64::from(width) * u64::from(height) > MAX_PIXELS {
            return Err(invalid("Image exceeds 40 megapixels"));
        }
        // Force decoding here so truncated or malformed originals never become durable attachments.
        reader
            .decode()
            .map_err(|_| invalid("Image could not be decoded within memory limits"))?;
        let id = Uuid::new_v4();
        self.connection.execute(
            "INSERT INTO attachments VALUES (?1,?2,?3,?4,?5,?6)",
            params![id.to_string(), mime_type, bytes, width, height, now()?],
        )?;
        Ok(AttachmentInfo {
            id,
            mime_type: mime_type.into(),
            width,
            height,
        })
    }

    /// Return bytes only through an existing note's durable reference, never a caller-supplied path.
    pub fn load_attachment(&self, note_id: Uuid, attachment_id: Uuid) -> Result<Attachment> {
        self.connection.query_row("SELECT a.mime_type,a.width,a.height,a.bytes FROM attachments a JOIN note_attachments n ON n.attachment_id=a.id WHERE n.note_id=?1 AND a.id=?2", params![note_id.to_string(),attachment_id.to_string()], |r| Ok(Attachment { info: AttachmentInfo { id: attachment_id, mime_type: r.get(0)?, width: r.get(1)?, height: r.get(2)? }, bytes: r.get(3)? })).optional()?.ok_or_else(|| StoreError::new("not_found", "Attachment is not referenced by this note"))
    }

    /// Read an original for a recovery snapshot, including references not yet committed to a note.
    pub(crate) fn recovery_attachment(&self, id: Uuid) -> Result<Option<Attachment>> {
        self.connection
            .query_row(
                "SELECT mime_type,width,height,bytes FROM attachments WHERE id=?1",
                [id.to_string()],
                |r| {
                    Ok(Attachment {
                        info: AttachmentInfo {
                            id,
                            mime_type: r.get(0)?,
                            width: r.get(1)?,
                            height: r.get(2)?,
                        },
                        bytes: r.get(3)?,
                    })
                },
            )
            .optional()
            .map_err(Into::into)
    }

    /// Load session independently of note content. Tab restoration later filters stale note IDs.
    pub fn load_session(&self) -> Result<SavedSession> {
        let row = self
            .connection
            .query_row("SELECT revision,state FROM session WHERE id=1", [], |r| {
                Ok((r.get(0)?, r.get::<_, String>(1)?))
            })
            .optional()?;
        match row {
            None => Ok(SavedSession {
                revision: 0,
                state: None,
            }),
            Some((revision, json)) => Ok(SavedSession {
                revision,
                state: Some(
                    serde_json::from_str(&json)
                        .map_err(|e| StoreError::new("data", e.to_string()))?,
                ),
            }),
        }
    }

    /// Persist bounded session state with optimistic concurrency, including on the initial insert.
    pub fn save_session(&mut self, expected_revision: i64, state: Session) -> Result<SavedSession> {
        if expected_revision < 0
            || expected_revision == i64::MAX
            || state.version != 1
            || state.tabs.len() > 1000
        {
            return Err(invalid("Invalid session revision, version, or tab count"));
        }
        let mut ids = HashSet::new();
        let mut unpinned = false;
        for tab in &state.tabs {
            if tab.note_id.is_nil()
                || !ids.insert(tab.note_id)
                || !tab.scroll_top.is_finite()
                || tab.scroll_top < 0.0
                || tab.scroll_top > 1e12
                || (unpinned && tab.pinned)
            {
                return Err(invalid("Invalid tab state, duplicate tab, or pinned order"));
            }
            unpinned |= !tab.pinned;
        }
        if state.active_note_id.is_some_and(|id| !ids.contains(&id)) {
            return Err(invalid("Active note must be an open tab"));
        }
        let json = serde_json::to_string(&state).map_err(|e| invalid(&e.to_string()))?;
        if json.len() > 1024 * 1024 {
            return Err(invalid("Session exceeds 1 MiB"));
        }
        let tx = self
            .connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let revision = tx
            .query_row("SELECT revision FROM session WHERE id=1", [], |r| {
                r.get::<_, i64>(0)
            })
            .optional()?
            .unwrap_or(0);
        if revision != expected_revision {
            return Err(StoreError::new("conflict", "Session has a newer revision"));
        }
        tx.execute("INSERT INTO session VALUES(1,1,?1,?2) ON CONFLICT(id) DO UPDATE SET state=excluded.state,revision=excluded.revision", params![json,revision+1])?;
        tx.commit()?;
        Ok(SavedSession {
            revision: revision + 1,
            state: Some(state),
        })
    }
}

/// Decode only metadata columns; malformed persisted identifiers become database errors.
fn summary_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<NoteSummary> {
    let id: String = row.get(0)?;
    let id = Uuid::parse_str(&id).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
    })?;
    Ok(NoteSummary {
        id,
        title: row.get(1)?,
        language: row.get(2)?,
        revision: row.get(3)?,
        created_at: row.get(4)?,
        updated_at: row.get(5)?,
        deleted_at: row.get(6)?,
    })
}

#[cfg(test)]
mod tests;

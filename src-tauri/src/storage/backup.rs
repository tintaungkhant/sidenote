//! SQLite snapshots, candidate validation, and crash-recoverable replacement stay beside storage invariants.
use super::*;
use rusqlite::{OpenFlags, backup::Backup};
use std::{fs, io::Write, path::PathBuf};

const DATABASE_FILES: [&str; 4] = [
    "notes.sqlite3",
    "notes.sqlite3-wal",
    "notes.sqlite3-shm",
    "notes.sqlite3-journal",
];
const RESTORE_MARKER: &str = "restore-pending";

#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupStatus {
    pub directory: String,
    pub last_backup: Option<String>,
    pub error: Option<String>,
}

pub struct PreparedRestore {
    directory: tempfile::TempDir,
    pub token: String,
    pub notes: i64,
    pub trashed: i64,
}

impl PreparedRestore {
    /// Expose the private staged file only to native replacement code, never to the renderer.
    pub(crate) fn path(&self) -> PathBuf {
        self.directory.path().join("notes.sqlite3")
    }
}

/// Keep backup failures distinct from acknowledged note writes.
pub(crate) fn error(e: impl std::fmt::Display) -> StoreError {
    StoreError::new("backup", e.to_string())
}

/// Open an existing SQLite file without creating it or permitting schema-controlled functions.
fn read_only(path: &Path) -> Result<Connection> {
    let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    connection.busy_timeout(std::time::Duration::from_secs(5))?;
    connection.pragma_update(None, "trusted_schema", "OFF")?;
    Ok(connection)
}

/// Reject corrupt pages, unknown schema versions, foreign-key damage, or executable/custom schema objects.
fn validate_structure(connection: &Connection) -> Result<()> {
    let version: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version != 1 {
        return Err(error("Backup schema is unsupported; expected version 1"));
    }
    let check: String = connection.query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
    if check != "ok" {
        return Err(error(format!("Backup integrity check failed: {check}")));
    }
    if connection.prepare("PRAGMA foreign_key_check")?.exists([])? {
        return Err(error("Backup has broken attachment references"));
    }
    // Exact schema comparison also rejects triggers, views, and altered constraints before opening a writable store.
    let expected_dir = tempfile::tempdir().map_err(error)?;
    let expected = Store::open(expected_dir.path())?;
    /// Compare the entire user schema, including constraints and indexes, with a fresh version-one store.
    fn schema(connection: &Connection) -> Result<Vec<(String, String, String, Option<String>)>> {
        let mut statement = connection.prepare("SELECT type,name,tbl_name,sql FROM sqlite_schema WHERE name NOT GLOB 'sqlite_*' ORDER BY type,name")?;
        Ok(statement
            .query_map([], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?)
    }
    if schema(connection)? != schema(&expected.connection)? {
        return Err(error("Backup schema does not match this app"));
    }
    Ok(())
}

/// Use SQLite's online backup transaction, which includes committed WAL pages without copying live files.
fn copy_database(source: &Connection, destination: &Path) -> Result<()> {
    let mut target = Connection::open(destination)?;
    {
        let backup = Backup::new(source, &mut target)?;
        // A single dedicated worker owns the source, so Busy/Locked fails instead of retrying forever.
        loop {
            match backup.step(128)? {
                rusqlite::backup::StepResult::Done => break,
                rusqlite::backup::StepResult::More => (),
                _ => return Err(error("Database is busy; retry the backup")),
            }
        }
    }
    target.pragma_update(None, "journal_mode", "DELETE")?;
    drop(target);
    fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(destination)
        .map_err(error)?
        .sync_all()
        .map_err(error)?;
    Ok(())
}

/// Fully validate restored content with the same note/image/session contracts used for normal writes.
fn validate_content(store: &Store) -> Result<()> {
    let connection = &store.connection;
    if connection.prepare("SELECT 1 FROM notes WHERE length(CAST(document AS BLOB))>8388608 OR length(CAST(title AS BLOB))>1024 OR length(id)<>36 OR language NOT IN ('plaintext','markdown','javascript','typescript','json','html') OR revision<1 OR revision=9223372036854775807 OR document_version<>1 LIMIT 1")?.exists([])?
        || connection.prepare("SELECT 1 FROM attachments WHERE length(bytes)>20971520 OR length(id)<>36 LIMIT 1")?.exists([])?
        || connection.prepare("SELECT 1 FROM session WHERE length(CAST(state AS BLOB))>1048576 OR version<>1 LIMIT 1")?.exists([])? {
        return Err(error("Backup contains oversized or invalid records"));
    }
    let mut statement = connection.prepare("SELECT id FROM notes")?;
    let mut rows = statement.query([])?;
    while let Some(row) = rows.next()? {
        let id = Uuid::parse_str(&row.get::<_, String>(0)?).map_err(error)?;
        if id.is_nil() {
            return Err(error("Backup contains an invalid note ID"));
        }
        let note = store.load_note(id)?;
        let (_, expected) = store.validate_note(&NoteInput {
            title: note.summary.title,
            language: note.summary.language,
            document: note.document,
        })?;
        let mut references =
            connection.prepare("SELECT attachment_id FROM note_attachments WHERE note_id=?1")?;
        let actual = references
            .query_map([id.to_string()], |row| row.get::<_, String>(0))?
            .map(|row| Uuid::parse_str(&row?).map_err(error))
            .collect::<Result<HashSet<_>>>()?;
        if actual != expected {
            return Err(error(
                "Backup image references disagree with source documents",
            ));
        }
    }
    let mut statement =
        connection.prepare("SELECT id,mime_type,width,height,bytes FROM attachments")?;
    let mut rows = statement.query([])?;
    while let Some(row) = rows.next()? {
        let id = Uuid::parse_str(&row.get::<_, String>(0)?).map_err(error)?;
        let bytes: Vec<u8> = row.get(4)?;
        let (mime, width, height) = validate_image(&bytes)?;
        if id.is_nil()
            || mime != row.get::<_, String>(1)?
            || width != row.get::<_, u32>(2)?
            || height != row.get::<_, u32>(3)?
        {
            return Err(error("Backup image metadata is invalid"));
        }
    }
    let saved = store.load_session()?;
    if let Some(state) = saved.state {
        validate_session(saved.revision, &state)?;
    }
    Ok(())
}

impl Store {
    /// Protect app-owned database, WAL, rollback journals, and automatic snapshots from export overwrite.
    pub(crate) fn export_backup(&self, path: &Path) -> Result<()> {
        let parent = path
            .parent()
            .ok_or_else(|| error("Backup destination has no parent"))?
            .canonicalize()
            .map_err(error)?;
        if parent.starts_with(self.directory.canonicalize().map_err(error)?) {
            return Err(error(
                "Export outside the app data directory to protect the live store and recovery files",
            ));
        }
        self.snapshot(path)
    }

    /// Track committed changes on the worker; rollback attempts may cause an extra safe backup, never data loss.
    pub(crate) fn changes(&self) -> u64 {
        self.connection.total_changes()
    }

    /// Publish a fully synced, checked SQLite file atomically; failures preserve any existing destination.
    pub(crate) fn snapshot(&self, path: &Path) -> Result<()> {
        let parent = path
            .parent()
            .ok_or_else(|| error("Backup destination has no parent"))?;
        let temporary = tempfile::NamedTempFile::new_in(parent).map_err(error)?;
        copy_database(&self.connection, temporary.path())?;
        validate_structure(&read_only(temporary.path())?)?;
        temporary.persist(path).map_err(error)?;
        Ok(())
    }

    /// Try after successful writes until today's snapshot succeeds; prune only after publishing a new valid snapshot.
    pub(crate) fn daily_backup(&self, day: u64) -> Result<PathBuf> {
        let directory = self.directory.join("backups");
        fs::create_dir_all(&directory).map_err(error)?;
        let path = directory.join(format!("daily-{day:010}.sqlite3"));
        if !path.exists()
            || read_only(&path)
                .and_then(|connection| validate_structure(&connection))
                .is_err()
        {
            self.snapshot(&path)?;
        }
        let mut successful = Vec::new();
        for entry in fs::read_dir(&directory).map_err(error)? {
            let entry = entry.map_err(error)?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.len() == 24
                && name.starts_with("daily-")
                && name.ends_with(".sqlite3")
                && name.as_bytes()[6..16]
                    .iter()
                    .all(|byte| byte.is_ascii_digit())
                && entry.file_type().map_err(error)?.is_file()
                && read_only(&entry.path())
                    .and_then(|connection| validate_structure(&connection))
                    .is_ok()
            {
                successful.push(entry.path());
            }
        }
        successful.sort();
        let excess = successful.len().saturating_sub(7);
        for old in successful.into_iter().take(excess) {
            fs::remove_file(old).map_err(error)?;
        }
        Ok(path)
    }

    /// Stage a consistent private copy before confirmation, preventing selected-file changes from changing the restore.
    pub(crate) fn prepare_restore(source: &Path, directory: &Path) -> Result<PreparedRestore> {
        fs::create_dir_all(directory).map_err(error)?;
        let staged = tempfile::Builder::new()
            .prefix("restore-candidate-")
            .tempdir_in(directory)
            .map_err(error)?;
        let source = read_only(source)?;
        validate_structure(&source)?;
        let path = staged.path().join("notes.sqlite3");
        copy_database(&source, &path)?;
        let candidate = Store {
            connection: read_only(&path)?,
            directory: staged.path().to_owned(),
        };
        validate_structure(&candidate.connection)?;
        validate_content(&candidate)?;
        let notes = candidate.connection.query_row(
            "SELECT count(*) FROM notes WHERE deleted_at IS NULL",
            [],
            |row| row.get(0),
        )?;
        let trashed = candidate.connection.query_row(
            "SELECT count(*) FROM notes WHERE deleted_at IS NOT NULL",
            [],
            |row| row.get(0),
        )?;
        Ok(PreparedRestore {
            directory: staged,
            token: Uuid::new_v4().to_string(),
            notes,
            trashed,
        })
    }
}

/// Copy closed database files through temporary files so rollback itself is safe to retry after a crash.
fn durable_copy(source: &Path, destination: &Path) -> Result<()> {
    let mut temporary = tempfile::NamedTempFile::new_in(
        destination
            .parent()
            .ok_or_else(|| error("Destination has no parent"))?,
    )
    .map_err(error)?;
    std::io::copy(
        &mut fs::File::open(source).map_err(error)?,
        temporary.as_file_mut(),
    )
    .map_err(error)?;
    temporary.as_file().sync_all().map_err(error)?;
    temporary.persist(destination).map_err(error)?;
    Ok(())
}

/// Recover interrupted replacement before opening SQLite; the marker accepts only an app-generated archive name.
pub(crate) fn recover_interrupted(directory: &Path) -> Result<()> {
    let marker = directory.join(RESTORE_MARKER);
    if !marker.exists() {
        return Ok(());
    }
    let name = fs::read_to_string(&marker).map_err(error)?;
    let suffix = name
        .strip_prefix("pre-restore-")
        .ok_or_else(|| error("Invalid restore recovery marker"))?;
    Uuid::parse_str(suffix).map_err(error)?;
    if name != format!("pre-restore-{}", Uuid::parse_str(suffix).map_err(error)?) {
        return Err(error("Invalid restore recovery marker"));
    }
    let archive = directory.join("backups").join(name);
    let files: Vec<String> =
        serde_json::from_slice(&fs::read(archive.join("original-files.json")).map_err(error)?)
            .map_err(error)?;
    let unique: HashSet<_> = files.iter().collect();
    if unique.len() != files.len()
        || files
            .iter()
            .any(|name| !DATABASE_FILES.contains(&name.as_str()) || !archive.join(name).is_file())
    {
        return Err(error(
            "Restore archive is incomplete; original files were not changed",
        ));
    }
    for file in DATABASE_FILES {
        let original = archive.join(file);
        let target = directory.join(file);
        if files.iter().any(|name| name == file) {
            durable_copy(&original, &target)?;
        } else if target.exists() {
            fs::remove_file(target).map_err(error)?;
        }
    }
    fs::remove_file(marker).map_err(error)?;
    Ok(())
}

/// Preserve a pre-restore snapshot/raw originals, replace while closed, and roll back any reopen failure.
pub(crate) fn replace_store(
    directory: &Path,
    current: &mut Result<Store>,
    candidate: PreparedRestore,
    fail_reopen: bool,
) -> Result<()> {
    if directory.join(RESTORE_MARKER).exists() {
        return Err(error(
            "Interrupted restore recovery must finish before another restore",
        ));
    }
    // Restored revisions must not collide with stale renderer state if the success acknowledgement is lost.
    /// Find a revision above every note/session acknowledgement in one workspace.
    fn maximum_revision(connection: &Connection) -> Result<i64> {
        Ok(connection.query_row("SELECT max(value) FROM (SELECT coalesce(max(revision),0) AS value FROM notes UNION ALL SELECT coalesce(max(revision),0) FROM session)", [], |row| row.get(0))?)
    }
    let mut staged = Connection::open(candidate.path())?;
    let previous_revision = current
        .as_ref()
        .ok()
        .map(|store| maximum_revision(&store.connection))
        .transpose()?
        .unwrap_or(0);
    let revision = maximum_revision(&staged)?
        .max(previous_revision)
        .checked_add(1)
        .filter(|revision| *revision < i64::MAX)
        .ok_or_else(|| error("Workspace revisions are exhausted; restore was not started"))?;
    let transaction = staged.transaction()?;
    transaction.execute("UPDATE notes SET revision=?1", [revision])?;
    if transaction.execute("UPDATE session SET revision=?1", [revision])? == 0 {
        // Even an absent session needs a new revision, otherwise a stale revision-zero save could recreate old tabs.
        let first: Option<String> = transaction
            .query_row(
                "SELECT id FROM notes WHERE deleted_at IS NULL ORDER BY updated_at DESC,id LIMIT 1",
                [],
                |row| row.get(0),
            )
            .optional()?;
        let active_note_id = first
            .map(|id| Uuid::parse_str(&id).map_err(error))
            .transpose()?;
        let state = Session {
            version: 1,
            tabs: active_note_id
                .map(|note_id| {
                    vec![Tab {
                        note_id,
                        pinned: false,
                        anchor: 0,
                        head: 0,
                        scroll_top: 0.0,
                    }]
                })
                .unwrap_or_default(),
            active_note_id,
            sidebar_visible: false,
            word_wrap: true,
        };
        transaction.execute(
            "INSERT INTO session VALUES(1,1,?1,?2)",
            params![serde_json::to_string(&state).map_err(error)?, revision],
        )?;
    }
    transaction.commit()?;
    drop(staged);
    let name = format!("pre-restore-{}", Uuid::new_v4());
    let archive = directory.join("backups").join(&name);
    fs::create_dir_all(&archive).map_err(error)?;
    if let Ok(store) = current {
        store.snapshot(&archive.join("workspace.sqlite3"))?;
    }
    // Dropping the only connection checkpoints its WAL before raw-file archival. Corrupt originals are preserved verbatim.
    *current = Err(error("Restore is in progress"));
    let replaced = (|| {
        let mut files = Vec::new();
        for file in DATABASE_FILES {
            let source = directory.join(file);
            if source.exists() {
                durable_copy(&source, &archive.join(file))?;
                files.push(file);
            }
        }
        let mut manifest = fs::File::create(archive.join("original-files.json")).map_err(error)?;
        serde_json::to_writer(&mut manifest, &files).map_err(error)?;
        manifest.sync_all().map_err(error)?;
        let mut marker = tempfile::NamedTempFile::new_in(directory).map_err(error)?;
        marker.write_all(name.as_bytes()).map_err(error)?;
        marker.as_file().sync_all().map_err(error)?;
        marker
            .persist(directory.join(RESTORE_MARKER))
            .map_err(error)?;
        for file in &DATABASE_FILES[1..] {
            let path = directory.join(file);
            if path.exists() {
                fs::remove_file(path).map_err(error)?;
            }
        }
        durable_copy(&candidate.path(), &directory.join("notes.sqlite3"))?;
        if fail_reopen {
            return Err(error("Injected replacement/reopen failure"));
        }
        let restored = Store::open(directory)?;
        validate_structure(&restored.connection)?;
        validate_content(&restored)?;
        fs::remove_file(directory.join(RESTORE_MARKER)).map_err(error)?;
        Ok(restored)
    })();
    match replaced {
        Ok(restored) => {
            *current = Ok(restored);
            Ok(())
        }
        Err(failure) => {
            if let Err(rollback) = recover_interrupted(directory) {
                let message = error(format!(
                    "{failure}; rollback could not finish: {rollback}. Originals remain in {}. Restart to retry recovery.",
                    archive.display()
                ));
                *current = Err(message.clone());
                return Err(message);
            }
            *current = Store::open(directory);
            Err(error(format!(
                "Restore failed; original workspace preserved: {failure}. Pre-restore files: {}",
                archive.display()
            )))
        }
    }
}

/// UTC day boundaries avoid a calendar dependency; persisted filenames prevent repeat snapshots after restart.
pub(crate) fn today() -> Result<u64> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(error)?
        .as_secs()
        / 86_400)
}

#[cfg(test)]
mod tests;

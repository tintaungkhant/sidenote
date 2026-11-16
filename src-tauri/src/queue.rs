//! One FIFO worker owns SQLite. Enqueuing happens before awaiting, so pool scheduling cannot reorder jobs.
use crate::storage::{BackupStatus, PreparedRestore, Store, StoreError, backup};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex, mpsc},
};
use tokio::sync::oneshot;

struct Workspace {
    store: Result<Store, StoreError>,
    directory: PathBuf,
}
type Job = Box<dyn FnOnce(&mut Workspace) -> bool + Send>;
#[derive(Clone)]
pub struct Storage {
    sender: mpsc::Sender<Job>,
    status: Arc<Mutex<BackupStatus>>,
}

/// Report a stopped worker without claiming that an unacknowledged write succeeded.
fn unavailable() -> StoreError {
    StoreError {
        code: "unavailable",
        message: "Storage worker stopped; keep this window open and export recovery".into(),
    }
}

impl Storage {
    /// Keep a failed startup available to the recovery UI instead of creating an empty store or terminating launch.
    pub fn open(
        directory: PathBuf,
        report: impl Fn(BackupStatus) + Send + 'static,
    ) -> Result<Self, StoreError> {
        let store = backup::recover_interrupted(&directory).and_then(|_| Store::open(&directory));
        Self::start(Workspace { store, directory }, report)
    }

    /// Accept an isolated store in queue tests without changing its app-data directory.
    #[cfg(test)]
    pub fn new(store: Store) -> Result<Self, StoreError> {
        let directory = PathBuf::from(store.database_directory());
        Self::start(
            Workspace {
                store: Ok(store),
                directory,
            },
            |_| {},
        )
    }

    /// Move the connection to one worker; successful writes trigger best-effort daily backup after acknowledgement.
    fn start(
        mut workspace: Workspace,
        report: impl Fn(BackupStatus) + Send + 'static,
    ) -> Result<Self, StoreError> {
        let (sender, receiver) = mpsc::channel::<Job>();
        let status = Arc::new(Mutex::new(BackupStatus {
            directory: workspace.directory.join("backups").display().to_string(),
            ..Default::default()
        }));
        let worker_status = status.clone();
        std::thread::Builder::new()
            .name("note-storage".into())
            .spawn(move || {
                let mut backed_up_day = None;
                for job in receiver {
                    let before = workspace.store.as_ref().ok().map(Store::changes);
                    let successful = job(&mut workspace);
                    let changed = workspace.store.as_ref().ok().map(Store::changes) != before;
                    if successful && changed {
                        let result = backup::today().and_then(|day| {
                            if backed_up_day == Some(day) {
                                return Ok(None);
                            }
                            let store = workspace.store.as_ref().map_err(Clone::clone)?;
                            let path = store.daily_backup(day)?;
                            backed_up_day = Some(day);
                            Ok(Some(path))
                        });
                        let mut state = worker_status
                            .lock()
                            .unwrap_or_else(|poisoned| poisoned.into_inner());
                        match result {
                            Ok(Some(path)) => {
                                state.last_backup = Some(path.display().to_string());
                                state.error = None;
                            }
                            Ok(None) => continue,
                            Err(error) => state.error = Some(error.to_string()),
                        }
                        let updated = state.clone();
                        drop(state);
                        report(updated);
                    }
                }
            })
            .map_err(|e| StoreError {
                code: "unavailable",
                message: e.to_string(),
            })?;
        Ok(Self { sender, status })
    }

    /// Submit synchronously and return an asynchronous acknowledgement sent only after the operation finishes.
    pub fn submit<T: Send + 'static>(
        &self,
        operation: impl FnOnce(&mut Store) -> Result<T, StoreError> + Send + 'static,
    ) -> impl std::future::Future<Output = Result<T, StoreError>> + Send + 'static {
        let (sender, receiver) = oneshot::channel();
        let submitted = self.sender.send(Box::new(move |workspace| {
            let result = match workspace.store.as_mut() {
                Ok(store) => operation(store),
                Err(error) => Err(error.clone()),
            };
            let successful = result.is_ok();
            let _ = sender.send(result);
            successful
        }));
        async move {
            submitted.map_err(|_| unavailable())?;
            receiver.await.map_err(|_| unavailable())?
        }
    }

    /// Read backup warnings independently of SQLite availability or a long-running snapshot.
    pub fn backup_status(&self) -> BackupStatus {
        self.status
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    /// Serialize replacement against all ordinary writes; private staged bytes remain alive until completion.
    pub async fn restore(&self, prepared: PreparedRestore) -> Result<(), StoreError> {
        let (sender, receiver) = oneshot::channel();
        self.sender
            .send(Box::new(move |workspace| {
                let result = backup::replace_store(
                    &workspace.directory,
                    &mut workspace.store,
                    prepared,
                    false,
                );
                let _ = sender.send(result);
                false
            }))
            .map_err(|_| unavailable())?;
        receiver.await.map_err(|_| unavailable())?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::NoteInput;

    /// Keep queue integration fixtures independent of real app state and native pickers.
    fn input(title: &str) -> NoteInput {
        serde_json::from_value(serde_json::json!({ "title": title, "language": "plaintext", "document": { "version": 1, "blocks": [{ "kind": "text", "text": title }] } })).unwrap()
    }

    /// Automatic backup failure never fails a committed write; the next successful write retries and clears the warning.
    #[test]
    fn backup_warning_and_retry_are_separate_from_saves() {
        let directory = tempfile::tempdir().unwrap();
        let (report, received) = mpsc::channel();
        std::fs::write(directory.path().join("backups"), b"block backup directory").unwrap();
        let queue = Storage::open(directory.path().to_owned(), move |status| {
            report.send(status).unwrap();
        })
        .unwrap();
        tauri::async_runtime::block_on(async {
            let note = queue
                .submit(|store| store.create_note(input("Saved despite backup error")))
                .await
                .unwrap();
            let status = received
                .recv_timeout(std::time::Duration::from_secs(5))
                .unwrap();
            assert!(status.error.is_some());
            assert_eq!(
                queue
                    .submit(move |store| store.load_note(note.summary.id))
                    .await
                    .unwrap()
                    .summary
                    .title,
                "Saved despite backup error"
            );
            assert!(queue.backup_status().error.is_some());
            std::fs::remove_file(directory.path().join("backups")).unwrap();
            queue
                .submit(|store| store.create_note(input("Retry")))
                .await
                .unwrap();
            let status = received
                .recv_timeout(std::time::Duration::from_secs(5))
                .unwrap();
            assert!(status.error.is_none());
            assert!(std::path::Path::new(&status.last_backup.unwrap()).is_file());
            queue.submit(|_| Ok(())).await.unwrap();
            assert!(queue.backup_status().error.is_none());
        });
    }

    /// A corrupt startup remains observable and recoverable; confirmed recovery preserves the unreadable original verbatim.
    #[test]
    fn unreadable_startup_can_restore_without_empty_replacement() {
        let directory = tempfile::tempdir().unwrap();
        let source_dir = tempfile::tempdir().unwrap();
        let original = b"unreadable database that must be preserved";
        std::fs::write(directory.path().join("notes.sqlite3"), original).unwrap();
        let queue = Storage::open(directory.path().to_owned(), |_| {}).unwrap();
        let mut source = Store::open(source_dir.path()).unwrap();
        let note = source.create_note(input("Recovered")).unwrap();
        let prepared = Store::prepare_restore(
            &source_dir.path().join("notes.sqlite3"),
            &directory.path().join("backups"),
        )
        .unwrap();
        tauri::async_runtime::block_on(async {
            assert!(queue.submit(|store| store.load_session()).await.is_err());
            assert_eq!(
                std::fs::read(directory.path().join("notes.sqlite3")).unwrap(),
                original
            );
            queue.restore(prepared).await.unwrap();
            assert_eq!(
                queue
                    .submit(move |store| store.load_note(note.summary.id))
                    .await
                    .unwrap()
                    .summary
                    .title,
                "Recovered"
            );
        });
        let archive = std::fs::read_dir(directory.path().join("backups"))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .find(|path| {
                path.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("pre-restore-")
            })
            .unwrap();
        assert_eq!(
            std::fs::read(archive.join("notes.sqlite3")).unwrap(),
            original
        );
    }
    /// Even awaiting in reverse order cannot reorder submitted operations; a failed job does not stop the queue.
    #[test]
    fn fifo_and_failure_barrier() {
        let directory = tempfile::tempdir().unwrap();
        let queue = Storage::new(Store::open(directory.path()).unwrap()).unwrap();
        let order = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let a = order.clone();
        let first = queue.submit(move |_| {
            std::thread::sleep(std::time::Duration::from_millis(30));
            a.lock().unwrap().push(1);
            Ok(())
        });
        let b = order.clone();
        let failed = queue.submit(move |_| {
            b.lock().unwrap().push(2);
            Err::<(), _>(StoreError {
                code: "database",
                message: "injected".into(),
            })
        });
        let c = order.clone();
        let barrier = queue.submit(move |_| {
            c.lock().unwrap().push(3);
            Ok(())
        });
        tauri::async_runtime::block_on(async {
            barrier.await.unwrap();
            assert!(failed.await.is_err());
            first.await.unwrap();
        });
        assert_eq!(*order.lock().unwrap(), vec![1, 2, 3]);
    }
}

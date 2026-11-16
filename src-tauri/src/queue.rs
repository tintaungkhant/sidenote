//! One FIFO worker owns SQLite. Enqueuing happens before awaiting, so pool scheduling cannot reorder jobs.
use crate::storage::{Store, StoreError};
use std::sync::mpsc;
use tokio::sync::oneshot;

type Job = Box<dyn FnOnce(&mut Store) + Send>;
#[derive(Clone)]
pub struct Storage {
    sender: mpsc::Sender<Job>,
}

/// Report a stopped worker without claiming that an unacknowledged write succeeded.
fn unavailable() -> StoreError {
    StoreError {
        code: "unavailable",
        message: "Storage worker stopped; keep this window open and export recovery".into(),
    }
}

impl Storage {
    /// Move the connection to a dedicated worker; queued closures execute in receive order.
    pub fn new(mut store: Store) -> Result<Self, StoreError> {
        let (sender, receiver) = mpsc::channel::<Job>();
        std::thread::Builder::new()
            .name("note-storage".into())
            .spawn(move || {
                for job in receiver {
                    job(&mut store);
                }
            })
            .map_err(|e| StoreError {
                code: "unavailable",
                message: e.to_string(),
            })?;
        Ok(Self { sender })
    }

    /// Submit synchronously and return an asynchronous acknowledgement sent only after the operation finishes.
    pub fn submit<T: Send + 'static>(
        &self,
        operation: impl FnOnce(&mut Store) -> Result<T, StoreError> + Send + 'static,
    ) -> impl std::future::Future<Output = Result<T, StoreError>> + Send + 'static {
        let (sender, receiver) = oneshot::channel();
        let submitted = self.sender.send(Box::new(move |store| {
            let _ = sender.send(operation(store));
        }));
        async move {
            submitted.map_err(|_| unavailable())?;
            receiver.await.map_err(|_| unavailable())?
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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

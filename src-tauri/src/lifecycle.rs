//! Normal close and explicit quit share an idempotent renderer-flush handshake.
use crate::{queue::Storage, storage::StoreError};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use tauri::{Emitter, Manager, State};

#[derive(Default)]
pub struct ExitState {
    phase: AtomicU8,
    renderer_ready: AtomicBool,
}
impl ExitState {
    /// Allow actual termination only after the queue barrier or an explicit discard decision.
    pub fn approved(&self) -> bool {
        self.phase.load(Ordering::SeqCst) == 2
    }
    /// Start one close attempt. Repeated OS close/quit events do not trigger duplicate flushes.
    fn request(&self) -> bool {
        if self
            .phase
            .compare_exchange(0, 1, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return false;
        }
        if !self.renderer_ready.load(Ordering::SeqCst) {
            // The renderer acknowledges readiness before loading notes or admitting edits, so no local work exists yet.
            self.phase.store(2, Ordering::SeqCst);
        }
        true
    }
    /// Identify an active flush handshake after the renderer has admitted workspace loading.
    fn pending(&self) -> bool {
        self.phase.load(Ordering::SeqCst) == 1
    }
    /// Leave the app open after a failed flush, permitting another close attempt.
    fn cancel(&self) {
        let _ = self
            .phase
            .compare_exchange(1, 0, Ordering::SeqCst, Ordering::SeqCst);
    }
}

/// Exit directly before renderer readiness, when edits are impossible; otherwise request an acknowledged flush.
pub fn request(app: &tauri::AppHandle) {
    let state = app.state::<ExitState>();
    if state.request() {
        if state.approved() {
            app.exit(0);
        } else {
            let _ = app.emit_to("main", "flush-before-exit", ());
        }
    }
}
/// Support the explicit Quit button without bypassing the native close guard.
#[tauri::command]
pub fn request_exit(app: tauri::AppHandle) {
    request(&app);
}
/// Acknowledge the installed listener before loading notes or admitting edits; report any pending flush request.
#[tauri::command]
pub fn exit_ready(state: State<'_, ExitState>) -> bool {
    state.renderer_ready.store(true, Ordering::SeqCst);
    state.pending()
}
/// Release a failed close attempt while preserving all in-memory edits.
#[tauri::command]
pub fn cancel_exit(state: State<'_, ExitState>) {
    state.cancel();
}
/// Drain earlier jobs before exit. The renderer calls this only after flush or explicit discard confirmation.
#[tauri::command]
pub async fn finish_exit(
    app: tauri::AppHandle,
    storage: State<'_, Storage>,
    state: State<'_, ExitState>,
    discard: bool,
) -> Result<(), StoreError> {
    if !state.pending() && !discard {
        return Err(StoreError {
            code: "validation",
            message: "No close attempt is pending".into(),
        });
    }
    // Discard still waits for submitted commits; it never interrupts SQLite mid-transaction.
    if let Err(error) = storage.submit(|_| Ok(())).await {
        // A stopped worker has no remaining commits to drain. Only explicit discard may exit in that case.
        if !discard {
            return Err(error);
        }
    }
    state.phase.store(2, Ordering::SeqCst);
    app.exit(0);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    /// Repeated requests, failed retries, and startup handoff never accidentally authorize termination.
    #[test]
    fn close_handshake() {
        let state = ExitState::default();
        assert!(!state.approved());
        state.renderer_ready.store(true, Ordering::SeqCst);
        assert!(state.request());
        assert!(!state.request());
        assert!(state.pending());
        state.cancel();
        assert!(!state.pending());
        assert!(!state.approved());
        assert!(state.request());
        state.phase.store(2, Ordering::SeqCst);
        state.cancel();
        assert!(state.approved());
        assert!(!state.request());
    }
    /// Before listener readiness there can be no editable state, so close must not wait for JavaScript startup.
    #[test]
    fn startup_close_without_renderer() {
        let state = ExitState::default();
        assert!(state.request());
        assert!(state.approved());
        assert!(!state.pending());
        assert!(!state.request());
    }
}

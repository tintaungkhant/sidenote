/** One close attempt freezes edits, flushes all state, and only then authorizes native exit. */
export class ExitGuard {
  private pending?: Promise<void>;
  private complete = false;
  /** Keep lifecycle operations injectable so repeated close and write failures can be tested without a browser. */
  constructor(private actions: {
    freeze: (frozen: boolean) => void;
    prepare: () => Promise<void>;
    flush: () => Promise<void>;
    finish: (discard: boolean) => Promise<void>;
    cancel: () => Promise<void>;
    failed: (error: unknown) => void;
  }) {}

  /** Keep other operations from unfreezing editing while a close attempt owns the final snapshot. */
  get busy(): boolean { return !!this.pending || this.complete; }

  /** Repeated close events share one flush. Failure unfreezes the editor and keeps all edits available. */
  request(discard = false): Promise<void> {
    if (this.pending) return this.pending;
    if (this.complete) return Promise.resolve();
    this.actions.freeze(true);
    this.pending = (async () => {
      try {
        await this.actions.prepare();
        if (!discard) await this.actions.flush();
        await this.actions.finish(discard);
        this.complete = true;
      } catch (error) {
        try { await this.actions.cancel(); } catch { /* The native guard still prevents termination if cancellation fails. */ }
        this.actions.failed(error);
        this.actions.freeze(false);
      } finally { this.pending = undefined; }
    })();
    return this.pending;
  }
}

/** Autosave owns dirty generations and acknowledged revisions. It never replaces current edits with a save response. */
import type { Note, NoteId, NoteInput, SavedSession, Session } from './storage';

type Slot<T> = { value: T; revision: number; generation: number; acknowledged: number };
export type SaveStatus = 'Saved' | 'Saving' | 'Error';
export type SaveTransport = {
  updateNote: (id: NoteId, revision: number, input: NoteInput) => Promise<Note>;
  saveSession: (revision: number, state: Session) => Promise<SavedSession>;
};
export type Timer = {
  set: (callback: () => void, delay: number) => unknown;
  clear: (handle: unknown) => void;
};
const timer: Timer = { set: (callback, delay) => setTimeout(callback, delay), clear: handle => clearTimeout(handle as ReturnType<typeof setTimeout>) };

export class Autosave {
  private notes = new Map<NoteId, Slot<NoteInput>>();
  private session?: Slot<Session>;
  private sessionRevision: number;
  private scheduled: unknown;
  private pending?: Promise<void>;
  private stopped = false;
  private paused = false;
  error: unknown;

  /** Accept transport and timer seams for headless failure/latency tests; loaded state starts clean. */
  constructor(private transport: SaveTransport, savedSession: SavedSession, private changed: () => void, private clock: Timer = timer) {
    this.sessionRevision = savedSession.revision;
    if (savedSession.state) this.session = { value: savedSession.state, revision: savedSession.revision, generation: 0, acknowledged: 0 };
  }

  /** Register a loaded note once; registration must never overwrite locally edited state. */
  register(note: Note): void {
    if (this.notes.has(note.id)) throw new Error('Note is already registered');
    this.notes.set(note.id, { value: { title: note.title, language: note.language, document: note.document }, revision: note.revision, generation: 0, acknowledged: 0 });
  }

  /** Stage immutable current content. Annotation completions may request an immediate commit attempt. */
  edit(id: NoteId, value: NoteInput, immediate = false): void {
    const slot = this.notes.get(id);
    if (!slot) throw new Error('Note is not registered');
    slot.value = value;
    slot.generation++;
    this.changed();
    this.schedule(immediate);
  }

  /** Stage session changes without scheduling writes for identical selection/scroll/preferences. */
  setSession(value: Session): void {
    if (this.session && JSON.stringify(value) === JSON.stringify(this.session.value)) return;
    if (!this.session) this.session = { value, revision: this.sessionRevision, generation: 0, acknowledged: 0 };
    this.session.value = value;
    this.session.generation++;
    this.changed();
    this.schedule(false);
  }

  /** Report Saved only when every local generation has been acknowledged and no save failed. */
  get status(): SaveStatus { return this.error ? 'Error' : this.dirty ? 'Saving' : 'Saved'; }
  /** Include every registered note and session in lifecycle flush decisions. */
  get dirty(): boolean {
    return [...this.notes.values()].some(slot => slot.generation !== slot.acknowledged)
      || !!this.session && this.session.generation !== this.session.acknowledged;
  }

  /** Use a fixed 250 ms batch window from the first edit, never a debounce reset by sustained typing. */
  private schedule(immediate: boolean): void {
    if (this.stopped || this.paused) return;
    if (immediate) { void this.flush().catch(() => {}); return; }
    if (this.scheduled !== undefined) return;
    this.scheduled = this.clock.set(() => { this.scheduled = undefined; void this.flush().catch(() => {}); }, 250);
  }

  /** Cancel pending delay, but preserve content and in-flight acknowledgement tracking. */
  private clearTimer(): void {
    if (this.scheduled !== undefined) this.clock.clear(this.scheduled);
    this.scheduled = undefined;
  }

  /** Flush all generations in order. Repeated close/retry calls share the same in-flight promise. */
  flush(): Promise<void> {
    this.clearTimer();
    if (this.pending) return this.pending;
    this.error = undefined;
    this.changed();
    this.pending = this.drain().catch(error => { this.error = error; throw error; }).finally(() => {
      this.pending = undefined;
      this.changed();
      if (this.dirty && !this.error && !this.stopped) this.schedule(false);
    });
    return this.pending;
  }

  /** Acknowledge the captured generation only; edits made during an await stay dirty and save next. */
  private async drain(): Promise<void> {
    while (this.dirty) {
      for (const [id, slot] of this.notes) {
        if (slot.generation === slot.acknowledged) continue;
        const generation = slot.generation;
        const result = await this.transport.updateNote(id, slot.revision, structuredClone(slot.value));
        slot.revision = result.revision;
        slot.acknowledged = generation;
        this.changed();
      }
      const slot = this.session;
      if (slot && slot.generation !== slot.acknowledged) {
        const generation = slot.generation;
        const result = await this.transport.saveSession(slot.revision, structuredClone(slot.value));
        slot.revision = result.revision;
        slot.acknowledged = generation;
        this.changed();
      }
    }
  }

  /** Capture current source, including unsaved generations, for a separate emergency export. */
  snapshot(): string {
    return JSON.stringify({ notes: [...this.notes].map(([id, slot]) => ({ id, revision: slot.revision, input: slot.value })), session: this.session?.value ?? null });
  }

  /** Pause automatic scheduling during close/export; explicit flush still drains every dirty generation. */
  pause(paused: boolean): void {
    this.paused = paused;
    if (paused) this.clearTimer();
    else if (this.dirty && !this.error) this.schedule(false);
  }

  /** Stop delayed retries before teardown without erasing dirty state or interrupting native commits. */
  stop(): void { this.stopped = true; this.clearTimer(); }
}

/** Format native structured errors and transport failures without discarding their useful message. */
export function errorMessage(error: unknown): string {
  if (typeof error === 'object' && error !== null && 'message' in error) return String(error.message);
  return String(error);
}

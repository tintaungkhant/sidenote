import { describe, expect, test } from 'bun:test';
import { Autosave, type SaveTransport, type Timer } from './autosave';
import { ExitGuard } from './exit';
import type { Note, NoteId, NoteInput, Session } from './storage';

/** Advance promises without real clocks, browser APIs, or UI inspection. */
async function settle() { for (let i = 0; i < 12; i++) await Promise.resolve(); }
/** Control delayed native acknowledgements explicitly in ordering and close tests. */
function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
class Clock implements Timer {
  now = 0;
  private next = 0;
  private jobs = new Map<number, { at: number; callback: () => void }>();
  /** Schedule work at an exact virtual time so sustained-input tests do not rely on machine speed. */
  set(callback: () => void, delay: number) { const id = ++this.next; this.jobs.set(id, { at: this.now + delay, callback }); return id; }
  /** Cancel a pending batch when lifecycle flush takes ownership. */
  clear(handle: unknown) { this.jobs.delete(handle as number); }
  /** Run due timers in chronological order, allowing acknowledgements to settle after each callback. */
  async advance(duration: number) {
    const target = this.now + duration;
    for (;;) {
      const next = [...this.jobs].sort((a, b) => a[1].at - b[1].at)[0];
      if (!next || next[1].at > target) break;
      this.now = next[1].at;
      this.jobs.delete(next[0]);
      next[1].callback();
      await settle();
    }
    this.now = target;
    await settle();
  }
}
const id = '00000000-0000-4000-8000-000000000001' as NoteId;
const other = '00000000-0000-4000-8000-000000000002' as NoteId;
/** Build source fixtures that distinguish stale acknowledgements from current local edits. */
function input(text: string): NoteInput { return { title: 'Untitled', language: 'plaintext', document: { version: 1, blocks: [{ kind: 'text', text }] } }; }
/** Build a native-style response at a specific revision without mutating caller input. */
function note(value = input(''), revision = 1, noteId = id): Note {
  return { ...value, id: noteId, revision, createdAt: 1, updatedAt: 1, deletedAt: null };
}
/** Build session state with a view preference that can change during a delayed write. */
function session(wordWrap = false): Session {
  return { version: 1, tabs: [{ noteId: id, pinned: false, anchor: 0, head: 0, scrollTop: 0 }], activeNoteId: id, sidebarVisible: false, wordWrap };
}
/** Use immediate acknowledgements unless a test explicitly controls one transport operation. */
function transport(overrides: Partial<SaveTransport> = {}): SaveTransport {
  return { updateNote: async (id, revision, value) => note(value, revision + 1, id), saveSession: async (revision, state) => ({ revision: revision + 1, state }), ...overrides };
}

describe('autosave ordering and timing', () => {
  /** Sustained input must trigger commits throughout typing, rather than only after a quiet interval. */
  test('continuous typing keeps a bounded batch deadline', async () => {
    const clock = new Clock();
    const times: number[] = [];
    const saver = new Autosave(transport({ updateNote: async (id, revision, value) => { times.push(clock.now); return note(value, revision + 1, id); } }), { revision: 0, state: null }, () => {}, clock);
    saver.register(note());
    for (let i = 0; i < 40; i++) { saver.edit(id, input(String(i))); await clock.advance(100); }
    await clock.advance(250);
    expect(times[0]).toBe(250);
    expect(times.length).toBeGreaterThan(10);
    expect(times.slice(1).every((time, i) => time - times[i] <= 1000)).toBe(true);
    expect(saver.status).toBe('Saved');
  });

  /** A response containing old content must acknowledge only its own generation and advance the next expected revision. */
  test('delayed acknowledgements never overwrite newer source or falsely report Saved', async () => {
    const responses = [deferred<Note>(), deferred<Note>()];
    const sent: { revision: number; value: NoteInput }[] = [];
    const saver = new Autosave(transport({ updateNote: (_id, revision, value) => { sent.push({ revision, value }); return responses[sent.length - 1].promise; } }), { revision: 0, state: null }, () => {});
    saver.register(note());
    saver.edit(id, input('first'));
    const pending = saver.flush();
    saver.edit(id, input('latest'));
    expect(saver.flush()).toBe(pending);
    responses[0].resolve(note(input('first'), 2));
    await settle();
    expect(saver.status).toBe('Saving');
    expect(JSON.parse(saver.snapshot()).notes[0].input.document.blocks[0].text).toBe('latest');
    expect(sent.map(write => write.revision)).toEqual([1, 2]);
    responses[1].resolve(note(input('latest'), 3));
    await pending;
    expect(saver.status).toBe('Saved');
    saver.stop();
  });

  /** Save failure preserves both source and session and retries against the last successful revision. */
  test('failed writes retain dirty state and retry safely', async () => {
    let fail = true;
    const revisions: number[] = [];
    const saver = new Autosave(transport({ updateNote: async (id, revision, value) => { revisions.push(revision); if (fail) throw { code: 'database', message: 'disk full' }; return note(value, revision + 1, id); } }), { revision: 0, state: null }, () => {});
    saver.register(note());
    saver.edit(id, input('keep me'));
    saver.setSession(session(true));
    await expect(saver.flush()).rejects.toMatchObject({ code: 'database' });
    expect(saver.status).toBe('Error');
    expect(saver.dirty).toBe(true);
    const snapshot = JSON.parse(saver.snapshot());
    expect(snapshot.notes[0].input.document.blocks[0].text).toBe('keep me');
    expect(snapshot.session.wordWrap).toBe(true);
    fail = false;
    await saver.flush();
    expect(revisions).toEqual([1, 1]);
    expect(saver.status).toBe('Saved');
    saver.stop();
  });

  /** Delayed session writes follow the same generation rules as note writes. */
  test('session changes during a write remain dirty until their own acknowledgement', async () => {
    const responses = [deferred<{ revision: number; state: Session }>(), deferred<{ revision: number; state: Session }>()];
    const expected: number[] = [];
    const saver = new Autosave(transport({ saveSession: (revision) => { expected.push(revision); return responses[expected.length - 1].promise; } }), { revision: 0, state: null }, () => {});
    saver.setSession(session());
    const pending = saver.flush();
    saver.setSession(session(true));
    responses[0].resolve({ revision: 1, state: session() });
    await settle();
    expect(saver.status).toBe('Saving');
    expect(expected).toEqual([0, 1]);
    responses[1].resolve({ revision: 2, state: session(true) });
    await pending;
    expect(saver.status).toBe('Saved');
    expect(JSON.parse(saver.snapshot()).session.wordWrap).toBe(true);
    saver.stop();
  });

  /** Annotation completions bypass delay, and multiple dirty notes all participate in a single flush. */
  test('immediate edits and per-note revisions work across multiple notes', async () => {
    const writes: { id: NoteId; revision: number }[] = [];
    const saver = new Autosave(transport({ updateNote: async (id, revision, value) => { writes.push({ id, revision }); return note(value, revision + 1, id); } }), { revision: 0, state: null }, () => {});
    saver.register(note()); saver.register(note(input('other'), 8, other));
    saver.edit(other, input('other edited'));
    saver.edit(id, input('annotation completed'), true);
    expect(writes[0].id).toBe(id);
    await saver.flush();
    expect(writes).toEqual([{ id, revision: 1 }, { id: other, revision: 8 }]);
    expect(saver.status).toBe('Saved');
    saver.stop();
  });

  /** Lifecycle pause cancels timers but explicit flush still commits the final state. */
  test('pause and stop preserve recovery content without delayed writes', async () => {
    const clock = new Clock();
    let writes = 0;
    const saver = new Autosave(transport({ updateNote: async (id, revision, value) => { writes++; return note(value, revision + 1, id); } }), { revision: 0, state: null }, () => {}, clock);
    saver.register(note()); saver.edit(id, input('last edit')); saver.pause(true);
    await clock.advance(2000);
    expect(writes).toBe(0);
    await saver.flush();
    expect(writes).toBe(1);
    saver.pause(false); saver.edit(id, input('recover after teardown')); saver.stop();
    await clock.advance(2000);
    expect(writes).toBe(1);
    expect(JSON.parse(saver.snapshot()).notes[0].input.document.blocks[0].text).toBe('recover after teardown');
  });
});

describe('close lifecycle', () => {
  /** Repeated requests wait for startup, then flush exactly once before authorizing native termination. */
  test('close waits for preparation and acknowledgement', async () => {
    const prepared = deferred<void>();
    const committed = deferred<void>();
    const calls: string[] = [];
    const guard = new ExitGuard({ freeze: value => calls.push(`freeze:${value}`), prepare: () => prepared.promise, flush: () => { calls.push('flush'); return committed.promise; }, finish: async () => { calls.push('exit'); }, cancel: async () => { calls.push('cancel'); }, failed: () => { calls.push('failed'); } });
    const first = guard.request();
    expect(guard.request()).toBe(first);
    expect(calls).toEqual(['freeze:true']);
    prepared.resolve(); await settle();
    expect(calls).toEqual(['freeze:true', 'flush']);
    committed.resolve(); await first;
    expect(calls).toEqual(['freeze:true', 'flush', 'exit']);
  });

  /** A failed native write keeps content available and permits a later successful close. */
  test('failed close keeps edits open; retry flushes the final note and session', async () => {
    let fail = true;
    const durable: NoteInput[] = [];
    const saver = new Autosave(transport({ updateNote: async (id, revision, value) => { if (fail) throw new Error('write failed'); durable.push(value); return note(value, revision + 1, id); } }), { revision: 0, state: null }, () => {});
    saver.register(note()); saver.edit(id, input('final keystroke')); saver.setSession(session(true));
    let exits = 0; let cancelled = 0; let frozen = false;
    const guard = new ExitGuard({ freeze: value => { frozen = value; saver.pause(value); }, prepare: async () => {}, flush: () => saver.flush(), finish: async () => { exits++; expect(saver.status).toBe('Saved'); }, cancel: async () => { cancelled++; }, failed: () => {} });
    await guard.request();
    expect(exits).toBe(0); expect(cancelled).toBe(1); expect(frozen).toBe(false);
    expect(JSON.parse(saver.snapshot()).notes[0].input.document.blocks[0].text).toBe('final keystroke');
    fail = false;
    await guard.request();
    expect(exits).toBe(1); expect(durable[0]).toEqual(input('final keystroke'));
    saver.stop();
  });

  /** Discard is a separate explicit path that never claims failed edits were committed. */
  test('confirmed discard skips flush but still waits for native exit acknowledgement', async () => {
    const calls: string[] = [];
    const guard = new ExitGuard({ freeze: () => {}, prepare: async () => { calls.push('prepared'); }, flush: async () => { throw new Error('should not flush on discard'); }, finish: async discard => { calls.push(`discard:${discard}`); }, cancel: async () => {}, failed: () => { calls.push('failed'); } });
    await guard.request(true);
    expect(calls).toEqual(['prepared', 'discard:true']);
  });
});

import { expect, test } from 'bun:test';
import { automaticTitle, changeTab, closeTab, emptySession, normalizeSession, openTab, tabShortcut } from './tabs';
import type { NoteId } from './storage';
const a = 'a' as NoteId, b = 'b' as NoteId, c = 'c' as NoteId;
test('normalization drops duplicates and stale references; pins precede other tabs', () => {
  let s = openTab(openTab(emptySession(), a), b);
  s = changeTab(s, b, 'pin');
  s.tabs.push(s.tabs[0], { noteId: c, pinned: false, anchor: 0, head: 0, scrollTop: 0 });
  expect(normalizeSession(s, new Set([a, b])).tabs.map(t => t.noteId)).toEqual([b, a]);
});
test('close preserves view metadata for reopen, selects neighbor, and supports empty sessions', () => {
  const s = openTab(openTab(emptySession(), a), b);
  s.tabs[1].anchor = 9;
  const closed = closeTab(s, b);
  expect(closed.activeNoteId).toBe(a);
  expect(openTab(closed, b, s.tabs[1]).tabs[1].anchor).toBe(9);
  expect(closeTab(closeTab(s, a), b).activeNoteId).toBeNull();
  expect(normalizeSession(emptySession(), new Set([a])).tabs).toEqual([]);
});
test('opening is unique; reordering cannot cross pinned boundary', () => {
  let s = openTab(openTab(openTab(emptySession(), a), b), c);
  s = changeTab(s, a, 'pin');
  expect(changeTab(s, b, -1).tabs).toEqual(s.tabs);
  expect(changeTab(s, c, -1).tabs.map(t => t.noteId)).toEqual([a, c, b]);
  expect(openTab(s, a).tabs.length).toBe(3);
});
test('titles use first text and remain within native byte limit', () => {
  expect(automaticTitle({ version: 1, blocks: [{ kind: 'text', text: '\n Hello\nworld' }] })).toBe('Hello');
  expect(automaticTitle({ version: 1, blocks: [{ kind: 'text', text: '😀'.repeat(500) }] }).length).toBe(160);
});
test('shortcuts preserve native unrelated keys and IME composition', () => {
  const e = { key: 'w', ctrlKey: true, metaKey: false, altKey: false, shiftKey: false, isComposing: false };
  expect(tabShortcut(e)).toBe('close');
  expect(tabShortcut({ ...e, isComposing: true })).toBeUndefined();
  expect(tabShortcut({ ...e, altKey: true })).toBeUndefined();
  expect(tabShortcut({ ...e, key: 't', shiftKey: true, metaKey: true })).toBe('reopen');
  expect(tabShortcut({ ...e, key: 'Tab', shiftKey: true })).toBe('previous');
  expect(tabShortcut({ ...e, key: 's' })).toBeUndefined();
});

/** Cached CodeMirror states retain independent history without constructing any DOM views. */
test('switching cached states preserves independent undo and selection', async () => {
  const { createNoteState } = await import('./editor/state');
  const { undo } = await import('@codemirror/commands');
  const cache = new Map<NoteId, import('@codemirror/state').EditorState>();
  const initial = () => createNoteState({ version: 1, blocks: [{ kind: 'text', text: '' }] });
  cache.set(a, initial().update({ changes: { from: 0, insert: 'first' }, selection: { anchor: 3 } }).state);
  cache.set(b, initial().update({ changes: { from: 0, insert: 'second' } }).state);
  let active = cache.get(a)!;
  expect(active.selection.main.anchor).toBe(3);
  expect(undo({ state: active, dispatch: transaction => { active = transaction.state; } })).toBe(true);
  expect(active.doc.toString()).toBe('');
  expect(cache.get(b)!.doc.toString()).toBe('second');
});

/** Immediate lifecycle flush must commit the final pin/order snapshot, not the previous arrangement. */
test('immediate exit after pin and reorder saves exact session', async () => {
  const { Autosave } = await import('./autosave');
  let saved = emptySession();
  const saver = new Autosave({
    updateNote: async () => { throw new Error('No note writes expected'); },
    saveSession: async (revision, state) => { saved = state; return { revision: revision + 1, state }; }
  }, { revision: 0, state: null }, () => {});
  let s = openTab(openTab(openTab(emptySession(), a), b), c);
  s = changeTab(changeTab(s, c, 'pin'), b, -1);
  s.sidebarVisible = true;
  saver.setSession(s);
  await saver.flush();
  expect(saved).toEqual(s);
  expect(saved.tabs.map(tab => tab.noteId)).toEqual([c, b, a]);
  expect(saver.status).toBe('Saved');
  saver.stop();
});

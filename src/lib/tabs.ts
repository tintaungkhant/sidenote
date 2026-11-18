/** Pure tab transitions keep ordering, uniqueness, and active-note membership consistent. */
import type { NoteDocument } from './editor/document';
import type { NoteId, Session, SessionTab } from './storage';
export const emptySession = (): Session => ({ version: 1, tabs: [], activeNoteId: null, sidebarVisible: false, wordWrap: true });
/** Ignore stale/deleted IDs and duplicates without changing any stored note. */
export function normalizeSession(session: Session, available: ReadonlySet<NoteId>): Session {
  const seen = new Set<NoteId>();
  const tabs = session.tabs.filter(tab => available.has(tab.noteId) && !seen.has(tab.noteId) && !!seen.add(tab.noteId));
  tabs.sort((a, b) => Number(b.pinned) - Number(a.pinned));
  return { ...session, tabs, activeNoteId: tabs.some(tab => tab.noteId === session.activeNoteId) ? session.activeNoteId : tabs[0]?.noteId ?? null };
}
/** Opening an existing tab activates it; a new unpinned tab follows existing tabs. */
export function openTab(session: Session, id: NoteId, restored?: SessionTab): Session {
  const tabs = session.tabs.some(tab => tab.noteId === id) ? session.tabs : [...session.tabs, restored ?? { noteId: id, pinned: false, anchor: 0, head: 0, scrollTop: 0 }];
  return normalizeSession({ ...session, tabs, activeNoteId: id }, new Set(tabs.map(tab => tab.noteId)));
}
/** Closing a tab selects its nearest neighbor and never deletes the note. */
export function closeTab(session: Session, id: NoteId): Session {
  const index = session.tabs.findIndex(tab => tab.noteId === id);
  const tabs = session.tabs.filter(tab => tab.noteId !== id);
  return { ...session, tabs, activeNoteId: session.activeNoteId === id ? tabs[Math.min(index, tabs.length - 1)]?.noteId ?? null : session.activeNoteId };
}
/** Pinning preserves relative order within each group; moving cannot cross the pin boundary. */
export function changeTab(session: Session, id: NoteId, action: 'pin' | -1 | 1): Session {
  const tabs = session.tabs.map(tab => ({ ...tab }));
  const index = tabs.findIndex(tab => tab.noteId === id);
  if (index < 0) return session;
  if (action === 'pin') tabs[index].pinned = !tabs[index].pinned;
  else {
    const target = index + action;
    if (tabs[target]?.pinned === tabs[index].pinned) [tabs[index], tabs[target]] = [tabs[target], tabs[index]];
  }
  return normalizeSession({ ...session, tabs }, new Set(tabs.map(tab => tab.noteId)));
}
/** Untitled notes derive a bounded title from their first nonblank text line until explicitly renamed. */
export function automaticTitle(document: NoteDocument): string {
  for (const block of document.blocks) if (block.kind === 'text') {
    const line = block.text.split(/\r?\n/).map(line => line.trim()).find(Boolean);
    if (line) return Array.from(line).slice(0, 80).join('');
  }
  return 'Untitled';
}
/** Decode app shortcuts without swallowing composition, Alt combinations, or unrelated native keys. */
export function tabShortcut(event: Pick<KeyboardEvent, 'key' | 'ctrlKey' | 'metaKey' | 'altKey' | 'shiftKey' | 'isComposing'>): 'new' | 'close' | 'reopen' | 'next' | 'previous' | 'sidebar' | 'explorer' | 'search' | 'git' | 'timeline' | 'wrap' | 'quit' | undefined {
  if (event.isComposing) return;
  const key = event.key.toLowerCase();
  if (event.altKey) return !event.ctrlKey && !event.metaKey && !event.shiftKey && key === 'z' ? 'wrap' : undefined;
  if (event.ctrlKey && !event.metaKey && key === 'tab') return event.shiftKey ? 'previous' : 'next';
  if (!(event.ctrlKey || event.metaKey)) return;
  if (event.shiftKey) return ({ t: 'reopen', e: 'explorer', f: 'search', g: 'git', h: 'timeline' } as const)[key as 't'];
  return ({ n: 'new', w: 'close', b: 'sidebar', q: 'quit' } as const)[key as 'n'];
}

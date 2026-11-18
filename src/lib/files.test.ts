import { test, expect } from 'bun:test';
import {
  untitled, opened, modified, closeDecision, fingerprint, reconcile, removeBuffer, languageFor, recovered,
  nextUntitledName, withinRoot, samePath, relativeToRoot, displayPath, searchWorkspace, bufferLanguage,
  preferredFileName, validateFileName, type Workspace
} from './files';

test('Untitled, explicit save acknowledgement, modification, close and cancellation decisions', () => {
  const b = untitled();
  expect(b.path).toBeNull(); expect(closeDecision(b)).toBe('close');
  b.text = 'work'; expect(modified(b)).toBe(true); expect(closeDecision(b)).toBe('prompt');
  const saved = opened({ path: '/example/example.ts', text: b.text, metadata: b.metadata, revision: '1' });
  expect(modified(saved)).toBe(false);
  saved.metadata = { ...saved.metadata, images: { './assets/a.png': { id: 'stable', annotations: [{ kind: 'caption', text: 'caption' }] } } };
  expect(modified(saved)).toBe(true);
});
test('external updates reload clean buffers but retain dirty work, including deletion', () => {
  const disk = { path: '/file.txt', text: 'old', metadata: untitled().metadata, revision: '1' };
  const clean = opened(disk), updated = { ...disk, text: 'new', revision: '2' };
  expect(reconcile(clean,updated).text).toBe('new');
  const dirty = { ...clean, text: 'local edits' };
  expect(reconcile(dirty,updated)).toMatchObject({ text: 'local edits', revision: '1', external: 'changed' });
  expect(reconcile(dirty,null)).toMatchObject({ text: 'local edits', path: '/file.txt', external: 'deleted' });
  expect(reconcile(clean,null).external).toBe('deleted');
});
test('hot exit round trips all buffers, image metadata, view positions and workspace preferences', () => {
  const b = { ...untitled(), text: '[image:recovery:assets/a.png]', anchor: 4, head: 7, scrollTop: 90, pinned: true };
  b.metadata.images['recovery:assets/a.png'] = { id: 'stable', annotations: [{ kind: 'caption', text: 'caption' }] };
  const workspace: Workspace = { version: 1, buffers: [b], active: b.id, root: '/workspace', wrap: false, sidebar: true };
  const recovered: Workspace = JSON.parse(JSON.stringify(workspace));
  expect(recovered).toEqual(workspace); expect(modified(recovered.buffers[0])).toBe(true);
  expect(removeBuffer(recovered,b.id)).toMatchObject({ buffers: [], active: null });
  expect(workspace.buffers).toHaveLength(1);
});
test('language is selected by extension', () => {
  expect(['a.MD','a.txt','a.js','a.ts','a.php','a.html','a.css','a.json','a.unknown'].map(languageFor)).toEqual(['markdown','plaintext','javascript','typescript','php','html','css','json','plaintext']);
});
test('native map key reordering does not mark saved or recovered content as modified', () => {
  const buffer = untitled();
  buffer.metadata = JSON.parse('{"images":{},"version":1}');
  expect(modified(buffer)).toBe(false);
  const a = { version: 1 as const, images: { './a.png': { id: 'a', annotations: [] } } };
  const b = { images: { './a.png': { annotations: [], id: 'a' } }, version: 1 as const };
  expect(fingerprint('source',a)).toBe(fingerprint('source',b));
});
test('recovery rejects malformed or duplicate buffers and preserves the clean baseline across IPC ordering', () => {
  const buffer = untitled();
  const workspace: Workspace = { version: 1, buffers: [buffer], active: buffer.id, root: null, wrap: true, sidebar: false };
  const native = JSON.parse(JSON.stringify(workspace));
  native.buffers[0].metadata = { images: {}, version: 1 };
  expect(modified(recovered(native).buffers[0])).toBe(false);
  expect(() => recovered({ ...workspace, buffers: [buffer,buffer] })).toThrow();
  expect(() => recovered({ ...workspace, active: 'missing' })).toThrow();
  expect(() => recovered({ ...workspace, buffers: [{ ...buffer, anchor: 0.5 }] })).toThrow();
  const named = { ...untitled('Untitled-1'), text: 'draft' };
  expect(recovered({ ...workspace, buffers: [named], active: named.id }).buffers[0].name).toBe('Untitled-1');
  expect(() => recovered({ ...workspace, buffers: [{ ...named, name: 'a'.repeat(256) }], active: named.id })).toThrow();
});
test('nextUntitledName skips taken open tabs and root files', () => {
  expect(nextUntitledName([])).toBe('Untitled-1');
  expect(nextUntitledName([untitled('Untitled-1')], [{ relative: 'Untitled-2.md', directory: false }, { relative: 'sub/Untitled-3.md', directory: false }])).toBe('Untitled-3');
  expect(nextUntitledName([{ path: 'C:\\notes\\Untitled-1.md', name: undefined }])).toBe('Untitled-2');
});
test('Windows verbatim and prefix paths compare accurately in withinRoot, samePath, and relativeToRoot', () => {
  const root = 'C:\\Users\\ryan\\notes';
  expect(displayPath('\\\\?\\C:\\Users\\ryan\\notes\\a.md')).toBe('C:\\Users\\ryan\\notes\\a.md');
  expect(withinRoot('\\\\?\\C:\\Users\\ryan\\NOTES\\sub\\a.md', root)).toBe(true);
  expect(withinRoot('C:\\Users\\ryan\\notes-2\\a.md', root)).toBe(false);
  expect(samePath('\\\\?\\C:\\Users\\ryan\\notes\\a.md', 'c:/Users/ryan/notes/a.md')).toBe(true);
  expect(relativeToRoot('\\\\?\\C:\\Users\\ryan\\notes\\sub\\a.md', root)).toBe('sub/a.md');
});
test('searchWorkspace searches unsaved buffers without root and prefers live open buffers over stale disk entries', () => {
  const draft = { ...untitled('Draft-1'), text: 'line one\nTODO: unsaved note' };
  const openSaved = opened({ path: '\\\\?\\C:\\notes\\open.md', text: 'disk text', metadata: untitled().metadata, revision: '1' });
  openSaved.text = 'updated TODO in open buffer';
  const diskMatches = [
    { path: 'C:\\notes\\open.md', relative: 'open.md', directory: false, line: 1, preview: 'TODO: stale disk line' },
    { path: 'C:\\notes\\other.md', relative: 'other.md', directory: false, line: 4, preview: 'TODO: on disk only' },
  ];
  expect(searchWorkspace('', [draft], diskMatches, null)).toEqual([]);
  const noRoot = searchWorkspace('todo', [draft], [], null);
  expect(noRoot).toHaveLength(1);
  expect(noRoot[0]).toMatchObject({ bufferId: draft.id, label: 'Draft-1', line: 2, unsaved: true });
  const withRoot = searchWorkspace('todo', [draft, openSaved], diskMatches, 'C:\\notes');
  expect(withRoot.map(m => ({ label: m.label, line: m.line, preview: m.preview, unsaved: m.unsaved }))).toEqual([
    { label: 'Draft-1', line: 2, preview: 'TODO: unsaved note', unsaved: true },
    { label: 'open.md', line: 1, preview: 'updated TODO in open buffer', unsaved: false },
    { label: 'other.md', line: 4, preview: 'TODO: on disk only', unsaved: false },
  ]);
});
test('bufferLanguage, preferredFileName, and validateFileName handle unsaved and typed extensions', () => {
  expect(bufferLanguage({ path: null, name: 'Untitled-1' })).toBe('markdown');
  expect(bufferLanguage({ path: null, name: 'scratch.ts' })).toBe('typescript');
  expect(bufferLanguage({ path: '/root/a.json', name: undefined })).toBe('json');
  expect(preferredFileName({ path: null, name: 'Untitled-1' })).toBe('Untitled-1.md');
  expect(preferredFileName({ path: null, name: 'note.txt' })).toBe('note.txt');
  expect(validateFileName('notes', 'md')).toBe('notes.md');
  expect(validateFileName('app', 'ts')).toBe('app.ts');
  expect(validateFileName('custom.json', 'md')).toBe('custom.json');
  expect(() => validateFileName('.simple-note.json')).toThrow();
  expect(() => validateFileName('foo.simple-note.json')).toThrow();
  expect(() => validateFileName('../escape.md')).toThrow();
  expect(() => validateFileName('CON.md')).toThrow();
});

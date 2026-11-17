import { test, expect } from 'bun:test';
import { untitled, opened, modified, closeDecision, fingerprint, reconcile, removeBuffer, languageFor, recovered, type Workspace } from './files';

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
});

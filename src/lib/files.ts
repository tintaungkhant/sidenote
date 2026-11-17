import { invoke } from '@tauri-apps/api/core';
import type { Annotation } from './editor/document';
export type Language = 'plaintext' | 'markdown' | 'javascript' | 'typescript' | 'php' | 'html' | 'css' | 'json';
export type Metadata = { version: 1; main?: string; images: Record<string, { id: string; annotations: readonly Annotation[] }> };
export type DiskDocument = { path: string; text: string; metadata: Metadata; revision: string };
export type Buffer = { id: string; path: string | null; text: string; metadata: Metadata; baseline: string; revision: string; external: 'changed' | 'deleted' | 'unavailable' | null; anchor: number; head: number; scrollTop: number; pinned: boolean };
export type Workspace = { version: 1; buffers: Buffer[]; active: string | null; root: string | null; wrap: boolean; sidebar: boolean };
export type Entry = { path: string; relative: string; directory: boolean; line?: number; preview?: string };
export function languageFor(path: string | null): Language {
  return ({ md: 'markdown', markdown: 'markdown', js: 'javascript', jsx: 'javascript', mjs: 'javascript', cjs: 'javascript', ts: 'typescript', tsx: 'typescript', php: 'php', html: 'html', htm: 'html', css: 'css', json: 'json' } as Record<string, Language>)[path?.split('.').at(-1)?.toLowerCase() ?? ''] ?? 'plaintext';
}
/** IPC object key order is not document content. Rust may return maps in sorted order. */
export const fingerprint = (text: string, metadata: Metadata) => JSON.stringify([text, metadata], (_key,value) => value && typeof value === 'object' && !Array.isArray(value) ? Object.fromEntries(Object.keys(value).sort().map(key => [key,value[key]])) : value);
export const modified = (buffer: Buffer) => buffer.baseline !== fingerprint(buffer.text, buffer.metadata);
export const title = (buffer: Buffer) => buffer.path?.split(/[/\\]/).at(-1) ?? 'Untitled';
export function untitled(): Buffer { const metadata: Metadata = { version: 1, images: {} }; return { id: crypto.randomUUID(), path: null, text: '', metadata, baseline: fingerprint('', metadata), revision: '', external: null, anchor: 0, head: 0, scrollTop: 0, pinned: false }; }
export function opened(disk: DiskDocument): Buffer { return { ...untitled(), ...disk, baseline: fingerprint(disk.text, disk.metadata) }; }
export function reconcile(buffer: Buffer, disk: DiskDocument | null): Buffer {
  if (!disk) return { ...buffer, external: 'deleted' };
  if (disk.revision === buffer.revision) return { ...buffer, external: null };
  if (modified(buffer)) return { ...buffer, external: 'changed' };
  return { ...buffer, ...disk, baseline: fingerprint(disk.text, disk.metadata), external: null };
}
export function closeDecision(buffer: Buffer): 'prompt' | 'close' { return modified(buffer) ? 'prompt' : 'close'; }
export function removeBuffer(workspace: Workspace, id: string): Workspace {
  const index = workspace.buffers.findIndex(b => b.id === id);
  const buffers = workspace.buffers.filter(b => b.id !== id);
  return { ...workspace, buffers, active: workspace.active === id ? buffers[Math.min(index, buffers.length - 1)]?.id ?? null : workspace.active };
}
export const operation = <T>(operation: object): Promise<T> => invoke('file_operation', { operation });
export const picker = (kind: 'open' | 'save' | 'folder', name?: string): Promise<string | null> => invoke('file_picker', { kind, name });
export const readFile = (path: string) => operation<DiskDocument>({ kind: 'open', path });
export const snapshot = (workspace: Workspace) => operation<void>({ kind: 'snapshot', snapshot: JSON.stringify(workspace) });
/** Reject corrupt recovery rather than replacing it with an empty workspace. */
export function recovered(value: unknown): Workspace {
  const w = value as Workspace;
  if (!w || w.version !== 1 || !Array.isArray(w.buffers) || w.buffers.length > 1000 || typeof w.wrap !== 'boolean' || typeof w.sidebar !== 'boolean' || !(w.root === null || typeof w.root === 'string')) throw new Error('Unsupported recovery session');
  const ids = new Set<string>();
  for (const b of w.buffers) {
    if (!b || typeof b.id !== 'string' || ids.has(b.id) || !(b.path === null || typeof b.path === 'string') || typeof b.text !== 'string' || typeof b.baseline !== 'string' || typeof b.revision !== 'string' || typeof b.pinned !== 'boolean' || ![b.anchor,b.head,b.scrollTop].every(n => Number.isFinite(n) && n >= 0) || !b.metadata || b.metadata.version !== 1 || !b.metadata.images || typeof b.metadata.images !== 'object' || Array.isArray(b.metadata.images)) throw new Error('Invalid recovery buffer; original recovery was preserved');
    if (!Number.isSafeInteger(b.anchor) || !Number.isSafeInteger(b.head)) throw new Error('Invalid recovery selection');
    ids.add(b.id);
    const baseline = JSON.parse(b.baseline);
    if (!Array.isArray(baseline) || typeof baseline[0] !== 'string' || baseline[1]?.version !== 1) throw new Error('Invalid recovery baseline');
    b.baseline = fingerprint(baseline[0],baseline[1]);
  }
  if (w.active !== null && !ids.has(w.active)) throw new Error('Invalid active recovery tab');
  return w;
}

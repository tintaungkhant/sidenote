import { invoke } from '@tauri-apps/api/core';
import type { Annotation } from './editor/document';
export type Language = 'plaintext' | 'markdown' | 'javascript' | 'typescript' | 'php' | 'html' | 'css' | 'json';
export type Metadata = { version: 1; main?: string; images: Record<string, { id: string; annotations: readonly Annotation[] }> };
export type DiskDocument = { path: string; text: string; metadata: Metadata; revision: string };
export type Buffer = { id: string; path: string | null; name?: string; text: string; metadata: Metadata; baseline: string; revision: string; external: 'changed' | 'deleted' | 'unavailable' | null; anchor: number; head: number; scrollTop: number; pinned: boolean };
export type Workspace = { version: 1; buffers: Buffer[]; active: string | null; root: string | null; wrap: boolean; sidebar: boolean };
export type Entry = { path: string; relative: string; directory: boolean; package?: boolean; line?: number; preview?: string };
export type SearchMatch = { key: string; bufferId?: string; path?: string; label: string; line: number; preview: string; unsaved: boolean };
export function languageFor(path: string | null): Language {
  return ({ md: 'markdown', markdown: 'markdown', js: 'javascript', jsx: 'javascript', mjs: 'javascript', cjs: 'javascript', ts: 'typescript', tsx: 'typescript', php: 'php', html: 'html', htm: 'html', css: 'css', json: 'json' } as Record<string, Language>)[path?.split('.').at(-1)?.toLowerCase() ?? ''] ?? 'plaintext';
}
export function bufferLanguage(buffer: Pick<Buffer, 'path' | 'name'>): Language {
  if (buffer.path) return languageFor(buffer.path);
  if (buffer.name?.includes('.')) return languageFor(buffer.name);
  return 'markdown';
}
/** IPC object key order is not document content. Rust may return maps in sorted order. */
export const fingerprint = (text: string, metadata: Metadata) => JSON.stringify([text, metadata], (_key,value) => value && typeof value === 'object' && !Array.isArray(value) ? Object.fromEntries(Object.keys(value).sort().map(key => [key,value[key]])) : value);
export const modified = (buffer: Buffer) => buffer.baseline !== fingerprint(buffer.text, buffer.metadata);
export const title = (buffer: Buffer) => buffer.path?.split(/[/\\]/).at(-1) ?? buffer.name ?? 'Untitled';
export function preferredFileName(buffer: Pick<Buffer, 'path' | 'name'>): string {
  const raw = (buffer.path?.split(/[/\\]/).at(-1) ?? buffer.name ?? 'Untitled-1').trim() || 'Untitled-1';
  return raw.includes('.') ? raw : `${raw}.md`;
}
export function nextUntitledName(buffers: readonly Pick<Buffer, 'path' | 'name'>[], entries: readonly Pick<Entry, 'relative' | 'directory' | 'package'>[] = []): string {
  const taken = new Set<number>();
  const record = (raw: string | undefined) => {
    if (!raw) return;
    const base = raw.split(/[/\\]/).at(-1) ?? raw;
    const match = /^untitled-(\d+)(?:\.[^.]+)?$/i.exec(base.trim());
    if (match) taken.add(Number(match[1]));
  };
  for (const b of buffers) {
    record(b.path ?? undefined);
    if (b.path) record(b.path.split(/[/\\]/).at(-2));
    record(b.name);
  }
  for (const e of entries) record(e.relative.split('/')[0]);
  let n = 1;
  while (taken.has(n)) n += 1;
  return `Untitled-${n}`;
}
export function displayPath(path: string): string {
  if (path.startsWith('\\\\?\\UNC\\')) return '\\\\' + path.slice(8);
  if (path.startsWith('\\\\?\\')) return path.slice(4);
  return path;
}
export function normalizePath(path: string): string {
  const clean = displayPath(path).replace(/\\/g, '/');
  return clean === '/' ? '/' : clean.replace(/\/+$/, '');
}
function isWindowsPath(path: string): boolean {
  return /^[a-z]:/i.test(path) || path.startsWith('//');
}
export function samePath(a: string | null | undefined, b: string | null | undefined): boolean {
  if (!a || !b) return false;
  const na = normalizePath(a), nb = normalizePath(b);
  return isWindowsPath(na) || isWindowsPath(nb) ? na.toLowerCase() === nb.toLowerCase() : na === nb;
}
export function withinRoot(filePath: string | null | undefined, root: string | null | undefined): boolean {
  if (!filePath || !root) return false;
  let nf = normalizePath(filePath), nr = normalizePath(root);
  if (isWindowsPath(nf) || isWindowsPath(nr)) { nf = nf.toLowerCase(); nr = nr.toLowerCase(); }
  if (nr === '/') return nf.startsWith('/');
  return nf === nr || nf.startsWith(`${nr}/`);
}
export function relativeToRoot(filePath: string, root: string | null | undefined): string {
  if (root && withinRoot(filePath, root)) {
    const nf = normalizePath(filePath), nr = normalizePath(root);
    if (nr === '/') return nf.slice(1);
    if (nf.length === nr.length) return filePath.split(/[/\\]/).at(-1) ?? nf;
    return nf.slice(nr.length + 1);
  }
  return filePath.split(/[/\\]/).at(-1) ?? filePath;
}
export function isUnsavedBuffer(buffer: Pick<Buffer, 'path' | 'external'>, root: string | null): boolean {
  return !buffer.path || buffer.external === 'deleted' || !withinRoot(buffer.path, root);
}
export function validateFileName(raw: string, fallbackExt = 'md'): string {
  const trimmed = raw.trim();
  if (!trimmed || trimmed === '.' || trimmed === '..' || trimmed.length > 200 || trimmed.endsWith('.') || trimmed.endsWith(' ')) {
    throw new Error('Invalid file name.');
  }
  if (/[\x00-\x1f/\\:<>\"|?*]/.test(trimmed)) {
    throw new Error('File name contains unsupported characters.');
  }
  const lower = trimmed.toLowerCase();
  if (lower === '.simple-note.json' || lower.endsWith('.simple-note.json')) {
    throw new Error('That file name is reserved for metadata.');
  }
  const dot = trimmed.lastIndexOf('.');
  const stem = dot >= 0 ? trimmed.slice(0, dot) : trimmed;
  const ext = dot >= 0 ? trimmed.slice(dot + 1) : fallbackExt;
  if (!stem || !ext || stem.endsWith(' ') || stem.endsWith('.')) {
    throw new Error('Invalid file name.');
  }
  if (/^(\.git|node_modules|target|assets)$/i.test(stem)) {
    throw new Error('That name is reserved.');
  }
  const base = stem.split('.')[0].trimEnd().toUpperCase();
  if (/^(CON|PRN|AUX|NUL|COM[1-9]|LPT[1-9])$/.test(base)) {
    throw new Error('That file name is reserved by the operating system.');
  }
  return `${stem}.${ext}`;
}
function stemOf(name: string): string {
  const dot = name.lastIndexOf('.');
  return dot > 0 ? name.slice(0, dot) : name;
}
export function compactNotePath(relative: string): string {
  const parts = relative.split('/').filter(Boolean);
  if (parts.length >= 2 && parts.at(-2)!.toLowerCase() === stemOf(parts.at(-1)!).toLowerCase()) {
    return [...parts.slice(0, -2), parts.at(-1)!].join('/');
  }
  return relative;
}
export function entryDepth(entry: Pick<Entry, 'relative' | 'package'>): number {
  return Math.max(0, entry.relative.split('/').length - (entry.package ? 2 : 1));
}
export function entryLabel(entry: Pick<Entry, 'relative' | 'package'>): string {
  if (entry.package) {
    const compacted = compactNotePath(entry.relative);
    const parts = compacted.split('/').filter(Boolean);
    if (parts.length >= 2 && entry.relative.split('/').length === parts.length) {
      return `${parts.at(-2)}/${parts.at(-1)}`;
    }
    return parts.at(-1) ?? entry.relative;
  }
  return entry.relative.split('/').at(-1) ?? entry.relative;
}
export function searchWorkspace(query: string, buffers: readonly Buffer[], diskMatches: readonly Entry[], root: string | null): SearchMatch[] {
  if (!query) return [];
  const needle = query.toLowerCase();
  const results: SearchMatch[] = [];
  const openPaths: string[] = [];
  const scanBuffer = (buffer: Buffer, unsaved: boolean, label: string) => {
    const lines = buffer.text.split(/\r?\n/);
    for (let i = 0; i < lines.length; i++) {
      if (lines[i].toLowerCase().includes(needle)) {
        results.push({
          key: `buf:${buffer.id}:${i + 1}`,
          bufferId: buffer.id,
          ...(buffer.path ? { path: buffer.path } : {}),
          label,
          line: i + 1,
          preview: lines[i].slice(0, 240),
          unsaved,
        });
        if (results.length >= 10000) return true;
      }
    }
    return false;
  };
  for (const buffer of buffers) {
    if (buffer.path && withinRoot(buffer.path, root) && buffer.external !== 'deleted') openPaths.push(buffer.path);
    if (isUnsavedBuffer(buffer, root) && scanBuffer(buffer, true, title(buffer))) return results;
  }
  for (const buffer of buffers) {
    if (!isUnsavedBuffer(buffer, root) && buffer.path && scanBuffer(buffer, false, compactNotePath(relativeToRoot(buffer.path, root)))) return results;
  }
  for (const entry of diskMatches) {
    if (entry.directory || entry.line === undefined) continue;
    if (openPaths.some(p => samePath(p, entry.path))) continue;
    results.push({
      key: `disk:${entry.path}:${entry.line}`,
      path: entry.path,
      label: compactNotePath(entry.relative),
      line: entry.line,
      preview: entry.preview ?? '',
      unsaved: false,
    });
    if (results.length >= 10000) return results;
  }
  return results;
}
export function untitled(name?: string): Buffer { const metadata: Metadata = { version: 1, images: {} }; return { id: crypto.randomUUID(), path: null, ...(name !== undefined ? { name } : {}), text: '', metadata, baseline: fingerprint('', metadata), revision: '', external: null, anchor: 0, head: 0, scrollTop: 0, pinned: false }; }
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
    if (!b || typeof b.id !== 'string' || ids.has(b.id) || !(b.path === null || typeof b.path === 'string') || !(b.name === undefined || (typeof b.name === 'string' && b.name.length <= 255)) || typeof b.text !== 'string' || typeof b.baseline !== 'string' || typeof b.revision !== 'string' || typeof b.pinned !== 'boolean' || ![b.anchor,b.head,b.scrollTop].every(n => Number.isFinite(n) && n >= 0) || !b.metadata || b.metadata.version !== 1 || !b.metadata.images || typeof b.metadata.images !== 'object' || Array.isArray(b.metadata.images)) throw new Error('Invalid recovery buffer; original recovery was preserved');
    if (!Number.isSafeInteger(b.anchor) || !Number.isSafeInteger(b.head)) throw new Error('Invalid recovery selection');
    ids.add(b.id);
    const baseline = JSON.parse(b.baseline);
    if (!Array.isArray(baseline) || typeof baseline[0] !== 'string' || baseline[1]?.version !== 1) throw new Error('Invalid recovery baseline');
    b.baseline = fingerprint(baseline[0],baseline[1]);
  }
  if (w.active !== null && !ids.has(w.active)) throw new Error('Invalid active recovery tab');
  return w;
}

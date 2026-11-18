import { invoke } from '@tauri-apps/api/core';
import type { Buffer, DiskDocument } from './files';
import {
  compactNotePath,
  isUnsavedBuffer,
  modified,
  normalizePath,
  relativeToRoot,
  samePath,
  title,
  withinRoot,
} from './files';

export type CommitEntry = {
  hash: string;
  shortHash: string;
  timestamp: number;
  author: string;
  subject: string;
};

export type VcsStatus = {
  available: boolean;
  supported: boolean;
  initialized: boolean;
  dirty: boolean;
  branch?: string;
  commits: CommitEntry[];
};

export type VcsNoteStatus = {
  path: string;
  relative: string;
  name: string;
  initialized: boolean;
  dirty: boolean;
  branch?: string;
};

export type VcsWorkspaceStatus = {
  available: boolean;
  notes: VcsNoteStatus[];
};

export type WorkspaceChange = {
  key: string;
  path?: string;
  bufferId?: string;
  title: string;
  relative: string;
  subpath?: string;
  branch?: string;
  status: 'modified' | 'untracked';
  dirty: boolean;
  initialized: boolean;
  isUnsaved: boolean;
};

export type VcsDiffPayload = {
  oldText: string;
  newText: string;
  isInitial: boolean;
  metaChanged: boolean;
  assetsChanged: boolean;
};

export type DiffLine = {
  kind: 'context' | 'add' | 'del';
  oldLine: number | null;
  newLine: number | null;
  text: string;
};

export type DiffSummary = {
  lines: DiffLine[];
  added: number;
  removed: number;
};

export const checkGit = (): Promise<{ available: boolean; version: string | null }> =>
  invoke('vcs_operation', { operation: { kind: 'check' } });

export const vcsStatus = (root: string, path: string): Promise<VcsStatus> =>
  invoke('vcs_operation', { operation: { kind: 'status', root, path } });

export const vcsWorkspaceStatus = (root: string): Promise<VcsWorkspaceStatus> =>
  invoke('vcs_operation', { operation: { kind: 'workspaceStatus', root } });

export const vcsCommit = (root: string, path: string, message: string): Promise<VcsStatus> =>
  invoke('vcs_operation', { operation: { kind: 'commit', root, path, message } });

export const vcsDiff = (
  root: string,
  path: string,
  hash: string | null = null,
  mode: 'parent' | 'working' = 'parent',
): Promise<VcsDiffPayload> =>
  invoke('vcs_operation', { operation: { kind: 'diff', root, path, hash, mode } });

export const vcsRestore = (root: string, path: string, hash: string): Promise<DiskDocument> =>
  invoke('vcs_operation', { operation: { kind: 'restore', root, path, hash } });

export const vcsDiscard = (root: string, path: string): Promise<VcsStatus> =>
  invoke('vcs_operation', { operation: { kind: 'discard', root, path } });

export function defaultCommitMessage(initialized: boolean): string {
  return initialized ? 'Update note' : 'Initial commit';
}

export function resolveCommitMessage(raw: string, initialized: boolean): string {
  return raw.trim() || defaultCommitMessage(initialized);
}

export function hasChangesToCommit(params: {
  gitAvailable?: boolean | null;
  status: VcsStatus | null;
  isModified: boolean;
  isWithinRoot: boolean;
  hasText: boolean;
  isDeleted?: boolean;
}): boolean {
  if (params.gitAvailable === false || params.isDeleted) return false;
  if (params.isModified) return true;
  if (params.isWithinRoot) {
    if (!params.status) return false;
    return !params.status.initialized || params.status.dirty;
  }
  return params.hasText;
}

export function buildWorkspaceChanges(params: {
  gitAvailable?: boolean | null;
  root: string | null;
  buffers: readonly Pick<Buffer, 'id' | 'path' | 'name' | 'text' | 'metadata' | 'baseline' | 'external'>[];
  workspaceNotes: readonly VcsNoteStatus[];
}): WorkspaceChange[] {
  if (!params.root || params.gitAvailable === false) return [];
  const results: WorkspaceChange[] = [];
  const seenPaths = new Set<string>();

  // 1. Open unsaved buffers (no disk path yet or outside root)
  for (const buffer of params.buffers) {
    if (isUnsavedBuffer(buffer, params.root)) {
      if (buffer.text.length > 0 || modified(buffer as Buffer)) {
        results.push({
          key: `buf:${buffer.id}`,
          bufferId: buffer.id,
          title: title(buffer as Buffer),
          relative: title(buffer as Buffer),
          branch: 'main',
          status: 'untracked',
          dirty: true,
          initialized: false,
          isUnsaved: true,
        });
      }
    }
  }

  // 2. Saved notes discovered in workspace root
  for (const note of params.workspaceNotes) {
    seenPaths.add(normalizePath(note.path).toLowerCase());
    const openBuffer = params.buffers.find(b => b.path && samePath(b.path, note.path));
    const inRamModified = openBuffer ? modified(openBuffer as Buffer) : false;
    const isDirty = note.dirty || inRamModified;
    const isInitialized = note.initialized;
    if (isDirty || !isInitialized) {
      const compacted = compactNotePath(note.relative);
      const parts = compacted.split('/');
      const subpath = parts.length > 1 ? parts.slice(0, -1).join('/') : undefined;
      results.push({
        key: `disk:${note.path}`,
        path: note.path,
        bufferId: openBuffer?.id,
        title: note.name,
        relative: compacted,
        subpath,
        branch: note.branch || 'main',
        status: isInitialized ? 'modified' : 'untracked',
        dirty: isDirty,
        initialized: isInitialized,
        isUnsaved: false,
      });
    }
  }

  // 3. Any other open buffers inside root not present in workspaceNotes (e.g. freshly created)
  for (const buffer of params.buffers) {
    if (buffer.path && withinRoot(buffer.path, params.root) && buffer.external !== 'deleted') {
      const norm = normalizePath(buffer.path).toLowerCase();
      if (!seenPaths.has(norm)) {
        if (modified(buffer as Buffer)) {
          const rel = compactNotePath(relativeToRoot(buffer.path, params.root));
          const parts = rel.split('/');
          const subpath = parts.length > 1 ? parts.slice(0, -1).join('/') : undefined;
          results.push({
            key: `buf:${buffer.id}`,
            bufferId: buffer.id,
            path: buffer.path,
            title: title(buffer as Buffer),
            relative: rel,
            subpath,
            branch: 'main',
            status: 'modified',
            dirty: true,
            initialized: false,
            isUnsaved: false,
          });
        }
      }
    }
  }

  return results;
}

export function formatRelativeTime(unixSeconds: number, nowMs = Date.now()): string {
  const diffSec = Math.max(0, Math.floor(nowMs / 1000) - unixSeconds);
  if (diffSec < 60) return 'just now';
  if (diffSec < 3600) return `${Math.floor(diffSec / 60)}m ago`;
  if (diffSec < 86400) return `${Math.floor(diffSec / 3600)}h ago`;
  const days = Math.floor(diffSec / 86400);
  if (days < 30) return `${days}d ago`;
  if (days < 365) return `${Math.floor(days / 30)}mo ago`;
  return `${Math.floor(days / 365)}y ago`;
}

export function diffLines(oldText: string, newText: string): DiffSummary {
  const oldLines = oldText === '' ? [] : oldText.split(/\r?\n/);
  const newLines = newText === '' ? [] : newText.split(/\r?\n/);
  const lines: DiffLine[] = [];
  let added = 0;
  let removed = 0;

  let prefix = 0;
  while (prefix < oldLines.length && prefix < newLines.length && oldLines[prefix] === newLines[prefix]) {
    lines.push({ kind: 'context', oldLine: prefix + 1, newLine: prefix + 1, text: oldLines[prefix] });
    prefix += 1;
  }

  let suffix = 0;
  while (
    suffix < oldLines.length - prefix &&
    suffix < newLines.length - prefix &&
    oldLines[oldLines.length - 1 - suffix] === newLines[newLines.length - 1 - suffix]
  ) {
    suffix += 1;
  }

  const midOld = oldLines.slice(prefix, oldLines.length - suffix);
  const midNew = newLines.slice(prefix, newLines.length - suffix);
  const m = midOld.length;
  const n = midNew.length;

  let oldNum = prefix + 1;
  let newNum = prefix + 1;

  if (m === 0 && n === 0) {
    // Nothing in middle
  } else if (m * n <= 2_500_000) {
    const stride = n + 1;
    const dp = new Int32Array((m + 1) * stride);
    for (let i = m - 1; i >= 0; i -= 1) {
      const row = i * stride;
      const nextRow = (i + 1) * stride;
      for (let j = n - 1; j >= 0; j -= 1) {
        dp[row + j] =
          midOld[i] === midNew[j]
            ? 1 + dp[nextRow + j + 1]
            : Math.max(dp[nextRow + j], dp[row + j + 1]);
      }
    }
    let i = 0;
    let j = 0;
    while (i < m || j < n) {
      if (i < m && j < n && midOld[i] === midNew[j]) {
        lines.push({ kind: 'context', oldLine: oldNum, newLine: newNum, text: midOld[i] });
        oldNum += 1;
        newNum += 1;
        i += 1;
        j += 1;
      } else if (i < m && (j === n || dp[(i + 1) * stride + j] >= dp[i * stride + j + 1])) {
        lines.push({ kind: 'del', oldLine: oldNum, newLine: null, text: midOld[i] });
        removed += 1;
        oldNum += 1;
        i += 1;
      } else {
        lines.push({ kind: 'add', oldLine: null, newLine: newNum, text: midNew[j] });
        added += 1;
        newNum += 1;
        j += 1;
      }
    }
  } else {
    for (const line of midOld) {
      lines.push({ kind: 'del', oldLine: oldNum, newLine: null, text: line });
      removed += 1;
      oldNum += 1;
    }
    for (const line of midNew) {
      lines.push({ kind: 'add', oldLine: null, newLine: newNum, text: line });
      added += 1;
      newNum += 1;
    }
  }

  for (let s = 0; s < suffix; s += 1) {
    const oldIdx = oldLines.length - suffix + s;
    const newIdx = newLines.length - suffix + s;
    lines.push({ kind: 'context', oldLine: oldIdx + 1, newLine: newIdx + 1, text: oldLines[oldIdx] });
  }

  return { lines, added, removed };
}

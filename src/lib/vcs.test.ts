import { expect, test } from 'bun:test';
import {
  buildWorkspaceChanges,
  defaultCommitMessage,
  diffLines,
  formatRelativeTime,
  hasChangesToCommit,
  resolveCommitMessage,
  type VcsNoteStatus,
  type VcsStatus,
} from './vcs';
import { fingerprint, type Buffer } from './files';

test('defaultCommitMessage and resolveCommitMessage supply non-empty fallbacks', () => {
  expect(defaultCommitMessage(false)).toBe('Initial commit');
  expect(defaultCommitMessage(true)).toBe('Update note');
  expect(resolveCommitMessage('   ', false)).toBe('Initial commit');
  expect(resolveCommitMessage('', true)).toBe('Update note');
  expect(resolveCommitMessage('  Fix typo  ', true)).toBe('Fix typo');
});

test('formatRelativeTime formats seconds through years', () => {
  const nowMs = 1_700_000_000_000;
  const nowSec = Math.floor(nowMs / 1000);
  expect(formatRelativeTime(nowSec - 10, nowMs)).toBe('just now');
  expect(formatRelativeTime(nowSec - 180, nowMs)).toBe('3m ago');
  expect(formatRelativeTime(nowSec - 7200, nowMs)).toBe('2h ago');
  expect(formatRelativeTime(nowSec - 86400 * 5, nowMs)).toBe('5d ago');
  expect(formatRelativeTime(nowSec - 86400 * 90, nowMs)).toBe('3mo ago');
  expect(formatRelativeTime(nowSec - 86400 * 400, nowMs)).toBe('1y ago');
});

test('diffLines computes line additions, deletions, replacements, CRLF, and empty initial state', () => {
  expect(diffLines('a\nb', 'a\nb')).toEqual({
    lines: [
      { kind: 'context', oldLine: 1, newLine: 1, text: 'a' },
      { kind: 'context', oldLine: 2, newLine: 2, text: 'b' },
    ],
    added: 0,
    removed: 0,
  });

  expect(diffLines('', 'hello\nworld')).toEqual({
    lines: [
      { kind: 'add', oldLine: null, newLine: 1, text: 'hello' },
      { kind: 'add', oldLine: null, newLine: 2, text: 'world' },
    ],
    added: 2,
    removed: 0,
  });

  expect(diffLines('one\r\ntwo\r\nthree', 'one\ntwo modified\nthree\nfour')).toEqual({
    lines: [
      { kind: 'context', oldLine: 1, newLine: 1, text: 'one' },
      { kind: 'del', oldLine: 2, newLine: null, text: 'two' },
      { kind: 'add', oldLine: null, newLine: 2, text: 'two modified' },
      { kind: 'context', oldLine: 3, newLine: 3, text: 'three' },
      { kind: 'add', oldLine: null, newLine: 4, text: 'four' },
    ],
    added: 2,
    removed: 1,
  });

  expect(diffLines('keep\ndrop', 'keep')).toEqual({
    lines: [
      { kind: 'context', oldLine: 1, newLine: 1, text: 'keep' },
      { kind: 'del', oldLine: 2, newLine: null, text: 'drop' },
    ],
    added: 0,
    removed: 1,
  });
});

test('hasChangesToCommit disables commit when note has no changes', () => {
  const cleanStatus: VcsStatus = {
    available: true,
    supported: true,
    initialized: true,
    dirty: false,
    commits: [
      {
        hash: 'abc',
        shortHash: 'abc',
        timestamp: 100,
        author: 'User',
        subject: 'Initial commit',
      },
    ],
  };

  const dirtyStatus: VcsStatus = {
    ...cleanStatus,
    dirty: true,
  };

  const uninitializedStatus: VcsStatus = {
    available: true,
    supported: true,
    initialized: false,
    dirty: true,
    commits: [],
  };

  // Clean note in root, editor not modified -> No changes (disabled)
  expect(
    hasChangesToCommit({
      gitAvailable: true,
      status: cleanStatus,
      isModified: false,
      isWithinRoot: true,
      hasText: true,
    })
  ).toBe(false);

  // Clean note in root, but editor has unsaved in-memory edits -> Has changes (enabled)
  expect(
    hasChangesToCommit({
      gitAvailable: true,
      status: cleanStatus,
      isModified: true,
      isWithinRoot: true,
      hasText: true,
    })
  ).toBe(true);

  // Working tree dirty on disk -> Has changes (enabled)
  expect(
    hasChangesToCommit({
      gitAvailable: true,
      status: dirtyStatus,
      isModified: false,
      isWithinRoot: true,
      hasText: true,
    })
  ).toBe(true);

  // Untracked / uninitialized note in root -> Has changes (enabled)
  expect(
    hasChangesToCommit({
      gitAvailable: true,
      status: uninitializedStatus,
      isModified: false,
      isWithinRoot: true,
      hasText: true,
    })
  ).toBe(true);

  // Git is not available / installed -> Disabled
  expect(
    hasChangesToCommit({
      gitAvailable: false,
      status: dirtyStatus,
      isModified: true,
      isWithinRoot: true,
      hasText: true,
    })
  ).toBe(false);

  // File is marked deleted -> Disabled
  expect(
    hasChangesToCommit({
      gitAvailable: true,
      status: dirtyStatus,
      isModified: true,
      isWithinRoot: true,
      hasText: true,
      isDeleted: true,
    })
  ).toBe(false);

  // Unsaved buffer with text -> Has changes (enabled)
  expect(
    hasChangesToCommit({
      gitAvailable: true,
      status: null,
      isModified: true,
      isWithinRoot: false,
      hasText: true,
    })
  ).toBe(true);

  // Empty unsaved buffer with no modifications -> No changes (disabled)
  expect(
    hasChangesToCommit({
      gitAvailable: true,
      status: null,
      isModified: false,
      isWithinRoot: false,
      hasText: false,
    })
  ).toBe(false);
});

test('buildWorkspaceChanges collects all changed and untracked notes across the workspace', () => {
  const root = 'C:/test/workspace';
  const meta = { version: 1 as const, images: {} };

  const cleanBuf: Buffer = {
    id: 'b1',
    path: 'C:/test/workspace/note1/note1.md',
    text: 'hello',
    metadata: meta,
    baseline: fingerprint('hello', meta),
    revision: '1',
    external: null,
    anchor: 0,
    head: 0,
    scrollTop: 0,
    pinned: false,
  };

  const modifiedBuf: Buffer = {
    id: 'b2',
    path: 'C:/test/workspace/note2/note2.md',
    text: 'edited in ram',
    metadata: meta,
    baseline: fingerprint('original', meta),
    revision: '2',
    external: null,
    anchor: 0,
    head: 0,
    scrollTop: 0,
    pinned: false,
  };

  const unsavedWithText: Buffer = {
    id: 'b3',
    path: null,
    name: 'Untitled-1',
    text: 'draft content',
    metadata: meta,
    baseline: fingerprint('', meta),
    revision: '3',
    external: null,
    anchor: 0,
    head: 0,
    scrollTop: 0,
    pinned: false,
  };

  const emptyUnsaved: Buffer = {
    id: 'b4',
    path: null,
    name: 'Untitled-2',
    text: '',
    metadata: meta,
    baseline: fingerprint('', meta),
    revision: '4',
    external: null,
    anchor: 0,
    head: 0,
    scrollTop: 0,
    pinned: false,
  };

  const workspaceNotes: VcsNoteStatus[] = [
    {
      path: 'C:/test/workspace/note1/note1.md',
      relative: 'note1/note1.md',
      name: 'note1.md',
      initialized: true,
      dirty: false,
    },
    {
      path: 'C:/test/workspace/note2/note2.md',
      relative: 'note2/note2.md',
      name: 'note2.md',
      initialized: true,
      dirty: false, // clean on disk, but buffer b2 is modified in memory!
    },
    {
      path: 'C:/test/workspace/note3/note3.md',
      relative: 'projects/note3/note3.md',
      name: 'note3.md',
      initialized: true,
      dirty: true, // dirty on disk, not open in any buffer
    },
    {
      path: 'C:/test/workspace/note4/note4.md',
      relative: 'note4/note4.md',
      name: 'note4.md',
      initialized: false, // untracked note
      dirty: true,
    },
  ];

  // 1. Missing git -> empty changes
  expect(
    buildWorkspaceChanges({
      gitAvailable: false,
      root,
      buffers: [cleanBuf, modifiedBuf, unsavedWithText],
      workspaceNotes,
    })
  ).toEqual([]);

  // 2. No root folder -> empty changes
  expect(
    buildWorkspaceChanges({
      gitAvailable: true,
      root: null,
      buffers: [cleanBuf, modifiedBuf, unsavedWithText],
      workspaceNotes,
    })
  ).toEqual([]);

  // 3. Normal workspace: collects unsaved buffer (b3), in-RAM modified buffer (b2), disk dirty (note3), untracked (note4)
  const changes = buildWorkspaceChanges({
    gitAvailable: true,
    root,
    buffers: [cleanBuf, modifiedBuf, unsavedWithText, emptyUnsaved],
    workspaceNotes,
  });

  expect(changes.length).toBe(4);

  // Unsaved buffer
  const cUnsaved = changes.find(c => c.bufferId === 'b3');
  expect(cUnsaved).toBeDefined();
  expect(cUnsaved?.status).toBe('untracked');
  expect(cUnsaved?.isUnsaved).toBe(true);

  // Note 2: clean on disk but in-RAM modified
  const cNote2 = changes.find(c => c.title === 'note2.md');
  expect(cNote2).toBeDefined();
  expect(cNote2?.status).toBe('modified');
  expect(cNote2?.bufferId).toBe('b2');

  // Note 3: dirty on disk without open buffer
  const cNote3 = changes.find(c => c.title === 'note3.md');
  expect(cNote3).toBeDefined();
  expect(cNote3?.status).toBe('modified');
  expect(cNote3?.subpath).toBe('projects');

  // Note 4: untracked
  const cNote4 = changes.find(c => c.title === 'note4.md');
  expect(cNote4).toBeDefined();
  expect(cNote4?.status).toBe('untracked');

  // Note 1 (clean) and empty unsaved (b4) must NOT be in changes
  expect(changes.find(c => c.title === 'note1.md')).toBeUndefined();
  expect(changes.find(c => c.bufferId === 'b4')).toBeUndefined();
});

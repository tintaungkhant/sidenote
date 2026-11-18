import { expect, test } from 'bun:test';
import { defaultCommitMessage, diffLines, formatRelativeTime, resolveCommitMessage } from './vcs';

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

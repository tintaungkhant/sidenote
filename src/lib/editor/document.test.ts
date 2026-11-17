import { describe, expect, test } from 'bun:test';
import { redo, undo } from '@codemirror/commands';
import { copyText, decodeDocument, encodeDocument, type ImageBlock, type NoteDocument } from './document';
import { createNoteState, imageField, insertImage, textSearch } from './state';

const image: ImageBlock = { kind: 'image', id: 'image-1' as ImageBlock['id'], attachmentId: 'asset-1' as ImageBlock['attachmentId'], annotations: [] };

describe('document boundary', () => {
  test('arbitrary text and literal placeholder characters round trip', () => {
    const text = '\uFFFC\u0000\n😀 [image:asset-1] <script>\r\n';
    const document: NoteDocument = { version: 1, blocks: [{ kind: 'text', text }] };
    expect(decodeDocument(encodeDocument(document))).toEqual(document);
  });

  test('images at every boundary preserve identity and text insertion offsets', () => {
    const document: NoteDocument = { version: 1, blocks: [image, image, { kind: 'text', text: 'x\uFFFC' }, image] };
    const encoded = encodeDocument(document);
    expect(encoded.images.map(item => item.from)).toEqual([0, 1, 4]);
    expect(decodeDocument(encoded)).toEqual(document);
    expect(copyText(encoded)).toBe('x\uFFFC');
  });

  test('mixed selection copy omits images but preserves ordinary text', () => {
    const encoded = encodeDocument({ version: 1, blocks: [{ kind: 'text', text: 'abc' }, image, { kind: 'text', text: 'def' }] });
    expect(copyText(encoded, 1, 6)).toBe('bcde');
    expect(copyText(encoded, 3, 4)).toBe('');
  });

  test('invalid sidecar placement fails instead of dropping text', () => {
    expect(() => decodeDocument({ text: 'x', images: [{ from: 0, block: image }] })).toThrow('Invalid image placement');
  });
});

describe('image history', () => {
  test('search finds literal markers but never internal image placeholders', () => {
    const state = createNoteState({ version: 1, blocks: [image, { kind: 'text', text: '\uFFFC' }] });
    const cursor = textSearch('\uFFFC').getCursor(state);
    expect(cursor.next().value).toMatchObject({ from: 1, to: 2 });
    expect(cursor.next().done).toBe(true);
    expect(textSearch('\uFFFC\uFFFC').getCursor(state).next().done).toBe(true);
  });

  test('adjacent image insertion, grouped undo, and redo keep independent IDs', () => {
    let state = createNoteState({ version: 1, blocks: [] });
    state = state.update(insertImage(state, image)).state;
    const second = { ...image, id: 'image-2' as ImageBlock['id'] };
    state = state.update(insertImage(state, second)).state;
    const expected = state.field(imageField);
    const target = { get state() { return state; }, dispatch(transaction: Parameters<Parameters<typeof undo>[0]['dispatch']>[0]) { state = transaction.state; } };
    while (undo(target)) { /* Exhaust history to verify merged effect ordering as well. */ }
    expect(state.doc.toString()).toBe('');
    expect(state.field(imageField)).toEqual([]);
    while (redo(target)) { /* Replay exactly the entries produced by insertion. */ }
    expect(state.field(imageField)).toEqual(expected);
  });

  test('editor preserves CRLF, lone CR, and literal markers exactly', () => {
    const text = 'a\r\nb\rc\n\uFFFC😀';
    const state = createNoteState({ version: 1, blocks: [{ kind: 'text', text }] });
    expect(state.doc.toString()).toBe(text);
  });

  test('insertion at image boundary maps identity without swallowing text', () => {
    let state = createNoteState({ version: 1, blocks: [image, image] });
    state = state.update({ changes: { from: 0, insert: 'before' } }).state;
    expect(state.field(imageField).map(item => item.from)).toEqual([6, 7]);
    expect(decodeDocument({ text: state.doc.toString(), images: state.field(imageField) }).blocks[0]).toEqual({ kind: 'text', text: 'before' });
  });

  test('deletion, undo, and redo restore exact image metadata', () => {
    let state = createNoteState({ version: 1, blocks: [{ kind: 'text', text: 'a' }, image, { kind: 'text', text: 'b' }] });
    state = state.update({ changes: { from: 0, to: 2 } }).state;
    expect(state.field(imageField)).toEqual([]);
    const target = { get state() { return state; }, dispatch(transaction: Parameters<Parameters<typeof undo>[0]['dispatch']>[0]) { state = transaction.state; } };
    expect(undo(target)).toBe(true);
    expect(state.doc.toString()).toBe('a\uFFFCb');
    expect(state.field(imageField)).toEqual([{ from: 1, block: image }]);
    expect(redo(target)).toBe(true);
    expect(state.doc.toString()).toBe('b');
    expect(state.field(imageField)).toEqual([]);
  });
});

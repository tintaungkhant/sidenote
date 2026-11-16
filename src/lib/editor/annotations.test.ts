import { expect, test } from 'bun:test';
import { undo, redo } from '@codemirror/commands';
import { AnnotationHistory, annotationKey, annotationSvg, imagePoint, translate } from './annotations';
import { annotateImage, createNoteState, imageField } from './state';
import { decodeDocument, encodeDocument, type Annotation, type ImageBlock } from './document';

const text: Annotation = { kind: 'text', x: 100, y: 50, text: '<script> & "caption"\nline two', color: '#ff0000', fontSize: 24 };

test('CSS resize and high-DPI displays resolve to the same original pixel', () => {
  for (const devicePixelRatio of [1, 1.5, 2, 3]) {
    const cssWidth = 800 / devicePixelRatio;
    expect(imagePoint(10 + cssWidth / 4, 20 + cssWidth / 8, { left: 10, top: 20, width: cssWidth, height: cssWidth / 2 }, 1600, 800)).toEqual({ x: 400, y: 200 });
  }
  expect(imagePoint(-1, 900, { left: 0, top: 0, width: 100, height: 100 }, 200, 200)).toEqual({ x: 0, y: 200 });
});

test('whole-object movement clamps to bounds without distortion or mutation', () => {
  const pen: Annotation = { kind: 'pen', points: [{ x: 10, y: 20 }, { x: 90, y: 80 }], color: '#ffffff', width: 4 };
  expect(translate(pen, 500, -500, 100, 100)).toMatchObject({ points: [{ x: 20, y: 0 }, { x: 100, y: 60 }] });
  expect(pen.points[0]).toEqual({ x: 10, y: 20 });
  expect(translate(text, -500, 500, 200, 100)).toMatchObject({ x: 0, y: 100 });
});

test('local history retains completed operations, clears branched redo, and never mutates snapshots', () => {
  const history = new AnnotationHistory([]);
  history.commit([text]);
  history.commit([translate(text, 10, 10, 1000, 1000)]);
  expect(history.step()).toEqual([text]);
  expect(history.step()).toEqual([]);
  expect(history.step(true)).toEqual([text]);
  history.commit([]);
  expect(history.step(true)).toEqual([]);
  expect(text.x).toBe(100);
});

test('annotation SVG escapes text and supports all persisted tools', () => {
  const markup = annotationSvg([text, ...(['pen', 'highlighter', 'arrow'] as const).map(kind => ({ kind, points: [{ x: 1, y: 2 }, { x: 30, y: 40 }], color: '#ff0000', width: 4 }))], 0);
  expect(markup).not.toContain('<script>');
  expect(markup).toContain('&lt;script&gt; &amp; &quot;caption&quot;');
  expect(markup).toContain('opacity="0.35"');
  expect(markup).toContain('<tspan x="100" dy="1.2em">line two</tspan>');
  expect(markup.match(/<polyline/g)?.length).toBe(4);
});

test('canvas keyboard routes history and tools but leaves text and composition untouched', () => {
  const event = { key: 'z', ctrlKey: true, metaKey: false, shiftKey: false, isComposing: false };
  expect(annotationKey(event, false)).toBe('undo');
  expect(annotationKey({ ...event, shiftKey: true }, false)).toBe('redo');
  expect(annotationKey(event, true)).toBeUndefined();
  expect(annotationKey({ ...event, key: 'Escape', isComposing: true }, false)).toBeUndefined();
  expect(annotationKey({ ...event, key: 'Escape' }, true)).toBe('exit');
  expect(annotationKey({ ...event, key: 'w' }, false)).toBeUndefined();
  for (const [key, action] of [['p', 'pen'], ['h', 'highlighter'], ['a', 'arrow'], ['t', 'text'], ['v', 'select'], ['Tab', 'cycle'], ['ArrowLeft', 'left'], ['Delete', 'delete']]) {
    expect(annotationKey({ ...event, key, ctrlKey: false }, false)).toBe(action);
  }
});

test('one original has independent placement annotations, serialized geometry, and note undo/redo', () => {
  const first: ImageBlock = { kind: 'image', id: 'one' as ImageBlock['id'], attachmentId: 'original' as ImageBlock['attachmentId'], annotations: [] };
  const second = { ...first, id: 'two' as ImageBlock['id'] };
  let state = createNoteState({ version: 1, blocks: [first, second] });
  state = state.update(annotateImage(state, first.id, [text])).state;
  expect(state.field(imageField)[1].block).toEqual(second);
  const saved = decodeDocument({ text: state.doc.toString(), images: state.field(imageField) });
  expect(encodeDocument(JSON.parse(JSON.stringify(saved)))).toEqual({ text: state.doc.toString(), images: state.field(imageField) });
  const target = { get state() { return state; }, dispatch(transaction: Parameters<Parameters<typeof undo>[0]['dispatch']>[0]) { state = transaction.state; } };
  expect(undo(target)).toBe(true);
  expect(state.field(imageField)[0].block.annotations).toEqual([]);
  expect(redo(target)).toBe(true);
  expect(state.field(imageField)[0].block.annotations).toEqual([text]);
});

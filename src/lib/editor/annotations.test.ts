import { expect, test } from 'bun:test';
import { undo, redo } from '@codemirror/commands';
import { AnnotationHistory, annotationKey, annotationSvg, imagePoint, translate, annotationBounds, resizeAnnotation } from './annotations';
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


test('selection uses a box and resize handle without glow or exported selection chrome', () => {
  const focused = annotationSvg([text], 0);
  expect(focused).not.toContain('drop-shadow');
  expect(focused).toContain('data-resize="true"');
  expect(focused).toContain('vector-effect="non-scaling-stroke"');
  const exported = annotationSvg([text]);
  expect(exported).not.toContain('data-resize');
  expect(exported).not.toContain('#75beff');
  expect(annotationSvg([text], 0, 0)).not.toContain('<text');
  expect(annotationSvg([text], 0, 0)).not.toContain('data-resize');
});

test('resize preserves geometry proportions, native bounds, and immutable source', () => {
  const pen: Annotation = { kind: 'pen', points: [{ x: 10, y: 20 }, { x: 50, y: 60 }], color: '#ff0000', width: 4 };
  expect(resizeAnnotation(pen, { x: 90, y: 100 }, 200, 200)).toMatchObject({ width: 8, points: [{ x: 10, y: 20 }, { x: 90, y: 100 }] });
  const clamped = resizeAnnotation(pen, { x: 1000, y: 1000 }, 100, 100);
  expect(annotationBounds(clamped)).toEqual({ left: 10, top: 20, right: 90, bottom: 100 });
  expect(pen.width).toBe(4);
  const dot = { ...pen, points: [{ x: 100, y: 100 }] };
  const small = resizeAnnotation(dot, { x: 0, y: 0 }, 100, 100);
  expect(small.kind !== 'text' && small.kind !== 'caption' && small.width > 0).toBe(true);
});

test('text bounds include empty and multiline captions; resizing changes font without changing content', () => {
  const empty = { ...text, text: '' };
  expect(annotationBounds(empty).right).toBeGreaterThan(empty.x);
  expect(annotationBounds(text).bottom).toBe(text.y + text.fontSize * 2 * 1.2);
  const resized = resizeAnnotation(text, { x: 1000, y: 1000 }, 2000, 2000);
  expect(resized.kind === 'text' && resized.text).toBe(text.text);
  expect(resized.kind === 'text' && resized.fontSize).toBeGreaterThan(text.fontSize);
  expect(text.fontSize).toBe(24);
});

test('rectangle and ellipse drags render, move, and resize in all directions', () => {
  for (const dx of [-60, 60]) for (const dy of [-40, 40]) {
    for (const kind of ['rectangle', 'circle'] as const) {
      const points = [{ x: 100, y: 100 }, { x: 100 + dx, y: 100 + dy }];
      const shape: Annotation = { kind, points, color: '#ff0000', width: 4 };
      const bounds = annotationBounds(shape);
      expect(bounds.right - bounds.left).toBe(60);
      expect(bounds.bottom - bounds.top).toBe(40);
      const svg = annotationSvg([shape]);
      expect(svg).toContain(kind === 'circle' ? '<ellipse ' : '<rect ');
      expect(svg).toContain('fill="none"');
      expect(svg).not.toContain('data-resize');
      const moved = translate(shape, 10, 10, 500, 500);
      expect(annotationBounds(moved).left).toBe(bounds.left + 10);
      const resized = resizeAnnotation(shape, { x: bounds.left + 120, y: bounds.top + 120 }, 500, 500);
      const b = annotationBounds(resized);
      expect((b.right - b.left) / (b.bottom - b.top)).toBeCloseTo(1.5);
      if (kind === 'circle') {
        expect(svg).toContain('rx="30" ry="20"');
        const round = { ...shape, points: [{ x: 10, y: 10 }, { x: 50, y: 50 }] };
        expect(annotationSvg([round])).toContain('rx="20" ry="20"');
      }
      expect(shape.points).toEqual(points);
    }
  }
  const key = { ctrlKey: false, metaKey: false, shiftKey: false, isComposing: false };
  expect(annotationKey({ ...key, key: 'r' }, false)).toBe('rectangle');
  expect(annotationKey({ ...key, key: 'c' }, false)).toBe('circle');
  expect(annotationKey({ ...key, key: 'c' }, true)).toBeUndefined();
});


test('image captions render at the bottom, escape content, and stay fixed during transforms', () => {
  const caption: Annotation = { kind: 'caption', text: '<caption> & hello' };
  const svg = annotationSvg([caption], 0, -1, 800, 600);
  expect(svg).toContain('fill-opacity="0.7"');
  expect(svg).toContain('&lt;caption&gt; &amp; hello');
  expect(svg).toContain('y="547.2"');
  expect(svg).not.toContain('data-resize');
  expect(annotationSvg([caption], 0, 0, 800, 600)).toBe('');
  expect(translate(caption, 10, 20, 800, 600)).toEqual(caption);
  expect(resizeAnnotation(caption, { x: 10, y: 20 }, 800, 600)).toEqual(caption);
});

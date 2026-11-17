import { expect, test } from 'bun:test';
import { annotationSvg } from './annotations';
import { drawOutput, outputSvg, validateOutputSize } from './image-output';
import type { Annotation } from './document';

test('output uses original pixel dimensions, the shared renderer, and the latest completed snapshot', () => {
  const saved: readonly Annotation[] = [];
  const current: readonly Annotation[] = [
    { kind: 'text', x: 120, y: 80, text: 'unsaved <caption>', color: '#ffffff', fontSize: 24 },
    ...(['pen', 'highlighter', 'arrow'] as const).map(kind => ({ kind, points: [{ x: 2, y: 3 }, { x: 200, y: 100 }], color: '#ff4444', width: 4 }))
  ];
  const before = JSON.stringify(current);
  const svg = outputSvg(current, 3200, 1800);
  expect(svg).toBe(`<svg xmlns="http://www.w3.org/2000/svg" width="3200" height="1800" viewBox="0 0 3200 1800">${annotationSvg(current)}</svg>`);
  expect(svg).toContain('unsaved &lt;caption&gt;');
  expect(svg).not.toContain('drop-shadow');
  expect(svg).not.toContain('<rect');
  expect(saved).toEqual([]);
  expect(JSON.stringify(current)).toBe(before);
});

test('composition clears transparent pixels then draws original and overlay using source-over', () => {
  const original = {} as CanvasImageSource, overlay = {} as CanvasImageSource;
  const calls: unknown[][] = [];
  const context = {
    globalAlpha: 0.2, globalCompositeOperation: 'copy' as GlobalCompositeOperation,
    clearRect: (...args: number[]) => { calls.push(['clear', ...args]); },
    drawImage: (...args: unknown[]) => { calls.push(['draw', ...args]); }
  };
  drawOutput(context, original, overlay, 1600, 900);
  expect(context.globalAlpha).toBe(1);
  expect(context.globalCompositeOperation).toBe('source-over');
  expect(calls).toEqual([
    ['clear', 0, 0, 1600, 900],
    ['draw', original, 0, 0, 1600, 900],
    ['draw', overlay, 0, 0, 1600, 900]
  ]);
});

test('invalid or oversized output is rejected before any drawing', () => {
  for (const [width, height] of [[0, 1], [-1, 1], [1.5, 2], [NaN, 1], [Infinity, 1], [40000001, 1]]) {
    expect(() => validateOutputSize(width, height)).toThrow();
    expect(() => outputSvg([], width, height)).toThrow();
  }
  expect(() => validateOutputSize(8000, 5000)).not.toThrow();
});


test('export includes the caption band at original image dimensions', () => {
  const svg = outputSvg([{ kind: 'caption', text: 'Saved caption' }], 800, 600);
  expect(svg).toContain('y="547.2" width="800"');
  expect(svg).toContain('Saved caption');
  expect(svg).not.toContain('data-resize');
});

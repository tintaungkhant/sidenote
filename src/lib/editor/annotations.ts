import type { Annotation } from './document';
export type Point = { x: number; y: number };
export type Tool = 'select' | 'pen' | 'highlighter' | 'arrow' | 'rectangle' | 'circle' | 'text';

/** Focused-view history contains completed immutable operations only; a new edit clears redo. */
export class AnnotationHistory {
  private past: (readonly Annotation[])[] = [];
  private future: (readonly Annotation[])[] = [];
  constructor(public current: readonly Annotation[]) {}
  /** Record one completed operation and abandon any redo branch. */
  commit(next: readonly Annotation[]): readonly Annotation[] {
    this.past.push(this.current); this.future = []; return this.current = next;
  }
  /** Return the current snapshot unchanged when the requested history direction is exhausted. */
  step(redo = false): readonly Annotation[] {
    const source = redo ? this.future : this.past, target = redo ? this.past : this.future, next = source.pop();
    if (next) { target.push(this.current); this.current = next; }
    return this.current;
  }
}

/** CSS pixels map directly to original pixels, independently of device pixel ratio. */
export function imagePoint(x: number, y: number, rect: { left: number; top: number; width: number; height: number }, width: number, height: number): Point {
  return { x: Math.max(0, Math.min(width, (x - rect.left) * width / rect.width)),
    y: Math.max(0, Math.min(height, (y - rect.top) * height / rect.height)) };
}

/** Clamp a whole annotation together so moving it never distorts or exceeds the original image. */
export function translate(annotation: Annotation, dx: number, dy: number, width: number, height: number): Annotation {
  const points = annotation.kind === 'text' ? [annotation] : annotation.points;
  const bounds = points.reduce((b, p) => ({ left: Math.min(b.left, p.x), right: Math.max(b.right, p.x), top: Math.min(b.top, p.y), bottom: Math.max(b.bottom, p.y) }), { left: width, right: 0, top: height, bottom: 0 });
  dx = Math.max(-bounds.left, Math.min(dx, width - bounds.right));
  dy = Math.max(-bounds.top, Math.min(dy, height - bounds.bottom));
  return annotation.kind === 'text' ? { ...annotation, x: annotation.x + dx, y: annotation.y + dy }
    : { ...annotation, points: points.map(p => ({ x: p.x + dx, y: p.y + dy })) };
}

/** Source-pixel selection bounds; text uses an approximate font-width estimate, including empty captions. */
export function annotationBounds(a: Annotation) {
  if (a.kind === 'text') {
    const lines = a.text.split('\n');
    return { left: a.x, top: a.y, right: a.x + Math.max(1, ...lines.map(line => Array.from(line).length)) * a.fontSize * 0.7,
      bottom: a.y + Math.max(1, lines.length) * a.fontSize * 1.2 };
  }
  return a.points.reduce((b, p) => ({ left: Math.min(b.left, p.x), top: Math.min(b.top, p.y), right: Math.max(b.right, p.x), bottom: Math.max(b.bottom, p.y) }),
    { left: a.points[0].x, top: a.points[0].y, right: a.points[0].x, bottom: a.points[0].y });
}

/** Resize uniformly from the top-left without mutating source or exceeding native coordinate/style limits. */
export function resizeAnnotation(a: Annotation, target: Point, width: number, height: number): Annotation {
  const b = annotationBounds(a), w = Math.max(1, b.right - b.left), h = Math.max(1, b.bottom - b.top);
  const size = a.kind === 'text' ? a.fontSize : a.width;
  const scale = Math.max(0.001, Math.min(Math.max(0.05, Math.min((target.x - b.left) / w, (target.y - b.top) / h)),
    Math.max(0.001, (width - b.left) / w), Math.max(0.001, (height - b.top) / h), 4096 / size));
  return a.kind === 'text' ? { ...a, fontSize: a.fontSize * scale }
    : { ...a, width: a.width * scale, points: a.points.map(p => ({ x: b.left + (p.x - b.left) * scale, y: b.top + (p.y - b.top) * scale })) };
}

/** Escape all persisted strings before constructing SVG markup; no source markup is trusted. */
function escape(value: string): string {
  return value.replace(/[&<>"']/g, char => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[char]!));
}

/** Shared vector renderer keeps inline and focused views in the same original-image coordinate space. */
export function annotationSvg(annotations: readonly Annotation[], selected = -1, editing = -1): string {
  return annotations.map((a, index) => {
    const style = `stroke="${escape(a.color)}" stroke-width="${a.kind === 'text' ? 1 : a.width}"`;
    let shape: string;
    if (a.kind === 'text') shape = `<text x="${a.x}" y="${a.y}" fill="${escape(a.color)}" font-size="${a.fontSize}" font-family="sans-serif" dominant-baseline="hanging">${a.text.split('\n').map((line, i) => `<tspan x="${a.x}" dy="${i ? '1.2em' : '0'}">${escape(line)}</tspan>`).join('')}</text>`;
    else if (a.kind === 'rectangle' || a.kind === 'circle') {
      const b = annotationBounds(a), w = b.right - b.left, h = b.bottom - b.top;
      shape = a.kind === 'rectangle'
        ? `<rect x="${b.left}" y="${b.top}" width="${w}" height="${h}" fill="none" ${style}/>`
        : `<ellipse cx="${b.left + w / 2}" cy="${b.top + h / 2}" rx="${w / 2}" ry="${h / 2}" fill="none" ${style}/>`;
    } else {
      shape = `<polyline points="${a.points.map(p => `${p.x},${p.y}`).join(' ')}" fill="none" ${style} stroke-linecap="round" stroke-linejoin="round" opacity="${a.kind === 'highlighter' ? 0.35 : 1}"/>`;
      if (a.kind === 'arrow' && a.points.length > 1) {
        const end = a.points.at(-1)!, start = a.points[0], angle = Math.atan2(end.y - start.y, end.x - start.x), size = a.width * 4;
        shape += `<polyline points="${end.x - size * Math.cos(angle - 0.5)},${end.y - size * Math.sin(angle - 0.5)} ${end.x},${end.y} ${end.x - size * Math.cos(angle + 0.5)},${end.y - size * Math.sin(angle + 0.5)}" fill="none" ${style}/>`;
      }
    }
    // Selection chrome is editor-only. Plain exports call this renderer without a selected index.
    let chrome = '';
    if (index === selected && index !== editing) {
      const b = annotationBounds(a), w = Math.max(8, b.right - b.left), h = Math.max(8, b.bottom - b.top);
      chrome = `<rect x="${b.left}" y="${b.top}" width="${w}" height="${h}" fill="transparent" stroke="#75beff" stroke-width="1" vector-effect="non-scaling-stroke" style="cursor:move"/>` +
        `<rect x="${b.left - 3}" y="${b.top - 3}" width="6" height="6" fill="#252526" stroke="#75beff" vector-effect="non-scaling-stroke" style="cursor:move"/>` +
        `<rect data-resize="true" x="${b.left + w - 5}" y="${b.top + h - 5}" width="10" height="10" fill="#75beff" stroke="#252526" vector-effect="non-scaling-stroke" style="cursor:nwse-resize"/>`;
    }
    return `<g data-annotation="${index}">${index === editing ? '' : shape}${chrome}</g>`;
  }).join('');
}

/** Resolve canvas shortcuts separately from text inputs and IME composition. */
export function annotationKey(event: { key: string; ctrlKey: boolean; metaKey: boolean; shiftKey: boolean; isComposing: boolean }, editing: boolean): string | undefined {
  if (event.isComposing) return;
  if (event.key === 'Escape') return 'exit';
  if (editing) return;
  if (event.ctrlKey || event.metaKey) return event.key.toLowerCase() === 'z' ? (event.shiftKey ? 'redo' : 'undo') : event.key.toLowerCase() === 'y' ? 'redo' : undefined;
  return ({ v: 'select', p: 'pen', h: 'highlighter', a: 'arrow', r: 'rectangle', c: 'circle', t: 'text', Tab: 'cycle', Delete: 'delete', Backspace: 'delete', ArrowLeft: 'left', ArrowRight: 'right', ArrowUp: 'up', ArrowDown: 'down' } as Record<string, string>)[event.key];
}

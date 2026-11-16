import type { Annotation } from './document';
export type Point = { x: number; y: number };
export type Tool = 'select' | 'pen' | 'highlighter' | 'arrow' | 'text';

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

/** Escape all persisted strings before constructing SVG markup; no source markup is trusted. */
function escape(value: string): string {
  return value.replace(/[&<>"']/g, char => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[char]!));
}

/** Shared vector renderer keeps inline and focused views in the same original-image coordinate space. */
export function annotationSvg(annotations: readonly Annotation[], selected = -1): string {
  return annotations.map((a, index) => {
    const style = `stroke="${escape(a.color)}" stroke-width="${a.kind === 'text' ? 1 : a.width}"`;
    let shape: string;
    if (a.kind === 'text') shape = `<text x="${a.x}" y="${a.y}" fill="${escape(a.color)}" font-size="${a.fontSize}" font-family="sans-serif" dominant-baseline="hanging">${a.text.split('\n').map((line, i) => `<tspan x="${a.x}" dy="${i ? '1.2em' : '0'}">${escape(line)}</tspan>`).join('')}</text>`;
    else {
      shape = `<polyline points="${a.points.map(p => `${p.x},${p.y}`).join(' ')}" fill="none" ${style} stroke-linecap="round" stroke-linejoin="round" opacity="${a.kind === 'highlighter' ? 0.35 : 1}"/>`;
      if (a.kind === 'arrow' && a.points.length > 1) {
        const end = a.points.at(-1)!, start = a.points[0], angle = Math.atan2(end.y - start.y, end.x - start.x), size = a.width * 4;
        shape += `<polyline points="${end.x - size * Math.cos(angle - 0.5)},${end.y - size * Math.sin(angle - 0.5)} ${end.x},${end.y} ${end.x - size * Math.cos(angle + 0.5)},${end.y - size * Math.sin(angle + 0.5)}" fill="none" ${style}/>`;
      }
    }
    return `<g data-annotation="${index}" ${index === selected ? 'style="filter:drop-shadow(0 0 2px white)"' : ''}>${shape}</g>`;
  }).join('');
}

/** Resolve canvas shortcuts separately from text inputs and IME composition. */
export function annotationKey(event: { key: string; ctrlKey: boolean; metaKey: boolean; shiftKey: boolean; isComposing: boolean }, editing: boolean): string | undefined {
  if (event.isComposing) return;
  if (event.key === 'Escape') return 'exit';
  if (editing) return;
  if (event.ctrlKey || event.metaKey) return event.key.toLowerCase() === 'z' ? (event.shiftKey ? 'redo' : 'undo') : event.key.toLowerCase() === 'y' ? 'redo' : undefined;
  return ({ v: 'select', p: 'pen', h: 'highlighter', a: 'arrow', t: 'text', Tab: 'cycle', Delete: 'delete', Backspace: 'delete', ArrowLeft: 'left', ArrowRight: 'right', ArrowUp: 'up', ArrowDown: 'down' } as Record<string, string>)[event.key];
}

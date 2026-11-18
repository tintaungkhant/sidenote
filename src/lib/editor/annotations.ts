import type { Annotation } from './document';
export type Point = { x: number; y: number };
export type Tool = 'select' | 'pen' | 'highlighter' | 'arrow' | 'rectangle' | 'circle' | 'text';
export type Viewport = { zoom: number; x: number; y: number };
export const MIN_ZOOM = 0.05;
export const MAX_ZOOM = 32;

/** Clamp zoom to supported bounds and keep an edge of the canvas reachable within the stage. */
export function clampViewport(viewport: Viewport, stageWidth: number, stageHeight: number, imageWidth: number, imageHeight: number): Viewport {
  const zoom = Math.max(MIN_ZOOM, Math.min(MAX_ZOOM, viewport.zoom));
  if (stageWidth <= 0 || stageHeight <= 0 || imageWidth <= 0 || imageHeight <= 0) return { ...viewport, zoom };
  const w = imageWidth * zoom, h = imageHeight * zoom;
  const marginX = Math.min(48, stageWidth * 0.25, w * 0.5);
  const marginY = Math.min(48, stageHeight * 0.25, h * 0.5);
  return {
    zoom,
    x: Math.max(marginX - w, Math.min(stageWidth - marginX, viewport.x)),
    y: Math.max(marginY - h, Math.min(stageHeight - marginY, viewport.y))
  };
}

/** Center and fit the image inside the stage, optionally capping initial open at 100% natural scale. */
export function fitViewport(stageWidth: number, stageHeight: number, imageWidth: number, imageHeight: number, capAtActualSize = false): Viewport {
  if (stageWidth <= 0 || stageHeight <= 0 || imageWidth <= 0 || imageHeight <= 0) return { zoom: 1, x: 0, y: 0 };
  const pad = Math.min(32, stageWidth * 0.08, stageHeight * 0.08);
  const raw = Math.min(Math.max(1, stageWidth - pad * 2) / imageWidth, Math.max(1, stageHeight - pad * 2) / imageHeight);
  const zoom = Math.max(MIN_ZOOM, Math.min(MAX_ZOOM, capAtActualSize ? Math.min(1, raw) : raw));
  return { zoom, x: (stageWidth - imageWidth * zoom) / 2, y: (stageHeight - imageHeight * zoom) / 2 };
}

/** Zoom around a stage-relative anchor point so the image pixel under the cursor stays fixed. */
export function zoomAtPoint(viewport: Viewport, nextZoom: number, anchorX: number, anchorY: number, stageWidth: number, stageHeight: number, imageWidth: number, imageHeight: number): Viewport {
  const zoom = Math.max(MIN_ZOOM, Math.min(MAX_ZOOM, nextZoom));
  const ratio = zoom / viewport.zoom;
  return clampViewport({ zoom, x: anchorX - (anchorX - viewport.x) * ratio, y: anchorY - (anchorY - viewport.y) * ratio }, stageWidth, stageHeight, imageWidth, imageHeight);
}

/** Reset zoom to 100%, keeping the current focus centered on large images and centering small ones. */
export function resetViewport(viewport: Viewport, stageWidth: number, stageHeight: number, imageWidth: number, imageHeight: number): Viewport {
  const next = zoomAtPoint(viewport, 1, stageWidth / 2, stageHeight / 2, stageWidth, stageHeight, imageWidth, imageHeight);
  return clampViewport({
    zoom: 1,
    x: imageWidth <= stageWidth ? (stageWidth - imageWidth) / 2 : next.x,
    y: imageHeight <= stageHeight ? (stageHeight - imageHeight) / 2 : next.y
  }, stageWidth, stageHeight, imageWidth, imageHeight);
}

/** Smooth 25% discrete zoom steps that snap cleanly back onto 100%. */
export function stepZoom(zoom: number, direction: 1 | -1): number {
  const raw = direction > 0 ? zoom * 1.25 : zoom / 1.25;
  return Math.max(MIN_ZOOM, Math.min(MAX_ZOOM, Math.abs(raw - 1) < 0.03 ? 1 : raw));
}

/** Figma-style wheel routing: Ctrl/Cmd or trackpad pinch zooms at cursor; plain wheel pans (Shift for horizontal). */
export function applyWheel(
  viewport: Viewport,
  event: { deltaX: number; deltaY: number; deltaMode: number; ctrlKey: boolean; metaKey: boolean; shiftKey: boolean },
  anchorX: number,
  anchorY: number,
  stageWidth: number,
  stageHeight: number,
  imageWidth: number,
  imageHeight: number
): Viewport {
  const unit = event.deltaMode === 1 ? 16 : event.deltaMode === 2 ? 100 : 1;
  let dx = event.deltaX * unit, dy = event.deltaY * unit;
  if (event.ctrlKey || event.metaKey) {
    const delta = dy !== 0 ? dy : dx;
    const clamped = Math.sign(delta) * Math.min(Math.abs(delta), 24);
    return zoomAtPoint(viewport, viewport.zoom * Math.exp(-clamped * 0.01), anchorX, anchorY, stageWidth, stageHeight, imageWidth, imageHeight);
  }
  if (event.shiftKey && dx === 0) { dx = dy; dy = 0; }
  return clampViewport({ ...viewport, x: viewport.x - dx, y: viewport.y - dy }, stageWidth, stageHeight, imageWidth, imageHeight);
}

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
  if (annotation.kind === 'caption') return annotation;
  const points = annotation.kind === 'text' ? [annotation] : annotation.points;
  const bounds = points.reduce((b, p) => ({ left: Math.min(b.left, p.x), right: Math.max(b.right, p.x), top: Math.min(b.top, p.y), bottom: Math.max(b.bottom, p.y) }), { left: width, right: 0, top: height, bottom: 0 });
  dx = Math.max(-bounds.left, Math.min(dx, width - bounds.right));
  dy = Math.max(-bounds.top, Math.min(dy, height - bounds.bottom));
  return annotation.kind === 'text' ? { ...annotation, x: annotation.x + dx, y: annotation.y + dy }
    : { ...annotation, points: points.map(p => ({ x: p.x + dx, y: p.y + dy })) };
}

/** Source-pixel selection bounds; text uses an approximate font-width estimate, including empty captions. */
export function annotationBounds(a: Annotation) {
  if (a.kind === 'caption') return { left: 0, top: 0, right: 0, bottom: 0 };
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
  if (a.kind === 'caption') return a;
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
export function captionLayout(text: string, width: number, height: number) {
  const fontSize = Math.max(1, Math.min(24, width / 24, height / 6));
  const padding = fontSize / 2;
  const columns = Math.max(1, Math.floor((width - padding * 2) / (fontSize * 0.7)));
  const lines = text.split('\n').flatMap(line => {
    const chars = Array.from(line);
    return chars.length ? Array.from({ length: Math.ceil(chars.length / columns) }, (_, i) => chars.slice(i * columns, (i + 1) * columns).join('')) : [''];
  });
  const top = Math.max(0, height - lines.length * fontSize * 1.2 - padding * 2);
  return { fontSize, padding, lines, top };
}

export function annotationSvg(annotations: readonly Annotation[], selected = -1, editing = -1, width = 0, height = 0, zoom = 1): string {
  const inv = zoom > 0 ? 1 / zoom : 1;
  return annotations.map((a, index) => {
    if (a.kind === 'caption') {
      if (!a.text || index === editing) return '';
      const { fontSize, padding, lines, top } = captionLayout(a.text, width, height);
      return `<g data-annotation="${index}"><rect x="0" y="${top}" width="${width}" height="${height - top}" fill="#000000" fill-opacity="0.7"/><text fill="#ffffff" font-size="${fontSize}" font-family="sans-serif" text-anchor="middle" dominant-baseline="hanging">${lines.map((line, i) => `<tspan x="${width / 2}" y="${top + padding + i * fontSize * 1.2}">${escape(line)}</tspan>`).join('')}</text></g>`;
    }
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
      const b = annotationBounds(a), w = Math.max(8 * inv, b.right - b.left), h = Math.max(8 * inv, b.bottom - b.top);
      chrome = `<rect x="${b.left}" y="${b.top}" width="${w}" height="${h}" fill="transparent" stroke="#75beff" stroke-width="1" vector-effect="non-scaling-stroke" style="cursor:move"/>` +
        `<rect x="${b.left - 3 * inv}" y="${b.top - 3 * inv}" width="${6 * inv}" height="${6 * inv}" fill="#252526" stroke="#75beff" stroke-width="1" vector-effect="non-scaling-stroke" style="cursor:move"/>` +
        `<rect data-resize="true" x="${b.left + w - 5 * inv}" y="${b.top + h - 5 * inv}" width="${10 * inv}" height="${10 * inv}" fill="#75beff" stroke="#252526" stroke-width="1" vector-effect="non-scaling-stroke" style="cursor:nwse-resize"/>`;
    }
    return `<g data-annotation="${index}">${index === editing ? '' : shape}${chrome}</g>`;
  }).join('');
}

/** Resolve canvas shortcuts separately from text inputs and IME composition. */
export function annotationKey(event: { key: string; ctrlKey: boolean; metaKey: boolean; shiftKey: boolean; isComposing: boolean }, editing: boolean): string | undefined {
  if (event.isComposing) return;
  if (event.key === 'Escape') return 'exit';
  if (editing) return;
  if (event.ctrlKey || event.metaKey) {
    const lower = event.key.toLowerCase();
    if (lower === 'z') return event.shiftKey ? 'redo' : 'undo';
    if (lower === 'y') return 'redo';
    if (event.key === '=' || event.key === '+') return 'zoom-in';
    if (event.key === '-' || event.key === '_') return 'zoom-out';
    if (event.key === '0' || event.key === ')') return 'zoom-reset';
    if (event.key === '1' || event.key === '!') return 'zoom-fit';
    return undefined;
  }
  if (event.key === '=' || event.key === '+') return 'zoom-in';
  if (event.key === '-' || event.key === '_') return 'zoom-out';
  if (event.key === '!' || (event.shiftKey && event.key === '1')) return 'zoom-fit';
  if (event.key === ')' || (event.shiftKey && event.key === '0')) return 'zoom-reset';
  return ({ v: 'select', p: 'pen', h: 'highlighter', a: 'arrow', r: 'rectangle', c: 'circle', t: 'text', Tab: 'cycle', Delete: 'delete', Backspace: 'delete', ArrowLeft: 'left', ArrowRight: 'right', ArrowUp: 'up', ArrowDown: 'down' } as Record<string, string>)[event.key];
}

/** Browser-only image presentation; callers provide local attachment sources. */
import { Decoration, EditorView, WidgetType } from '@codemirror/view';
import { imageField, selectionText } from './state';
import type { AttachmentId, ImageBlock } from './document';
import { annotationSvg } from './annotations';

/** Copy or cut the selection, or current line when empty, without delegating marker-bearing text to CodeMirror. */
function exportClipboard(event: ClipboardEvent, view: EditorView, cut: boolean): boolean {
  event.preventDefault();
  if (!event.clipboardData) return true;
  const selection = view.state.selection.main;
  const line = view.state.doc.lineAt(selection.from);
  const from = selection.empty ? line.from : selection.from;
  const to = selection.empty ? Math.min(view.state.doc.length, line.to + 1) : selection.to;
  event.clipboardData.setData('text/plain', selectionText(view.state, from, to));
  if (cut && from !== to) view.dispatch({ changes: { from, to }, selection: { anchor: from }, userEvent: 'delete.cut' });
  return true;
}

class ImageWidget extends WidgetType {
  constructor(readonly block: ImageBlock, readonly load: (id: AttachmentId) => Promise<Blob>, readonly open: (block: ImageBlock) => void) { super(); }
  private releases = new WeakMap<HTMLElement, () => void>();
  /** Stable identity permits widget reuse across text changes. */
  eq(other: ImageWidget): boolean { return this.block === other.block && this.load === other.load && this.open === other.open; }
  /** Load only mounted widgets; each widget owns and revokes its object URL. */
  toDOM(view: EditorView): HTMLElement {
    const image = document.createElement('img');
    const status = document.createElement('span');
    status.className = 'note-image-status';
    status.textContent = 'Loading image…';
    status.setAttribute('role', 'status');
    const button = document.createElement('button');
    button.type = 'button'; button.className = 'note-image-button';
    button.setAttribute('aria-label', 'Annotate image');
    button.onclick = () => this.open(this.block);
    button.append(image, status);
    image.hidden = true;
    image.alt = 'Note image';
    image.className = 'note-image';
    image.draggable = false;
    let alive = true;
    let url: string | undefined;
    this.releases.set(button, () => { alive = false; image.onload = image.onerror = null; image.removeAttribute('src'); if (url) URL.revokeObjectURL(url); });
    const unavailable = () => {
      if (!alive) return;
      image.hidden = true;
      status.hidden = false;
      status.textContent = 'Image unavailable; reopen the tab to retry';
      view.requestMeasure();
    };
    void this.load(this.block.attachmentId).then(blob => {
      if (!alive) return;
      url = URL.createObjectURL(blob);
      image.onload = () => {
        if (!alive) return;
        image.hidden = false;
        status.hidden = true;
        const overlay = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
        overlay.setAttribute('viewBox', `0 0 ${image.naturalWidth} ${image.naturalHeight}`);
        overlay.setAttribute('aria-hidden', 'true');
        overlay.style.cssText = 'position:absolute;inset:0;width:100%;height:100%;pointer-events:none';
        overlay.innerHTML = annotationSvg(this.block.annotations);
        button.append(overlay); view.requestMeasure();
      };
      image.onerror = unavailable;
      image.src = url;
    }).catch(unavailable);
    return button;
  }
  /** Dropped viewport widgets release decoded image resources, including late asynchronous loads. */
  destroy(dom: HTMLElement): void { this.releases.get(dom)?.(); this.releases.delete(dom); }
}

/** Decorate sidecar positions only, preserving literal placeholder characters. */
export function imagePresentation(load: (id: AttachmentId) => Promise<Blob>, open: (block: ImageBlock) => void = () => {}) {
  return [
    EditorView.decorations.compute([imageField], state => Decoration.set(
      state.field(imageField).map(image => Decoration.replace({ widget: new ImageWidget(image.block, load, open) }).range(image.from, image.from + 1))
    )),
    EditorView.domEventHandlers({
      /** Export text without image markers; native image copy is available in the focused annotation view. */
      copy(event, view) {
        return exportClipboard(event, view, false);
      },
      /** Cut exports the same text and applies one undoable deletion. */
      cut(event, view) {
        return exportClipboard(event, view, true);
      },
      /** Native text dragging cannot carry image identity, so it is disabled for this foundation. */
      dragstart(event) { event.preventDefault(); return true; }
    }),
    EditorView.theme({
      '&': { height: '100%', backgroundColor: '#1e1e1e', color: '#d4d4d4' },
      '.cm-scroller': { overflow: 'auto', fontFamily: 'Consolas, Menlo, monospace', fontSize: '14px' },
      '.cm-content': { padding: '12px 0', caretColor: '#ffffff' },
      '.cm-gutters': { backgroundColor: '#1e1e1e', color: '#858585', border: 'none' },
      '.cm-activeLine': { backgroundColor: '#ffffff08' },
      '.cm-cursor': { borderLeftColor: '#ffffff' },
      '&.cm-focused .cm-selectionBackground, .cm-selectionBackground': { backgroundColor: '#264f78' },
      '.note-image-button': { position: 'relative', display: 'inline-block', maxWidth: '100%', padding: '0', border: '0', margin: '4px', verticalAlign: 'middle', lineHeight: '0' },
      '.note-image': { display: 'block', maxWidth: '100%', maxHeight: '300px' },
      '.note-image[hidden], .note-image-status[hidden]': { display: 'none' },
      '.note-image-status': { display: 'block', padding: '12px', lineHeight: '1.4', color: '#a0a0a0' }
    }, { dark: true })
  ];
}

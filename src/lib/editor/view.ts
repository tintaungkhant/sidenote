/** Browser-only image presentation; callers provide local attachment sources. */
import { Decoration, EditorView, WidgetType } from '@codemirror/view';
import { HighlightStyle, syntaxHighlighting } from '@codemirror/language';
import { tags } from '@lezer/highlight';
import { imageField, selectionText } from './state';
import type { AttachmentId, ImageBlock } from './document';
import { annotationSvg } from './annotations';

/** Modern developer syntax highlighting theme inspired by Cursor & One Dark Pro. */
export const cursorHighlightStyle = HighlightStyle.define([
  { tag: tags.keyword, color: '#c678dd', fontWeight: 'bold' },
  { tag: [tags.name, tags.deleted, tags.character, tags.propertyName, tags.macroName], color: '#e06c75' },
  { tag: [tags.function(tags.variableName), tags.labelName], color: '#61afef' },
  { tag: [tags.color, tags.constant(tags.name), tags.standard(tags.name)], color: '#d19a66' },
  { tag: [tags.definition(tags.name), tags.separator], color: '#e5c07b' },
  { tag: [tags.typeName, tags.className, tags.number, tags.changed, tags.annotation, tags.modifier, tags.self, tags.namespace], color: '#e5c07b' },
  { tag: [tags.operator, tags.operatorKeyword, tags.url, tags.escape, tags.regexp, tags.link, tags.special(tags.string)], color: '#56b6c2' },
  { tag: [tags.meta, tags.comment], color: '#7f848e', fontStyle: 'italic' },
  { tag: tags.strong, fontWeight: 'bold' },
  { tag: tags.emphasis, fontStyle: 'italic' },
  { tag: tags.strikethrough, textDecoration: 'line-through' },
  { tag: tags.link, color: '#61afef', textDecoration: 'underline' },
  { tag: tags.heading, fontWeight: 'bold', color: '#e06c75' },
  { tag: [tags.atom, tags.bool, tags.special(tags.variableName)], color: '#d19a66' },
  { tag: [tags.processingInstruction, tags.string, tags.inserted], color: '#98c379' },
  { tag: tags.invalid, color: '#f44747' }
]);

export const cursorSyntaxExtension = syntaxHighlighting(cursorHighlightStyle);

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
    button.title = 'Click to open annotation editor';
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
        overlay.innerHTML = annotationSvg(this.block.annotations, -1, -1, image.naturalWidth, image.naturalHeight);
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
      '&': { height: '100%', backgroundColor: '#131418', color: '#e4e7ec' },
      '.cm-scroller': { overflow: 'auto', fontFamily: '"JetBrains Mono", "Cascadia Code", "Fira Code", Menlo, Monaco, Consolas, monospace', fontSize: '13.5px', lineHeight: '1.65' },
      '.cm-content': { padding: '16px 20px', caretColor: '#38bdf8' },
      '.cm-gutters': { backgroundColor: '#131418', color: '#4b5563', borderRight: '1px solid #1f2128', paddingRight: '4px' },
      '.cm-gutterElement': { paddingLeft: '10px', paddingRight: '10px' },
      '.cm-activeLine': { backgroundColor: 'rgba(255, 255, 255, 0.025)' },
      '.cm-activeLineGutter': { color: '#cbd5e1', backgroundColor: 'rgba(255, 255, 255, 0.04)' },
      '.cm-cursor': { borderLeft: '2px solid #38bdf8' },
      '&.cm-focused .cm-selectionBackground, .cm-selectionBackground': { backgroundColor: 'rgba(99, 102, 241, 0.3)' },
      '&.cm-focused .cm-matchingBracket': { outline: '1px solid #38bdf8', backgroundColor: 'rgba(56, 189, 248, 0.15)' },
      '.note-image-button': { position: 'relative', display: 'inline-block', maxWidth: '100%', padding: '0', border: '1px solid #282a35', borderRadius: '8px', overflow: 'hidden', margin: '8px 0', verticalAlign: 'middle', lineHeight: '0', backgroundColor: '#181920', boxShadow: '0 4px 20px rgba(0,0,0,0.3)', transition: 'all 0.15s ease', cursor: 'pointer' },
      '.note-image-button:hover': { borderColor: '#6366f1', boxShadow: '0 6px 24px rgba(99, 102, 241, 0.25)' },
      '.note-image': { display: 'block', maxWidth: '100%', maxHeight: '340px' },
      '.note-image[hidden], .note-image-status[hidden]': { display: 'none' },
      '.note-image-status': { display: 'block', padding: '16px', lineHeight: '1.4', color: '#9ca3af', fontSize: '12px' }
    }, { dark: true })
  ];
}

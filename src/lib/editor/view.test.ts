import { expect, test } from 'bun:test';
import { EditorView, type DecorationSet } from '@codemirror/view';
import { createNoteState } from './state';
import { imagePresentation } from './view';
import type { ImageBlock } from './document';

// Exercise the real decoration widget with a small DOM substitute, including delayed IPC.
test('initial image mount shows a placeholder and cleanup belongs to each DOM mount', async () => {
  const previous = Object.getOwnPropertyDescriptor(globalThis, 'document');
  class Element {
    children: Element[] = [];
    hidden = false;
    textContent = '';
    src = '';
    style = { cssText: '' };
    onload?: () => void;
    onerror?: () => void;
    append(...children: Element[]) { this.children.push(...children); }
    setAttribute() {}
    removeAttribute(name: string) { if (name === 'src') this.src = ''; }
  }
  Object.defineProperty(globalThis, 'document', { configurable: true, value: {
    createElement: () => new Element(), createElementNS: () => new Element()
  } });
  try {
    let resolve!: (blob: Blob) => void;
    const pending = new Promise<Blob>(done => { resolve = done; });
    const block: ImageBlock = { kind: 'image', id: 'image' as ImageBlock['id'], attachmentId: 'asset' as ImageBlock['attachmentId'], annotations: [] };
    const state = createNoteState({ version: 1, blocks: [block] }, imagePresentation(() => pending));
    const decorations = state.facet(EditorView.decorations)[0] as DecorationSet;
    const widget = decorations.iter().value!.spec.widget;
    const view = { requestMeasure() {} } as unknown as EditorView;
    const first = widget.toDOM(view) as unknown as Element;
    const second = widget.toDOM(view) as unknown as Element;
    expect(second.children[0].hidden).toBe(true);
    expect(second.children[1].textContent).toBe('Loading image…');
    widget.destroy(first as unknown as HTMLElement);
    resolve(new Blob(['image'], { type: 'image/png' }));
    await pending;
    await Promise.resolve();
    const image = second.children[0];
    expect(image.src).toStartWith('blob:');
    image.onload!();
    expect(image.hidden).toBe(false);
    expect(second.children[1].hidden).toBe(true);
    image.onerror!();
    expect(image.hidden).toBe(true);
    expect(second.children[1].textContent).toContain('Image unavailable');
    widget.destroy(second as unknown as HTMLElement);
    expect(image.src).toBe('');
  } finally {
    if (previous) Object.defineProperty(globalThis, 'document', previous);
    else Reflect.deleteProperty(globalThis, 'document');
  }
});

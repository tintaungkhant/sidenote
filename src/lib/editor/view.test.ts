import { expect, test } from 'bun:test';
import { EditorState } from '@codemirror/state';
import { SearchQuery, search, setSearchQuery } from '@codemirror/search';
import { EditorView, type DecorationSet } from '@codemirror/view';
import { createNoteState } from './state';
import { buildSearchDecorations, imagePresentation, referencePresentation } from './view';
import { referenceState, setMetadata } from './references';
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
    expect(second.children[0].children[0].hidden).toBe(true);
    expect(second.children[0].children[1].textContent).toBe('Loading image…');
    widget.destroy(first as unknown as HTMLElement);
    resolve(new Blob(['image'], { type: 'image/png' }));
    await pending;
    await Promise.resolve();
    const image = second.children[0].children[0];
    expect(image.src).toStartWith('blob:');
    image.onload!();
    expect(image.hidden).toBe(false);
    expect(second.children[0].children[1].hidden).toBe(true);
    image.onerror!();
    expect(image.hidden).toBe(true);
    expect(second.children[0].children[1].textContent).toContain('Image unavailable');
    widget.destroy(second as unknown as HTMLElement);
    expect(image.src).toBe('');
  } finally {
    if (previous) Object.defineProperty(globalThis, 'document', previous);
    else Reflect.deleteProperty(globalThis, 'document');
  }
});

test('search decorations highlight all matches and mark selected match without opening CodeMirror search panel', () => {
  let state = EditorState.create({ doc: 'Alpha beta ALPHA', extensions: [search()] });
  state = state.update({
    effects: setSearchQuery.of(new SearchQuery({ search: 'alpha', caseSensitive: false, literal: true })),
    selection: { anchor: 11, head: 16 },
  }).state;
  const set = buildSearchDecorations(state, [{ from: 0, to: state.doc.length }]);
  const found: [number, number, string][] = [];
  for (let iter = set.iter(); iter.value; iter.next()) {
    found.push([iter.from, iter.to, iter.value.spec.class as string]);
  }
  expect(found).toEqual([
    [0, 5, 'cm-note-match'],
    [11, 16, 'cm-note-match cm-note-match-active'],
  ]);

  const cleared = state.update({
    effects: setSearchQuery.of(new SearchQuery({ search: '', literal: true })),
  }).state;
  expect(buildSearchDecorations(cleared, [{ from: 0, to: cleared.doc.length }]).size).toBe(0);
});


test('reference previews reuse decoded images on text edits but refresh changed annotations', () => {
  const load = async () => new Blob(['image']);
  const metadata = { version: 1 as const, images: { './a.png': { id: 'image', annotations: [] } } };
  const initial = referenceState('[image:./a.png]\ntext', metadata, referencePresentation('plaintext', load, () => {}));
  const widget = (state: EditorState) => {
    for (const decoration of state.facet(EditorView.decorations)) {
      if (typeof decoration !== 'function') {
        const value = decoration.iter().value;
        if (value?.spec.widget) return value.spec.widget;
      }
    }
    throw new Error('Expected a preview widget');
  };
  const edited = initial.update({ changes: { from: initial.doc.length, insert: ' edited' } }).state;
  expect(widget(initial).eq(widget(edited))).toBe(true);
  const annotated = edited.update({ effects: setMetadata.of({ version: 1, images: { './a.png': { id: 'image', annotations: [{ kind: 'caption', text: 'new caption' }] } } }) }).state;
  expect(widget(edited).eq(widget(annotated))).toBe(false);
});

/** CodeMirror owns position mapping and history; the sidecar owns image identity. */
import { EditorState, StateEffect, StateField, type Extension, type TransactionSpec } from '@codemirror/state';
import { history, invertedEffects, isolateHistory } from '@codemirror/commands';
import { SearchQuery } from '@codemirror/search';
import { copyText, encodeDocument, type ImageBlock, type NoteDocument, type PlacedImage } from './document';

export const restoreImages = StateEffect.define<readonly PlacedImage[]>();

export const imageField = StateField.define<readonly PlacedImage[]>({
  create: () => [],
  /** Drop deleted image characters and map surviving positions through each change. */
  update(images, transaction) {
    const restored = transaction.effects.find(effect => effect.is(restoreImages));
    if (restored) return restored.value;
    if (!transaction.docChanged) return images;
    return images.flatMap(image => {
      let deleted = false;
      transaction.changes.iterChangedRanges((from, to) => {
        if (from <= image.from && to > image.from) deleted = true;
      });
      return deleted ? [] : [{ ...image, from: transaction.changes.mapPos(image.from, 1) }];
    });
  }
});

export const imageHistory = invertedEffects.of(transaction =>
  transaction.docChanged || transaction.effects.some(effect => effect.is(restoreImages))
    ? [restoreImages.of(transaction.startState.field(imageField))] : []
);

/** Create a headless-testable state, including image-aware undo and plain-text clipboard behavior. */
export function createNoteState(document: NoteDocument, extensions: Extension = []): EditorState {
  const encoded = encodeDocument(document);
  return EditorState.create({
    doc: encoded.text,
    extensions: [EditorState.lineSeparator.of('\n'), imageField.init(() => encoded.images), history(), imageHistory,
      extensions]
  });
}

/** Export text for a selection without leaking internal image characters. */
export function selectionText(state: EditorState, from: number, to: number): string {
  return copyText({ text: state.doc.toString(), images: state.field(imageField) }, from, to);
}

/** Insert an image at the main selection, preserving surviving image placements in the same history entry. */
export function insertImage(state: EditorState, block: ImageBlock): TransactionSpec {
  const { from, to } = state.selection.main;
  const changes = state.changes({ from, to, insert: '\uFFFC' });
  const mapped = state.update({ changes }).state.field(imageField);
  const images = [...mapped, { from, block }].sort((a, b) => a.from - b.from);
  return { changes, effects: restoreImages.of(images), selection: { anchor: from + 1 }, userEvent: 'input.paste', annotations: isolateHistory.of('full') };
}

/** Match literal source text while excluding matches that include an image placeholder. */
export function textSearch(query: string): SearchQuery {
  return new SearchQuery({ search: query, literal: true,
    test: (_match, state, from, to) => !state.field(imageField).some(image => image.from >= from && image.from < to)
  });
}

/** Move a range to an outside boundary while retaining image IDs, annotations, and one undo step. */
export function moveRange(state: EditorState, from: number, to: number, target: number): TransactionSpec {
  if (from < 0 || to > state.doc.length || from > to || target < 0 || target > state.doc.length) throw new RangeError('Invalid move range');
  if (target >= from && target <= to || from === to) return {};
  const insert = state.sliceDoc(from, to);
  const changes = state.changes([{ from, to }, { from: target, insert }].sort((a, b) => a.from - b.from));
  const start = target < from ? target : target - (to - from);
  const images = state.field(imageField).map(image => ({
    ...image,
    from: image.from >= from && image.from < to ? start + image.from - from : changes.mapPos(image.from, 1)
  })).sort((a, b) => a.from - b.from);
  return { changes, effects: restoreImages.of(images), selection: { anchor: start, head: start + insert.length }, userEvent: 'move' };
}

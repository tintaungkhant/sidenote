/** Persisted document data. Image placement is separate from the editor's text. */
export type AttachmentId = string & { readonly attachmentId: unique symbol };
export type ImageId = string & { readonly imageId: unique symbol };
export type Annotation =
  | { kind: 'pen' | 'highlighter' | 'arrow' | 'rectangle' | 'circle'; points: readonly { x: number; y: number }[]; color: string; width: number }
  | { kind: 'text'; x: number; y: number; text: string; color: string; fontSize: number };
export type ImageBlock = { kind: 'image'; id: ImageId; attachmentId: AttachmentId; annotations: readonly Annotation[] };
export type NoteDocument = { version: 1; blocks: readonly ({ kind: 'text'; text: string } | ImageBlock)[] };
export type PlacedImage = { from: number; block: ImageBlock };
export type EditorDocument = { text: string; images: readonly PlacedImage[] };

/** Encode each image as one character, identified only by its sidecar position. Literal U+FFFC remains ordinary text. */
export function encodeDocument(document: NoteDocument): EditorDocument {
  let text = '';
  const images: PlacedImage[] = [];
  for (const block of document.blocks) {
    if (block.kind === 'text') text += block.text;
    else {
      images.push({ from: text.length, block });
      text += '\uFFFC';
    }
  }
  return { text, images };
}

/** Recover ordered blocks without persisting editor placeholders; adjacent text is normalized. */
export function decodeDocument({ text, images }: EditorDocument): NoteDocument {
  const blocks: NoteDocument['blocks'][number][] = [];
  let offset = 0;
  for (const image of images) {
    if (image.from < offset || text[image.from] !== '\uFFFC') throw new Error('Invalid image placement');
    if (image.from > offset) blocks.push({ kind: 'text', text: text.slice(offset, image.from) });
    blocks.push(image.block);
    offset = image.from + 1;
  }
  if (offset < text.length || !blocks.length) blocks.push({ kind: 'text', text: text.slice(offset) });
  return { version: 1, blocks };
}

/** Plain-text copy omits only real images, preserving literal marker-like characters. */
export function copyText(document: EditorDocument, from = 0, to = document.text.length): string {
  let result = '';
  let offset = from;
  for (const image of document.images) {
    if (image.from < from || image.from >= to) continue;
    result += document.text.slice(offset, image.from);
    offset = image.from + 1;
  }
  return result + document.text.slice(offset, to);
}

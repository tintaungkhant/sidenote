/** Image identity belongs to a relative source reference, never a persisted character offset. */
import type { Extension } from '@codemirror/state';
import { EditorState, StateEffect, StateField } from '@codemirror/state';
import { history, invertedEffects } from '@codemirror/commands';
import { syntaxTree, ensureSyntaxTree } from '@codemirror/language';
import type { AttachmentId, ImageId, PlacedImage } from './document';
import type { Language, Metadata } from '../files';
export type Reference = { from: number; to: number; path: string };
export function references(text: string, language: Language): Reference[] {
  const result: Reference[] = [];
  const pattern = language === 'markdown' ? /!\[[^\]\n]*\]\(([^\s)]+)\)/g
    : language === 'html' ? /^[\t ]*<!--\s*\[image:([^\]\s]+)\]\s*-->[\t ]*$/gm
    : language === 'css' ? /^[\t ]*\/\*\s*\[image:([^\]\s]+)\]\s*\*\/[\t ]*$/gm
    : ['javascript', 'typescript', 'php'].includes(language) ? /^[\t ]*\/\/\s*\[image:([^\]\s]+)\][\t ]*$/gm
    : language === 'plaintext' ? /^\[image:([^\]\s]+)\][\t ]*$/gm : /$^/g;
  for (const match of text.matchAll(pattern)) result.push({ from: match.index!, to: match.index! + match[0].length, path: match[1] });
  return result;
}
export function imageSyntax(path: string, language: Language): string {
  if (language === 'json') throw new Error('JSON has no comments. Save As Markdown or another format before inserting images.');
  if (language === 'markdown') return `![image](${path})`;
  if (language === 'html') return `<!-- [image:${path}] -->`;
  if (language === 'css') return `/* [image:${path}] */`;
  if (language === 'plaintext') return `[image:${path}]`;
  return `// [image:${path}]`;
}
export const setMetadata = StateEffect.define<Metadata>();
export const metadataField = StateField.define<Metadata>({ create: () => ({ version: 1, images: {} }), update: (value, tr) => tr.effects.find(e => e.is(setMetadata))?.value ?? value });
export const metadataHistory = invertedEffects.of(tr => tr.effects.some(e => e.is(setMetadata)) ? [setMetadata.of(tr.startState.field(metadataField))] : []);
export function referenceState(text: string, metadata: Metadata, extensions: Extension): EditorState {
  return EditorState.create({ doc: text, extensions: [history(), metadataField.init(() => metadata), metadataHistory, extensions] });
}
/** Render only real comment syntax in code, ignoring marker-shaped strings and template literals. */
export function sourceImages(state: EditorState, language: Language): (PlacedImage & { to: number })[] {
  const metadata = state.field(metadataField);
  return references(state.doc.toString(), language).filter(ref => {
    if (language === 'plaintext') return true;
    if (language === 'markdown') {
      for (let node = syntaxTree(state).resolveInner(ref.from + 1,1); node; node = node.parent!) if (node.name === 'Image') return true;
      return false;
    }
    const pos = ref.from + state.doc.sliceString(ref.from, ref.to).search(/\S/);
    return /comment/i.test(syntaxTree(state).resolveInner(pos + 1, 1).name);
  }).map(ref => ({ from: ref.from, to: ref.to, block: { kind: 'image', id: (metadata.images[ref.path]?.id ?? ref.path) as ImageId, attachmentId: ref.path as AttachmentId, annotations: metadata.images[ref.path]?.annotations ?? [] } }));
}
/** Require an empty source line outside strings, expressions, and comments before adding code comments. */
export function insertion(state: EditorState, language: Language, path: string) {
  const selection = state.selection.main;
  const line = state.doc.lineAt(selection.from);
  if (language !== 'markdown' && language !== 'plaintext') {
    if (!selection.empty || line.text.trim()) throw new Error('Insert images on an empty line outside code constructs.');
    const tree = syntaxTree(state);
    let node = tree.resolveInner(selection.from, -1);
    for (; node.parent; node = node.parent) if (/string|template|comment|attribute|jsx|script|style/i.test(node.name)) throw new Error('Cannot insert an image inside a string, comment, attribute, or embedded language.');
    if (language === 'php') {
      // PHP comments are valid only within PHP regions. Never introduce PHP tags implicitly.
      const before = state.doc.sliceString(0, selection.from);
      if (before.lastIndexOf('<?') <= before.lastIndexOf('?>')) throw new Error('Place the image on an empty line inside a PHP block.');
    }
    if (['javascript','typescript'].includes(language) && /\\\s*$/.test(state.doc.line(Math.max(1,line.number - 1)).text)) throw new Error('Cannot insert on a continued source line.');
  }
  const source = imageSyntax(path, language);
  const spec = { changes: { from: selection.from, to: selection.to, insert: `${source}\n` }, selection: { anchor: selection.from + source.length + 1 } };
  if (language !== 'plaintext' && language !== 'markdown') {
    const next = state.update(spec).state;
    if (!ensureSyntaxTree(next,next.doc.length,1000) || !sourceImages(next,language).some(r => r.from === selection.from && r.block.attachmentId === path)) throw new Error('An image comment is not valid at this source location.');
  }
  return spec;
}

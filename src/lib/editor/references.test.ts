import { expect, test } from 'bun:test';
import type { Transaction } from '@codemirror/state';
import { javascript } from '@codemirror/lang-javascript';
import { json } from '@codemirror/lang-json';
import { html } from '@codemirror/lang-html';
import { css } from '@codemirror/lang-css';
import { php } from '@codemirror/lang-php';
import { markdown } from '@codemirror/lang-markdown';
import { undo, redo } from '@codemirror/commands';
import { ensureSyntaxTree } from '@codemirror/language';
import type { Language, Metadata } from '../files';
import { imageSyntax, insertion, references, referenceState, sourceImages, metadataField, setMetadata } from './references';
const metadata: Metadata = { version: 1, images: { './assets/a.png': { id: 'stable-id', annotations: [{ kind: 'caption', text: 'caption' }] } } };

test('portable source syntax and stable metadata follow moved references without persisted offsets', () => {
  for (const language of ['markdown','plaintext','javascript','typescript','php','html','css'] as Language[]) {
    const source = imageSyntax('./assets/a.png',language);
    expect(references(source,language).map(r => r.path)).toEqual(['./assets/a.png']);
  }
  let state = referenceState('prefix\n![image](./assets/a.png)', metadata, markdown());
  const before = sourceImages(state,'markdown')[0];
  state = state.update({ changes: { from: 0, insert: 'new line\n' } }).state;
  const after = sourceImages(state,'markdown')[0];
  expect(after.from).toBe(before.from+9); expect(String(after.block.id)).toBe('stable-id'); expect(after.block.annotations).toEqual(before.block.annotations);
  expect(JSON.stringify(state.field(metadataField))).not.toContain('from');
});
test('Markdown examples inside fenced code are ordinary text', () => {
  const state = referenceState('```md\n![image](./assets/example.png)\n```\n\n![image](./assets/a.png)',metadata,markdown());
  ensureSyntaxTree(state,state.doc.length,1000);
  expect(sourceImages(state,'markdown').map(r=>String(r.block.attachmentId))).toEqual(['./assets/a.png']);
});
test('code marker-shaped strings are not image references', () => {
  const source = 'const template = `\n// [image:./assets/fake.png]\n`;\n// [image:./assets/a.png]\n';
  const state = referenceState(source,metadata,javascript()); ensureSyntaxTree(state,state.doc.length,1000);
  expect(sourceImages(state,'javascript').map(r => String(r.block.attachmentId))).toEqual(['./assets/a.png']);
});
test('insertion refuses JSON, selected code, templates, JSX, HTML scripts and PHP outside tags', () => {
  expect(() => imageSyntax('./a.png','json')).toThrow();
  expect(() => insertion(referenceState('',metadata,json()),'json','./a.png')).toThrow();
  let state = referenceState('const x = `\n\n`;',metadata,javascript()); state = state.update({ selection: { anchor: 12 } }).state;
  expect(() => insertion(state,'javascript','./a.png')).toThrow();
  const jsx = referenceState('const x = <div>\n\n</div>',metadata,javascript({jsx:true}));
  expect(() => insertion(jsx.update({selection:{anchor:16}}).state,'javascript','./a.png')).toThrow();
  const script = referenceState('<script>\n\n</script>',metadata,html());
  expect(() => insertion(script.update({selection:{anchor:9}}).state,'html','./a.png')).toThrow();
  expect(() => insertion(referenceState('',metadata,php()),'php','./a.png')).toThrow();
});
test('valid comment insertion remains valid source and source plus metadata undo together', () => {
  for (const [language, extension, text] of [['javascript',javascript(),''],['css',css(),''],['html',html(),''],['php',php(),'<?php\n\n']] as const) {
    let state = referenceState(text,metadata,extension);
    state = state.update({selection:{anchor:text.length}}).state;
    const nextMeta = { ...metadata, images: { ...metadata.images, './b.png': { id: 'second', annotations: [] } } };
    state = state.update({ ...insertion(state,language,'./b.png'), effects: setMetadata.of(nextMeta) }).state;
    ensureSyntaxTree(state,state.doc.length,1000);
    expect(sourceImages(state,language).map(r => String(r.block.attachmentId))).toContain('./b.png');
    const target = { get state() { return state; }, dispatch(tr: Transaction) { state = tr.state; } };
    expect(undo(target)).toBe(true); expect(state.doc.toString()).toBe(text); expect(state.field(metadataField)).toEqual(metadata);
    expect(redo(target)).toBe(true); expect(state.field(metadataField)).toEqual(nextMeta);
  }
});

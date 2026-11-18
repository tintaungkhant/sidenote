import '../../src/app.css';
import { EditorView, lineNumbers } from '@codemirror/view';
import { markdown } from '@codemirror/lang-markdown';
import { referenceState } from '../../src/lib/editor/references';
import { referencePresentation } from '../../src/lib/editor/view';

const canvas = document.createElement('canvas');
const portrait = new URLSearchParams(location.search).has('portrait');
canvas.width = portrait ? 90 : 160; canvas.height = portrait ? 160 : 90;
const context = canvas.getContext('2d')!;
context.fillStyle = '#38bdf8';
context.fillRect(0, 0, canvas.width, canvas.height);
context.strokeStyle = '#facc15'; context.lineWidth = 8;
context.strokeRect(4, 4, canvas.width - 8, canvas.height - 8);
const image = new Promise<Blob>(resolve => canvas.toBlob(blob => resolve(blob!)));
let finish: (blob: Blob) => void;
let fail: (error: Error) => void;
let loads = 0;
const pending = new Promise<Blob>((resolve,reject) => { finish = resolve; fail = reject; });
const host = document.getElementById('editor')!;
if (new URLSearchParams(location.search).has('compact')) host.style.height = '300px';
if (new URLSearchParams(location.search).has('hidden')) host.style.display = 'none';
const view = new EditorView({ parent: document.getElementById('editor')!, state: referenceState(
  '# Heading\n\n![image](./assets/example.png)\n' + '\n'.repeat(new URLSearchParams(location.search).has('short') ? 1 : 17) + 'Text after the image.\n' + 'Wrapped source text '.repeat(15),
  { version: 1, images: { './assets/example.png': { id: 'example', annotations: [{ kind: 'caption', text: 'Preview caption — entire image visible' }] } } }, [markdown(), lineNumbers(), (new URLSearchParams(location.search).has('nowrap') ? [] : EditorView.lineWrapping), referencePresentation('markdown', () => { loads++; return pending; }, block => { (window as any).openedImage = block; })]
) });

Object.assign(window, {
  view,
  revealEditor: () => { host.style.display = ''; },
  finishImage: async () => finish(await image),
  failImage: () => fail(new Error('Missing image')),
  imageLoadCount: () => loads,
  insertAbove: () => view.dispatch({ changes: { from: 0, insert: 'Inserted line\n' } }),
  removeReference: () => view.dispatch({ changes: { from: view.state.doc.toString().indexOf('![image]'), to: view.state.doc.toString().indexOf('![image]') + '![image](./assets/example.png)'.length } }),
  resizeEditor: () => { document.getElementById('editor')!.style.width = '350px'; view.requestMeasure(); },
  alignment: () => {
    const editor = view.dom.getBoundingClientRect();
    return [...view.dom.querySelectorAll<HTMLElement>('.cm-lineNumbers .cm-gutterElement')]
      .filter(element => element.style.visibility !== 'hidden' && /^\d+$/.test(element.textContent ?? ''))
      .map(element => {
        const number = Number(element.textContent);
        const coords = view.coordsAtPos(view.state.doc.line(number).from)!;
        return { number, gutter: element.getBoundingClientRect().top - editor.top, text: coords.top - editor.top };
      });
  }
});

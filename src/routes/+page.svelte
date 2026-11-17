<script lang="ts">
  import '../app.css';
  import { onMount, tick } from 'svelte';
  import { Compartment, type EditorState } from '@codemirror/state';
  import { EditorView, drawSelection, highlightActiveLine, keymap, lineNumbers } from '@codemirror/view';
  import { defaultKeymap, historyKeymap, indentWithTab, undo, redo } from '@codemirror/commands';
  import { bracketMatching, indentOnInput, ensureSyntaxTree } from '@codemirror/language';
  import { SearchQuery, findNext, search, setSearchQuery } from '@codemirror/search';
  import { javascript } from '@codemirror/lang-javascript';
  import { json } from '@codemirror/lang-json';
  import { html } from '@codemirror/lang-html';
  import { markdown } from '@codemirror/lang-markdown';
  import { css } from '@codemirror/lang-css';
  import { php } from '@codemirror/lang-php';
  import { listen } from '@tauri-apps/api/event';
  import { invoke } from '@tauri-apps/api/core';
  import AnnotationEditor from '$lib/editor/AnnotationEditor.svelte';
  import type { Annotation, AttachmentId, ImageBlock } from '$lib/editor/document';
  import { flattenImage } from '$lib/editor/image-output';
  import { referencePresentation, cursorSyntaxExtension } from '$lib/editor/view';
  import { insertion, imageSyntax, metadataField, referenceState, references, sourceImages, setMetadata } from '$lib/editor/references';
  import { errorMessage } from '$lib/autosave';
  import { tabShortcut } from '$lib/tabs';
  import * as storage from '$lib/storage';
  import * as files from '$lib/files';

  let workspace = $state.raw<files.Workspace>({ version: 1, buffers: [], active: null, root: null, wrap: true, sidebar: false });
  let active = $derived(workspace.buffers.find(b => b.id === workspace.active));
  let host: HTMLDivElement;
  let view: EditorView | undefined;
  let viewId: string | undefined;
  const states = new Map<string, EditorState>();
  const wrapSlot = new Compartment();
  const editable = new Compartment();
  let loading = $state(true), busy = $state(false), frozen = $state(false), startupError = $state(''), notice = $state(''), recoveryError = $state('');
  let panel = $state<'explorer' | 'search'>('explorer'), query = $state(''), findQuery = $state(''), findVisible = $state(false);
  let entries = $state.raw<files.Entry[]>([]);
  let closeTarget = $state<string | null>(null);
  let closeDialog: HTMLDialogElement;
  let annotation = $state.raw<{ block: ImageBlock; blob: Blob } | undefined>();
  let closed: files.Buffer[] = [];
  let snapshotQueue: Promise<void> = Promise.resolve();
  let action: Promise<void> = Promise.resolve();
  let timer: ReturnType<typeof setTimeout> | undefined;
  let disposed = false, polling = false, started = false;
  let exitWork: Promise<void> | undefined;
  let boot: Promise<void> = Promise.resolve();
  let outputWork: Promise<string> | undefined;
  let menuBuffer = $state<string | null>(null);
  let tabMenu: HTMLDivElement;

  function patch(id: string, update: Partial<files.Buffer>) {
    workspace = { ...workspace, buffers: workspace.buffers.map(b => b.id === id ? { ...b, ...update } : b) };
  }
  function capture() {
    if (!view || !viewId) return;
    const selection = view.state.selection.main;
    patch(viewId, { text: view.state.doc.toString(), metadata: view.state.field(metadataField), anchor: selection.anchor, head: selection.head, scrollTop: view.scrollDOM.scrollTop });
  }
  function checkpoint(): Promise<void> {
    capture();
    clearTimeout(timer); timer = undefined;
    const snapshot = JSON.stringify(workspace);
    const pending = snapshotQueue.catch(() => {}).then(() => files.operation<void>({ kind: 'snapshot', snapshot }));
    snapshotQueue = pending;
    void pending.then(() => { recoveryError = ''; }, error => { recoveryError = errorMessage(error); });
    return pending;
  }
  function scheduleRecovery() {
    if (timer || !started) return;
    timer = setTimeout(() => { timer = undefined; void checkpoint().catch(() => {}); }, 250);
  }
  function syntax(language: files.Language) {
    return language === 'javascript' ? javascript({ jsx: true }) : language === 'typescript' ? javascript({ typescript: true, jsx: true }) : language === 'json' ? json() : language === 'html' ? html() : language === 'markdown' ? markdown() : language === 'css' ? css() : language === 'php' ? php() : [];
  }
  async function imageBlob(buffer: files.Buffer, reference: AttachmentId): Promise<Blob> {
    const bytes = await files.operation<number[]>({ kind: 'image', document: buffer.path, reference });
    return new Blob([new Uint8Array(bytes)]);
  }
  function unmount() {
    capture();
    if (view && viewId) states.set(viewId, view.state);
    view?.destroy(); view = undefined; viewId = undefined;
  }
  function mount(id: string | null) {
    if (id === viewId) return;
    unmount();
    workspace = { ...workspace, active: id };
    const buffer = workspace.buffers.find(b => b.id === id);
    if (!buffer) return;
    const language = files.languageFor(buffer.path);
    viewId = buffer.id;
    const state = states.get(buffer.id) ?? referenceState(buffer.text, buffer.metadata, [
      lineNumbers(), drawSelection(), highlightActiveLine(), indentOnInput(), bracketMatching(), cursorSyntaxExtension, search(), syntax(language),
      referencePresentation(language, reference => imageBlob(workspace.buffers.find(b => b.id === buffer.id) ?? buffer, reference), openAnnotation),
      wrapSlot.of(workspace.wrap ? EditorView.lineWrapping : []), editable.of(EditorView.editable.of(!frozen && !busy)),
      EditorView.contentAttributes.of({ 'aria-label': 'File editor' }),
      keymap.of([{ key: 'Mod-f', run: () => { void openFind(); return true; } }, indentWithTab, ...defaultKeymap, ...historyKeymap]),
      EditorView.domEventHandlers({ paste(event) { const text = event.clipboardData?.getData('text/plain') ?? ''; event.preventDefault(); run(() => pasteImage(text)); return true; }, scroll() { capture(); scheduleRecovery(); return false; } }),
      EditorView.updateListener.of(update => { if (update.docChanged || update.selectionSet || update.startState.field(metadataField) !== update.state.field(metadataField)) { capture(); scheduleRecovery(); } })
    ]);
    view = new EditorView({ parent: host, state });
    view.dispatch({ effects: [wrapSlot.reconfigure(workspace.wrap ? EditorView.lineWrapping : []), editable.reconfigure(EditorView.editable.of(!frozen && !busy))], selection: { anchor: Math.min(buffer.anchor, state.doc.length), head: Math.min(buffer.head, state.doc.length) } });
    view.scrollDOM.scrollTop = buffer.scrollTop;
    scheduleRecovery();
  }
  function run(work: () => Promise<void> | void) {
    if (busy || frozen || loading || annotation || startupError) return;
    busy = true;
    view?.dispatch({ effects: editable.reconfigure(EditorView.editable.of(false)) });
    action = Promise.resolve().then(work).catch(error => { notice = errorMessage(error); }).finally(() => {
      busy = false;
      view?.dispatch({ effects: editable.reconfigure(EditorView.editable.of(!frozen)) });
    });
  }
  async function newFile() {
    if (workspace.buffers.length >= 1000) throw new Error('Close a tab before opening more than 1,000 buffers.');
    capture();
    const buffer = files.untitled();
    workspace = { ...workspace, buffers: [...workspace.buffers, buffer] };
    mount(buffer.id); await checkpoint();
  }
  async function openFile(path?: string, line?: number) {
    const selected = path ?? await files.picker('open');
    if (!selected) return;
    const disk = await files.readFile(selected);
    const existing = workspace.buffers.find(b => b.path?.toLowerCase() === disk.path.toLowerCase());
    if (!existing && workspace.buffers.length >= 1000) throw new Error('Close a tab before opening more than 1,000 buffers.');
    if (!existing) workspace = { ...workspace, buffers: [...workspace.buffers, files.opened(disk)] };
    mount(existing?.id ?? workspace.buffers.at(-1)!.id);
    if (line && view) { const target = view.state.doc.line(Math.min(line, view.state.doc.lines)); view.dispatch({ selection: { anchor: target.from }, scrollIntoView: true }); }
    await checkpoint();
  }
  function documentReferences(buffer: files.Buffer) {
    const language = files.languageFor(buffer.path);
    const state = referenceState(buffer.text,buffer.metadata,syntax(language));
    if (language !== 'plaintext' && !ensureSyntaxTree(state,state.doc.length,1000)) throw new Error('Source parsing is incomplete. Try saving again.');
    return sourceImages(state,language).map(image => ({ from: image.from, to: image.to, path: image.block.attachmentId }));
  }
  async function saveFile(id = workspace.active, saveAs = false): Promise<boolean> {
    capture();
    const buffer = workspace.buffers.find(b => b.id === id);
    if (!buffer) return false;
    // Recovery must succeed before a multi-file save; a partial disk failure remains recoverable.
    await checkpoint();
    let disk: files.DiskDocument;
    if (!buffer.path || saveAs || buffer.external === 'deleted') {
      const destination = await files.picker('save', buffer.path ? files.title(buffer) : 'Untitled.md');
      if (!destination) return false;
      const targetLanguage = files.languageFor(destination);
      const oldLanguage = files.languageFor(buffer.path);
      const refs = documentReferences(buffer);
      let source = buffer.text;
      // Explicit Save As may change syntax, but must never place raw image markers in JSON.
      if (refs.length && targetLanguage !== oldLanguage) {
        if (targetLanguage === 'json') throw new Error('JSON cannot contain image comments. Choose another file type.');
        if (targetLanguage !== 'markdown' && targetLanguage !== 'plaintext' && refs.some(ref => {
          const start = source.lastIndexOf('\n',ref.from-1)+1;
          const end = source.indexOf('\n',ref.to);
          return source.slice(start,ref.from).trim() || source.slice(ref.to,end < 0 ? source.length : end).trim();
        })) throw new Error('Put each image reference on its own line before saving as code.');
        for (const ref of [...refs].reverse()) source = source.slice(0, ref.from) + imageSyntax(ref.path, targetLanguage) + source.slice(ref.to);
      }
      const targetRefs = targetLanguage === oldLanguage ? refs : documentReferences({ ...buffer, path: destination, text: source });
      if (targetRefs.length < refs.length) throw new Error('Image comments are not valid in this file type or source context. Choose another type or adjust the source.');
      disk = await files.operation<files.DiskDocument>({ kind: 'package', destination, text: source, metadata: buffer.metadata, origin: buffer.path?.replace(/[/\\][^/\\]+$/, '') ?? null, assets: targetRefs });
    } else {
      if (buffer.external) throw new Error('Resolve the external change with Reload from Disk or Save As.');
      disk = await files.operation<files.DiskDocument>({ kind: 'save', path: buffer.path, expected: buffer.revision, text: buffer.text, metadata: buffer.metadata, assets: documentReferences(buffer) });
    }
    const reset = buffer.path !== disk.path || buffer.text !== disk.text || files.fingerprint(buffer.text,buffer.metadata) !== files.fingerprint(disk.text,disk.metadata);
    if (reset && viewId === buffer.id) { view?.destroy(); view = undefined; viewId = undefined; }
    if (reset) states.delete(buffer.id);
    patch(buffer.id, { ...disk, baseline: files.fingerprint(disk.text, disk.metadata), external: null });
    if (reset && workspace.active === buffer.id) mount(buffer.id);
    await checkpoint(); notice = 'Saved.'; return true;
  }
  async function closeFile(id: string, choice?: 'save' | 'discard') {
    capture();
    const buffer = workspace.buffers.find(b => b.id === id);
    if (!buffer) return;
    if (!choice && files.closeDecision(buffer) === 'prompt') { closeTarget = id; closeDialog.showModal(); return; }
    if (choice === 'save' && !await saveFile(id)) return;
    capture();
    const previous = workspace;
    if (viewId === id) unmount();
    workspace = files.removeBuffer(workspace, id);
    states.delete(id);
    closeDialog.close(); closeTarget = null;
    mount(workspace.active);
    try { await checkpoint(); }
    catch (error) { unmount(); workspace = previous; mount(previous.active); throw error; }
    closed.push(buffer.path ? { ...buffer, baseline: buffer.baseline } : files.untitled());
  }
  async function reopen() {
    const buffer = closed.at(-1);
    if (!buffer) return;
    if (buffer.path) await openFile(buffer.path); else await newFile();
    closed.pop();
  }
  async function refresh() {
    if (workspace.root) entries = await files.operation<files.Entry[]>({ kind: 'browse', root: workspace.root, query: panel === 'search' ? query : '' });
    else entries = [];
  }
  async function openFolder() { const root = await files.picker('folder'); if (root) { workspace = { ...workspace, root, sidebar: true }; await refresh(); await checkpoint(); } }
  async function showPanel(next: 'explorer' | 'search') { panel = next; workspace = { ...workspace, sidebar: true }; await refresh(); scheduleRecovery(); }
  function wrap() { workspace = { ...workspace, wrap: !workspace.wrap }; view?.dispatch({ effects: wrapSlot.reconfigure(workspace.wrap ? EditorView.lineWrapping : []) }); scheduleRecovery(); }
  async function openFind() { findVisible = true; await tick(); document.getElementById('find')?.focus(); }
  function find(event: SubmitEvent) { event.preventDefault(); if (view) { view.dispatch({ effects: setSearchQuery.of(new SearchQuery({ search: findQuery, literal: true })) }); findNext(view); } }
  async function pasteImage(fallback = '') {
    if (!view || !active) return;
    const target = view, buffer = active, language = files.languageFor(buffer.path);
    const bytes = await invoke<number[] | null>('file_clipboard');
    if (!bytes) { if (fallback) target.dispatch(target.state.replaceSelection(fallback)); return; }
    if (language !== 'plaintext' && !ensureSyntaxTree(target.state, target.state.doc.length, 1000)) throw new Error('Source parsing is incomplete. Try inserting again.');
    insertion(target.state, language, './assets/validation.png'); // Validate before writing an asset.
    // Always stage new originals in recovery. Save adopts them only after safe packaging/materialization.
    const reference = await files.operation<string>({ kind: 'putImage', document: null, bytes });
    const metadata = { ...target.state.field(metadataField), images: { ...target.state.field(metadataField).images, [reference]: { id: crypto.randomUUID(), annotations: [] } } };
    target.dispatch({ ...insertion(target.state, language, reference), effects: setMetadata.of(metadata) });
    await checkpoint();
  }
  async function openAnnotation(block: ImageBlock) {
    if (busy || frozen || !active) return;
    run(async () => { annotation = { block, blob: await imageBlob(active!, block.attachmentId) }; });
  }
  function annotate(annotations: readonly Annotation[]) {
    if (!view || !annotation) return;
    const metadata = view.state.field(metadataField), reference = annotation.block.attachmentId;
    view.dispatch({ effects: setMetadata.of({ ...metadata, images: { ...metadata.images, [reference]: { id: annotation.block.id, annotations } } }) });
  }
  async function outputAnnotation(action: 'copy' | 'export', annotations: readonly Annotation[], width: number, height: number) {
    if (!annotation) throw new Error('Image is unavailable');
    const blob = annotation.blob;
    outputWork = (async () => {
      const png = await flattenImage(blob, annotations, width, height);
      if (action === 'copy') { await storage.copyImage(png); return 'Image copied.'; }
      return await storage.exportImage(png) ? 'PNG exported.' : 'Export cancelled.';
    })();
    try { return await outputWork; } finally { outputWork = undefined; }
  }
  async function reloadDisk() {
    if (!active?.path) return;
    if (files.modified(active) && !window.confirm('Discard this buffer’s unsaved edits and reload from disk?')) return;
    const disk = await files.readFile(active.path), id = active.id;
    unmount(); states.delete(id); patch(id, { ...disk, baseline: files.fingerprint(disk.text, disk.metadata), external: null }); mount(id); await checkpoint();
  }
  async function poll() {
    if (polling || busy || frozen || loading || annotation || startupError) return;
    polling = true;
    try {
      capture();
      for (const buffer of [...workspace.buffers]) {
        if (!buffer.path) continue;
        let next: files.Buffer;
        try { next = files.reconcile(buffer, await files.readFile(buffer.path)); }
        catch (error) {
          // Only an actual not-found is deletion. Permission/metadata failures keep the buffer intact.
          next = { ...buffer, external: typeof error === 'object' && error !== null && 'code' in error && error.code === 'not_found' ? 'deleted' : 'unavailable' };
        }
        if (busy || frozen) break;
        if (next.text !== buffer.text || files.fingerprint(next.text,next.metadata) !== files.fingerprint(buffer.text,buffer.metadata)) {
          // Recheck after await: a new local generation always wins over a background reload.
          const current = workspace.buffers.find(b => b.id === buffer.id);
          capture();
          const latest = workspace.buffers.find(b => b.id === buffer.id);
          if (!current || !latest || files.modified(latest) || latest.revision !== buffer.revision) { if (latest) patch(buffer.id, { external: 'changed' }); continue; }
          const wasActive = viewId === buffer.id;
          if (wasActive) unmount();
          states.delete(buffer.id); patch(buffer.id, next); if (wasActive) mount(buffer.id);
        } else {
          capture();
          const latest = workspace.buffers.find(b => b.id === buffer.id);
          if (latest && latest.revision === buffer.revision) patch(buffer.id, { external: next.external,
            ...(!files.modified(latest) ? { revision: next.revision, baseline: next.baseline } : {}) });
        }
      }
      if (workspace.sidebar) await refresh();
      scheduleRecovery();
    } catch (error) { notice = errorMessage(error); }
    finally { polling = false; }
  }
  async function exit() {
    if (exitWork) return exitWork;
    frozen = true;
    view?.contentDOM.blur();
    view?.dispatch({ effects: editable.reconfigure(EditorView.editable.of(false)) });
    exitWork = (async () => {
      try {
        try { await boot; } catch (error) { if (started) throw error; }
        await action; await outputWork?.catch(() => {}); annotation = undefined;
        // A failed recovery load has admitted no edits. Exit without replacing the original recovery file.
        if (started) await checkpoint();
        await storage.finishExit(false);
      }
      catch (error) { notice = `Could not preserve work for exit: ${errorMessage(error)}. Keep editing and retry.`; await storage.cancelExit(); frozen = false; view?.dispatch({ effects: editable.reconfigure(EditorView.editable.of(true)) }); }
      finally { exitWork = undefined; }
    })();
    return exitWork;
  }
  function menu(event: MouseEvent, id: string) {
    const element = document.getElementById(id)!;
    const rect = (event.currentTarget as HTMLElement).getBoundingClientRect();
    element.style.left = `${rect.left}px`; element.style.top = `${rect.bottom}px`; element.showPopover();
    element.querySelector<HTMLButtonElement>('button')?.focus();
  }
  function command(work: () => Promise<void> | void) { document.querySelectorAll<HTMLElement>('.command-menu').forEach(m => m.hidePopover()); run(work); }
  function tabContext(event: MouseEvent, id: string) { event.preventDefault(); menuBuffer = id; tabMenu.style.left = `${Math.min(event.clientX,window.innerWidth-240)}px`; tabMenu.style.top = `${event.clientY}px`; tabMenu.showPopover(); }
  function alterTab(direction: 'pin' | -1 | 1) {
    const buffers = [...workspace.buffers], index = buffers.findIndex(b => b.id === menuBuffer);
    if (index < 0) return;
    if (direction === 'pin') buffers[index] = { ...buffers[index], pinned: !buffers[index].pinned };
    else if (buffers[index + direction]?.pinned === buffers[index].pinned) [buffers[index], buffers[index + direction]] = [buffers[index + direction], buffers[index]];
    buffers.sort((a,b) => Number(b.pinned)-Number(a.pinned)); workspace = { ...workspace, buffers }; scheduleRecovery();
  }
  function shortcut(event: KeyboardEvent) {
    if (annotation || closeDialog?.open || event.isComposing || event.altKey && event.key.toLowerCase() !== 'z') return;
    const key = event.key.toLowerCase();
    if ((event.ctrlKey || event.metaKey) && (key === 's' || key === 'o')) { event.preventDefault(); run(() => key === 's' ? saveFile(workspace.active,event.shiftKey).then(() => {}) : event.shiftKey ? openFolder() : openFile()); return; }
    const action = tabShortcut(event); if (!action) return; event.preventDefault();
    if (action === 'quit') { void storage.requestExit(); return; }
    run(async () => {
      if (action === 'new') await newFile(); else if (action === 'close' && workspace.active) await closeFile(workspace.active);
      else if (action === 'reopen') await reopen(); else if (action === 'wrap') wrap();
      else if (action === 'sidebar') { workspace = { ...workspace, sidebar: !workspace.sidebar }; await refresh(); scheduleRecovery(); }
      else if (action === 'explorer' || action === 'search') await showPanel(action);
      else if (workspace.buffers.length) { const index = workspace.buffers.findIndex(b => b.id === workspace.active); mount(workspace.buffers[(index+(action === 'previous' ? workspace.buffers.length-1 : 1))%workspace.buffers.length].id); }
    });
  }
  onMount(() => {
    let unlisten: (() => void) | undefined;
    boot = (async () => {
      unlisten = await listen('flush-before-exit', () => { void exit(); });
      if (disposed) { unlisten(); return; }
      if (await storage.exitReady()) void exit();
      const recovered = await files.operation<files.Workspace | null>({ kind: 'recover' });
      if (disposed) return;
      if (recovered) {
        workspace = files.recovered(recovered);
        for (const buffer of [...workspace.buffers]) {
          if (!buffer.path) continue;
          try { patch(buffer.id,files.reconcile(buffer,await files.readFile(buffer.path))); }
          catch (error) { patch(buffer.id,{ external: typeof error === 'object' && error !== null && 'code' in error && error.code === 'not_found' ? 'deleted' : 'unavailable' }); }
        }
      }
      started = true;
      mount(workspace.active);
      if (!recovered) await newFile();
      try { await refresh(); } catch (error) { notice = errorMessage(error); }
    })();
    void boot.catch(error => { startupError = errorMessage(error); }).finally(() => { loading = false; void poll(); });
    const interval = setInterval(() => { void poll(); }, 2000);
    window.addEventListener('keydown', shortcut);
    const focus = () => { void poll(); }; window.addEventListener('focus', focus);
    return () => { disposed = true; clearInterval(interval); clearTimeout(timer); unlisten?.(); window.removeEventListener('keydown',shortcut); window.removeEventListener('focus',focus); view?.destroy(); };
  });
</script>

<svelte:head><title>Simple Note</title></svelte:head>
<main class="flex h-dvh flex-col bg-[var(--bg-app)] text-sm text-[var(--text-primary)]" aria-busy={busy || loading || frozen}>
  <header class="app-header">
    <div class="app-brand"><img class="app-mark" src="/app-icon.png" alt=""/><span class="app-title">Simple Note</span></div>
    <nav class="app-menubar" aria-label="Application menus">
      {#each ['File','Edit','View'] as name}<button type="button" aria-haspopup="true" onclick={event => menu(event,`app-${name}`)}>{name}</button>{/each}
    </nav>
    <div class="header-center"><button class="header-search-pill" onclick={() => run(() => showPanel('search'))}>Search files… <kbd>Ctrl+Shift+F</kbd></button></div>
    <button class="header-btn" disabled={!active || busy || loading || frozen} onclick={() => run(() => saveFile().then(() => {}))}>Save <kbd>Ctrl+S</kbd></button>
    <div id="app-File" popover class="command-menu">
      <button onclick={() => command(newFile)}>New file <kbd>Ctrl+N</kbd></button>
      <button onclick={() => command(() => openFile())}>Open file… <kbd>Ctrl+O</kbd></button>
      <button onclick={() => command(openFolder)}>Open folder… <kbd>Ctrl+Shift+O</kbd></button>
      <button disabled={!active} onclick={() => command(() => saveFile().then(() => {}))}>Save <kbd>Ctrl+S</kbd></button>
      <button disabled={!active} onclick={() => command(() => saveFile(workspace.active,true).then(() => {}))}>Save As… <kbd>Ctrl+Shift+S</kbd></button>
      <div class="menu-separator"></div><button onclick={() => command(reopen)}>Reopen closed tab <kbd>Ctrl+Shift+T</kbd></button>
      <button onclick={() => { void storage.requestExit(); }}>Quit <kbd>Ctrl+Q</kbd></button>
    </div>
    <div id="app-Edit" popover class="command-menu">
      <button onclick={() => command(() => { if (view) undo(view); })}>Undo <kbd>Ctrl+Z</kbd></button>
      <button onclick={() => command(() => { if (view) redo(view); })}>Redo <kbd>Ctrl+Y</kbd></button>
      <button onclick={() => command(openFind)}>Find <kbd>Ctrl+F</kbd></button>
      <button onclick={() => command(() => pasteImage())}>Paste screenshot</button>
    </div>
    <div id="app-View" popover class="command-menu">
      <button onclick={() => command(() => showPanel('explorer'))}>Explorer <kbd>Ctrl+Shift+E</kbd></button>
      <button onclick={() => command(() => showPanel('search'))}>Search <kbd>Ctrl+Shift+F</kbd></button>
      <button onclick={() => command(() => { workspace = { ...workspace, sidebar: !workspace.sidebar }; scheduleRecovery(); })}>Toggle sidebar <kbd>Ctrl+B</kbd></button>
      <button onclick={() => command(wrap)}>Toggle word wrap <kbd>Alt+Z</kbd></button>
    </div>
  </header>
  {#if startupError}<div role="alert" class="p-3 bg-red-950">Could not load workspace: {startupError}. Recovery was preserved. Restart after resolving the error.</div>{/if}
  {#if recoveryError}<div role="alert" class="p-3 bg-red-950">Recovery failed: {recoveryError}. Your work remains open. <button onclick={() => run(() => checkpoint())}>Retry recovery</button></div>{/if}
  {#if active?.external}<div role="alert" class="p-3 bg-amber-950">{active.external === 'deleted' ? 'File deleted or renamed externally. This buffer is retained; use Save As to save elsewhere.' : active.external === 'changed' ? 'File changed externally. Your unsaved edits are retained.' : 'File is unreadable. Your buffer is retained.'} <button onclick={() => run(() => saveFile(workspace.active,true).then(() => {}))}>Save As…</button>{#if active.external !== 'deleted'}<button onclick={() => run(reloadDisk)}>Reload from disk</button>{/if}</div>{/if}
  <div class="workspace">
    <nav class="activity-bar" aria-label="Workspace views">
      <button title="Explorer (Ctrl+Shift+E)" aria-label="Explorer" aria-pressed={workspace.sidebar && panel === 'explorer'} onclick={() => run(() => showPanel('explorer'))}><svg viewBox="0 0 24 24" aria-hidden="true"><path d="M8 3h8l4 4v14H8zM16 3v5h4M5 6H3v15h2"/></svg></button>
      <button title="Search (Ctrl+Shift+F)" aria-label="Search" aria-pressed={workspace.sidebar && panel === 'search'} onclick={() => run(() => showPanel('search'))}><svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="10.5" cy="10.5" r="7.5"/><path d="m16 16 5 5"/></svg></button>
    </nav>
    {#if workspace.sidebar}<aside class="note-sidebar" aria-label={panel}>
      <div class="sidebar-header"><span class="sidebar-title">{panel.toUpperCase()}</span><button onclick={() => run(openFolder)}>Open folder…</button><button aria-label="Refresh folder" onclick={() => run(refresh)}>↻</button></div>
      <p class="px-3 text-xs break-all">{workspace.root ?? 'Open a folder to browse and search real files.'}</p>
      {#if panel === 'search'}<form class="library-search" onsubmit={event => { event.preventDefault(); run(refresh); }}><input aria-label="Search workspace text" bind:value={query} placeholder="Search files…"/><button>Search</button></form>{/if}
      {#each entries as entry (`${entry.path}:${entry.line ?? ''}`)}
        <button class="library-row" title={entry.path} disabled={entry.directory} style:padding-left={`${12 + (entry.relative.split('/').length-1)*12}px`} onclick={() => run(() => openFile(entry.path,entry.line))}><span>{entry.directory ? '▾' : '·'}</span><span class="file-name">{panel === 'search' ? `${entry.relative}:${entry.line} ${entry.preview}` : entry.relative.split('/').at(-1)}</span></button>
      {/each}
      {#if !entries.length}<p class="empty-library">No files or matches.</p>{/if}
    </aside>{/if}
    <div class="editor-column">
      <div class="tab-strip" role="tablist" aria-label="Open files">
        {#each workspace.buffers as buffer, index (buffer.id)}<div class="note-tab" class:active={workspace.active === buffer.id}>
          <button class="tab-label" role="tab" aria-selected={workspace.active === buffer.id} aria-controls="file-panel" tabindex={workspace.active === buffer.id ? 0 : -1} title={buffer.path ?? 'Unsaved file'} disabled={busy || frozen} oncontextmenu={event => tabContext(event,buffer.id)} onkeydown={event => { const target = event.key === 'ArrowRight' ? (index+1)%workspace.buffers.length : event.key === 'ArrowLeft' ? (index+workspace.buffers.length-1)%workspace.buffers.length : event.key === 'Home' ? 0 : event.key === 'End' ? workspace.buffers.length-1 : -1; if (target >= 0) { event.preventDefault(); run(() => mount(workspace.buffers[target].id)); } }} onclick={() => run(() => mount(buffer.id))}>{buffer.pinned ? '◆ ' : ''}{files.title(buffer)}{files.modified(buffer) ? ' ●' : ''}{buffer.external ? ' !' : ''}</button>
          <button class="tab-action" aria-label={`Actions for ${files.title(buffer)}`} onclick={event => tabContext(event,buffer.id)}>···</button>
          <button class="tab-action" aria-label={`Close ${files.title(buffer)}`} onclick={() => run(() => closeFile(buffer.id))}>×</button>
        </div>{/each}
        <button class="tab-new-btn" title="New file (Ctrl+N)" onclick={() => run(newFile)}>＋</button>
      </div>
      <div bind:this={tabMenu} popover class="command-menu" aria-label="Tab actions">
        <button onclick={() => command(() => alterTab('pin'))}>Pin / Unpin</button><button onclick={() => command(() => alterTab(-1))}>Move left</button><button onclick={() => command(() => alterTab(1))}>Move right</button><button onclick={() => command(() => closeFile(menuBuffer!))}>Close</button>
      </div>
      {#if findVisible}<form class="find-bar" onsubmit={find}><input id="find" aria-label="Find in file" bind:value={findQuery} placeholder="Find…" onkeydown={event => { if (event.key === 'Escape') { findVisible = false; view?.focus(); } }}/><button>Find next</button><button type="button" aria-label="Close find" onclick={() => { findVisible = false; view?.focus(); }}>×</button></form>{/if}
      <div id="file-panel" role="tabpanel" tabindex="0" aria-label="Active file" class="relative min-h-0 min-w-0 flex-1">
        <div class="h-full" class:hidden={!active} bind:this={host}></div>
        {#if !active && !loading}<div class="empty-workspace"><h1 class="empty-title">Simple Note</h1><p class="empty-desc">Local files. Inline images. Unsaved work recovered on reopening.</p><div class="empty-actions"><button onclick={() => run(newFile)}>New file</button><button onclick={() => run(() => openFile())}>Open file…</button><button onclick={() => run(openFolder)}>Open folder…</button></div></div>{/if}
      </div>
    </div>
  </div>
  <footer class="status-bar"><span role="status">{loading ? 'Loading…' : busy || frozen ? 'Working…' : active && files.modified(active) ? 'Modified' : 'Saved'}{recoveryError ? ' · Recovery failed' : ''}</span>{#if notice}<span role="status" class="status-notice">{notice}</span>{/if}<div class="status-tools"><button onclick={() => run(wrap)}>Wrap: {workspace.wrap ? 'On' : 'Off'}</button><span>{files.languageFor(active?.path ?? null)} · UTF-8</span></div></footer>
</main>
<dialog bind:this={closeDialog} class="settings-dialog" aria-labelledby="close-heading" oncancel={() => { closeTarget = null; }}>
  <h2 id="close-heading">Save changes before closing?</h2><p>Unsaved changes will be lost if you choose Don’t Save.</p>
  <div class="dialog-actions"><button disabled={busy} onclick={() => run(() => closeFile(closeTarget!,'save'))}>Save</button><button disabled={busy} onclick={() => run(() => closeFile(closeTarget!,'discard'))}>Don’t Save</button><button disabled={busy} onclick={() => { closeDialog.close(); closeTarget = null; }}>Cancel</button></div>
</dialog>
{#if annotation}<AnnotationEditor block={annotation.block} blob={annotation.blob} status={recoveryError || 'Changes retained in recovery until Save'} onchange={annotate} onoutput={outputAnnotation} onclose={() => { annotation = undefined; view?.focus(); }}/>{/if}

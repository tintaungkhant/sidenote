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
  let unsavedBuffers = $derived(workspace.buffers.filter(b => files.isUnsavedBuffer(b, workspace.root)));
  let host: HTMLDivElement;
  let view: EditorView | undefined;
  let viewId: string | undefined;
  const states = new Map<string, EditorState>();
  const wrapSlot = new Compartment();
  const editable = new Compartment();
  let loading = $state(true), busy = $state(false), frozen = $state(false), startupError = $state(''), notice = $state(''), recoveryError = $state('');
  let panel = $state<'explorer' | 'search'>('explorer'), query = $state(''), findQuery = $state(''), findVisible = $state(false);
  let explorerEntries = $state.raw<files.Entry[]>([]);
  let searchEntries = $state.raw<files.Entry[]>([]);
  let searchMatches = $derived(files.searchWorkspace(query, workspace.buffers, searchEntries, workspace.root));
  let searchTimer: ReturnType<typeof setTimeout> | undefined;
  let renamingId = $state<string | null>(null), renameValue = $state('');
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
    const language = files.bufferLanguage(buffer);
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
  function jumpToLine(line: number) {
    if (!view) return;
    const target = view.state.doc.line(Math.max(1, Math.min(line, view.state.doc.lines)));
    view.dispatch({ selection: { anchor: target.from }, scrollIntoView: true });
    view.focus();
  }
  async function newFile() {
    if (workspace.buffers.length >= 1000) throw new Error('Close a tab before opening more than 1,000 buffers.');
    capture();
    const buffer = files.untitled(files.nextUntitledName(workspace.buffers, explorerEntries));
    workspace = { ...workspace, buffers: [...workspace.buffers, buffer] };
    mount(buffer.id); await checkpoint();
  }
  async function openRootFile(path: string, line?: number) {
    if (!workspace.root || !files.withinRoot(path, workspace.root)) throw new Error('Only files inside the workspace root folder can be opened.');
    const openMatch = workspace.buffers.find(b => b.path && files.samePath(b.path, path));
    if (openMatch) {
      mount(openMatch.id);
      if (line) jumpToLine(line);
      await checkpoint();
      return;
    }
    const disk = await files.readFile(path);
    if (!files.withinRoot(disk.path, workspace.root)) throw new Error('File points outside the workspace root folder.');
    if (workspace.buffers.length >= 1000) throw new Error('Close a tab before opening more than 1,000 buffers.');
    workspace = { ...workspace, buffers: [...workspace.buffers, files.opened(disk)] };
    mount(workspace.buffers.at(-1)!.id);
    if (line) jumpToLine(line);
    await checkpoint();
  }
  function applyRootFolder(nextRoot: string, preserveId?: string) {
    capture();
    if (viewId) {
      const activeBuffer = workspace.buffers.find(b => b.id === viewId);
      if (activeBuffer && activeBuffer.id !== preserveId && activeBuffer.path && !files.withinRoot(activeBuffer.path, nextRoot) && !files.modified(activeBuffer)) {
        unmount();
      }
    }
    const retained: files.Buffer[] = [];
    for (const buffer of workspace.buffers) {
      const dropCleanOutside = Boolean(buffer.path && buffer.id !== preserveId && !files.withinRoot(buffer.path, nextRoot) && !files.modified(buffer));
      if (dropCleanOutside) {
        states.delete(buffer.id);
        if (renamingId === buffer.id) renamingId = null;
      } else {
        retained.push(buffer);
      }
    }
    const nextActive = retained.some(b => b.id === workspace.active) ? workspace.active : retained[0]?.id ?? null;
    workspace = { ...workspace, root: nextRoot, buffers: retained, active: nextActive };
    mount(nextActive);
  }
  async function setRootFolder() {
    const root = await files.picker('folder');
    if (!root) return;
    applyRootFolder(root);
    workspace = { ...workspace, sidebar: true };
    await refresh();
    await checkpoint();
    notice = `Root folder set: ${files.displayPath(root)}`;
  }
  function documentReferences(buffer: files.Buffer) {
    const language = files.bufferLanguage(buffer);
    const state = referenceState(buffer.text,buffer.metadata,syntax(language));
    if (language !== 'plaintext' && !ensureSyntaxTree(state,state.doc.length,1000)) throw new Error('Source parsing is incomplete. Try saving again.');
    return sourceImages(state,language).map(image => ({ from: image.from, to: image.to, path: image.block.attachmentId }));
  }
  async function saveFile(id = workspace.active, saveCopy = false): Promise<boolean> {
    capture();
    const buffer = workspace.buffers.find(b => b.id === id);
    if (!buffer) return false;
    let rootPath = workspace.root;
    if (!rootPath) {
      const picked = await files.picker('folder');
      if (!picked) return false;
      applyRootFolder(picked, buffer.id);
      rootPath = picked;
    }
    const current = workspace.buffers.find(b => b.id === id);
    if (!current) return false;
    // Recovery must succeed before a multi-file save; a partial disk failure remains recoverable.
    await checkpoint();
    let disk: files.DiskDocument;
    const createInRoot = saveCopy || !current.path || current.external === 'deleted' || !files.withinRoot(current.path, rootPath);
    if (createInRoot) {
      const targetName = files.preferredFileName(current);
      const targetPath = `${rootPath.replace(/[/\\]+$/, '')}/${targetName}`;
      const targetLanguage = files.languageFor(targetName);
      const oldLanguage = files.bufferLanguage(current);
      let refs = documentReferences(current);
      let source = current.text;
      if (!current.path && oldLanguage !== 'plaintext') {
        const legacy = references(source, 'plaintext').filter(
          r => Object.hasOwn(current.metadata.images, r.path) && !refs.some(existing => existing.from <= r.from && r.to <= existing.to)
        );
        if (legacy.length) {
          if (targetLanguage === 'json') throw new Error('JSON cannot contain image comments. Rename the file type first.');
          if (targetLanguage !== 'markdown' && legacy.some(ref => {
            const start = source.lastIndexOf('\n', ref.from - 1) + 1;
            const end = source.indexOf('\n', ref.to);
            return source.slice(start, ref.from).trim() || source.slice(ref.to, end < 0 ? source.length : end).trim();
          })) throw new Error('Put each image reference on its own line before saving as code.');
          for (const ref of [...legacy].sort((a, b) => b.from - a.from)) {
            source = source.slice(0, ref.from) + imageSyntax(ref.path, targetLanguage) + source.slice(ref.to);
          }
          refs = documentReferences({ ...current, path: targetPath, text: source });
        }
      }
      if (refs.length && targetLanguage !== oldLanguage) {
        if (targetLanguage === 'json') throw new Error('JSON cannot contain image comments. Rename the file type first.');
        if (targetLanguage !== 'markdown' && targetLanguage !== 'plaintext' && refs.some(ref => {
          const start = source.lastIndexOf('\n',ref.from-1)+1;
          const end = source.indexOf('\n',ref.to);
          return source.slice(start,ref.from).trim() || source.slice(ref.to,end < 0 ? source.length : end).trim();
        })) throw new Error('Put each image reference on its own line before saving as code.');
        for (const ref of [...refs].reverse()) source = source.slice(0, ref.from) + imageSyntax(ref.path, targetLanguage) + source.slice(ref.to);
      }
      const targetRefs = targetLanguage === oldLanguage ? refs : documentReferences({ ...current, path: targetPath, text: source });
      if (targetRefs.length < refs.length) throw new Error('Image comments are not valid in this file type or source context. Rename the file or adjust the source.');
      disk = await files.operation<files.DiskDocument>({
        kind: 'createInRoot',
        root: rootPath,
        preferred: targetName,
        text: source,
        metadata: current.metadata,
        origin: current.path?.replace(/[/\\][^/\\]+$/, '') ?? null,
        assets: targetRefs,
      });
    } else {
      if (current.external) throw new Error('Resolve the external change with Reload from Disk or Save copy in root.');
      disk = await files.operation<files.DiskDocument>({ kind: 'save', path: current.path!, expected: current.revision, text: current.text, metadata: current.metadata, assets: documentReferences(current) });
    }
    const languageChanged = files.bufferLanguage(current) !== files.languageFor(disk.path);
    const contentChanged = current.text !== disk.text || files.fingerprint(current.text, current.metadata) !== files.fingerprint(disk.text, disk.metadata);
    const reset = languageChanged || contentChanged;
    if (reset && viewId === current.id) { view?.destroy(); view = undefined; viewId = undefined; }
    if (reset) states.delete(current.id);
    patch(current.id, { ...disk, name: undefined, baseline: files.fingerprint(disk.text, disk.metadata), external: null });
    if (reset && workspace.active === current.id) mount(current.id);
    await refresh();
    await checkpoint();
    notice = `Saved ${files.title(workspace.buffers.find(b => b.id === current.id) ?? current)}.`;
    return true;
  }
  async function beginRename(id = workspace.active) {
    if (frozen || !id) return;
    const buffer = workspace.buffers.find(b => b.id === id);
    if (!buffer) return;
    renamingId = buffer.id;
    renameValue = buffer.path ? (buffer.path.split(/[/\\]/).at(-1) ?? '') : (buffer.name ?? 'Untitled-1');
    await tick();
    const input = document.getElementById('rename-input') as HTMLInputElement | null;
    input?.focus();
    input?.select();
  }
  function submitRename(event: SubmitEvent) {
    event.preventDefault();
    const id = renamingId;
    if (!id) return;
    run(async () => {
      capture();
      const buffer = workspace.buffers.find(b => b.id === id);
      if (!buffer) { renamingId = null; return; }
      const isRootFile = Boolean(buffer.path && workspace.root && files.withinRoot(buffer.path, workspace.root) && buffer.external !== 'deleted');
      if (isRootFile) {
        const currentName = buffer.path!.split(/[/\\]/).at(-1) ?? '';
        const dot = currentName.lastIndexOf('.');
        const fallbackExt = dot >= 0 && dot < currentName.length - 1 ? currentName.slice(dot + 1) : 'md';
        const valid = files.validateFileName(renameValue, fallbackExt);
        const disk = await files.operation<files.DiskDocument>({ kind: 'rename', root: workspace.root!, path: buffer.path!, name: valid });
        const oldLanguage = files.bufferLanguage(buffer);
        const newLanguage = files.languageFor(disk.path);
        const dirty = files.modified(buffer);
        if (oldLanguage !== newLanguage) {
          if (viewId === buffer.id) unmount();
          states.delete(buffer.id);
        }
        patch(buffer.id, {
          path: disk.path,
          revision: disk.revision,
          name: undefined,
          ...(dirty ? {} : { text: disk.text, metadata: disk.metadata, baseline: files.fingerprint(disk.text, disk.metadata), external: null }),
        });
        if (oldLanguage !== newLanguage && workspace.active === buffer.id) mount(buffer.id);
        renamingId = null;
        await refresh();
        await checkpoint();
        notice = `Renamed to ${disk.path.split(/[/\\]/).at(-1)}.`;
        view?.focus();
      } else {
        const valid = files.validateFileName(renameValue, 'md');
        const oldLanguage = files.bufferLanguage(buffer);
        const newLanguage = files.bufferLanguage({ path: null, name: valid });
        if (oldLanguage !== newLanguage) {
          if (viewId === buffer.id) unmount();
          states.delete(buffer.id);
        }
        patch(buffer.id, { name: valid });
        if (oldLanguage !== newLanguage && workspace.active === buffer.id) mount(buffer.id);
        renamingId = null;
        await checkpoint();
        notice = `Renamed to ${valid}.`;
        view?.focus();
      }
    });
  }
  async function closeFile(id: string, choice?: 'save' | 'discard') {
    capture();
    const buffer = workspace.buffers.find(b => b.id === id);
    if (!buffer) return;
    if (!choice && files.closeDecision(buffer) === 'prompt') { closeTarget = id; closeDialog.showModal(); return; }
    if (choice === 'save' && !await saveFile(id)) return;
    capture();
    const previous = workspace;
    if (renamingId === id) renamingId = null;
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
    if (buffer.path && workspace.root && files.withinRoot(buffer.path, workspace.root)) await openRootFile(buffer.path); else await newFile();
    closed.pop();
  }
  async function refresh() {
    capture();
    if (workspace.root) {
      explorerEntries = await files.operation<files.Entry[]>({ kind: 'browse', root: workspace.root, query: '' });
      searchEntries = panel === 'search' && query ? await files.operation<files.Entry[]>({ kind: 'browse', root: workspace.root, query }) : [];
    } else {
      explorerEntries = [];
      searchEntries = [];
    }
  }
  function onSearchInput() {
    capture();
    clearTimeout(searchTimer);
    if (!query || !workspace.root) { searchEntries = []; return; }
    const current = query;
    searchTimer = setTimeout(() => {
      if (disposed || busy || frozen || !workspace.root || query !== current || panel !== 'search') return;
      void files.operation<files.Entry[]>({ kind: 'browse', root: workspace.root, query: current })
        .then(result => { if (query === current) searchEntries = result; })
        .catch(error => { notice = errorMessage(error); });
    }, 120);
  }
  async function showPanel(next: 'explorer' | 'search') {
    panel = next;
    workspace = { ...workspace, sidebar: true };
    await refresh();
    scheduleRecovery();
    if (next === 'search') {
      await tick();
      document.getElementById('workspace-search')?.focus();
    }
  }
  function wrap() { workspace = { ...workspace, wrap: !workspace.wrap }; view?.dispatch({ effects: wrapSlot.reconfigure(workspace.wrap ? EditorView.lineWrapping : []) }); scheduleRecovery(); }
  async function openFind() { findVisible = true; await tick(); document.getElementById('find')?.focus(); }
  function find(event: SubmitEvent) { event.preventDefault(); if (view) { view.dispatch({ effects: setSearchQuery.of(new SearchQuery({ search: findQuery, literal: true })) }); findNext(view); } }
  async function pasteImage(fallback = '') {
    if (!view || !active) return;
    const target = view, buffer = active, language = files.bufferLanguage(buffer);
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
  function tabContext(event: MouseEvent, id: string) {
    event.preventDefault();
    if (busy || frozen) return;
    menuBuffer = id;
    const rect = (event.currentTarget as HTMLElement).getBoundingClientRect();
    const x = event.clientX || rect.left;
    const y = event.clientY || rect.bottom;
    tabMenu.style.left = `${Math.min(x, window.innerWidth - 240)}px`;
    tabMenu.style.top = `${y}px`;
    tabMenu.showPopover();
    tabMenu.querySelector<HTMLButtonElement>('button')?.focus();
  }
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
    if ((event.ctrlKey || event.metaKey) && (key === 's' || key === 'o')) { event.preventDefault(); run(() => key === 's' ? saveFile(workspace.active,event.shiftKey).then(() => {}) : setRootFolder()); return; }
    if (event.key === 'F2' && !event.ctrlKey && !event.metaKey && !event.altKey && !event.shiftKey && workspace.active && !renamingId && !busy && !frozen) { event.preventDefault(); run(() => beginRename(workspace.active)); return; }
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
        const used: Pick<files.Buffer, 'path' | 'name'>[] = workspace.buffers.filter(b => b.path || b.name);
        for (const buffer of [...workspace.buffers]) {
          if (!buffer.path && !buffer.name) {
            const name = files.nextUntitledName(used);
            used.push({ path: null, name });
            patch(buffer.id, { name });
          }
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
    return () => { disposed = true; clearInterval(interval); clearTimeout(timer); clearTimeout(searchTimer); unlisten?.(); window.removeEventListener('keydown',shortcut); window.removeEventListener('focus',focus); view?.destroy(); };
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
      <button onclick={() => command(setRootFolder)}>{workspace.root ? 'Change root folder…' : 'Set root folder…'} <kbd>Ctrl+O</kbd></button>
      <button disabled={!active} onclick={() => command(() => saveFile().then(() => {}))}>Save <kbd>Ctrl+S</kbd></button>
      <button disabled={!active} onclick={() => command(() => beginRename(workspace.active))}>Rename <kbd>F2</kbd></button>
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
  {#if active?.external}<div role="alert" class="p-3 bg-amber-950">{active.external === 'deleted' ? 'File deleted or renamed externally. This buffer is retained; Save writes a new copy in the root folder.' : active.external === 'changed' ? 'File changed externally. Your unsaved edits are retained.' : 'File is unreadable. Your buffer is retained.'} <button onclick={() => run(() => saveFile(workspace.active,true).then(() => {}))}>Save copy in root</button>{#if active.external !== 'deleted'}<button onclick={() => run(reloadDisk)}>Reload from disk</button>{/if}</div>{/if}
  <div class="workspace">
    <nav class="activity-bar" aria-label="Workspace views">
      <button title="Explorer (Ctrl+Shift+E)" aria-label="Explorer" aria-pressed={workspace.sidebar && panel === 'explorer'} onclick={() => run(() => showPanel('explorer'))}><svg viewBox="0 0 24 24" aria-hidden="true"><path d="M8 3h8l4 4v14H8zM16 3v5h4M5 6H3v15h2"/></svg></button>
      <button title="Search (Ctrl+Shift+F)" aria-label="Search" aria-pressed={workspace.sidebar && panel === 'search'} onclick={() => run(() => showPanel('search'))}><svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="10.5" cy="10.5" r="7.5"/><path d="m16 16 5 5"/></svg></button>
    </nav>
    {#if workspace.sidebar}<aside class="note-sidebar" aria-label={panel}>
      <div class="sidebar-header"><span class="sidebar-title">{panel.toUpperCase()}</span><div class="sidebar-actions"><button onclick={() => run(setRootFolder)}>{workspace.root ? 'Change root…' : 'Set root…'}</button><button class="sidebar-btn" aria-label="Refresh folder" title="Refresh" onclick={() => run(refresh)}>↻</button></div></div>
      <p class="sidebar-root-info">{workspace.root ? files.displayPath(workspace.root) : 'No root folder set — select one now or on first Save.'}</p>
      {#if panel === 'search'}
        <form class="library-search" onsubmit={event => { event.preventDefault(); run(refresh); }}>
          <input id="workspace-search" aria-label="Search workspace text" bind:value={query} oninput={onSearchInput} placeholder="Search unsaved & saved files…"/>
          <button>Search</button>
        </form>
        {#if !query}
          <p class="empty-library">Type to search unsaved tabs and root files.</p>
        {:else if !searchMatches.length}
          <p class="empty-library">No matches found.</p>
        {:else}
          {#each searchMatches as match (match.key)}
            <button
              class="library-row"
              title={`${match.path ? files.displayPath(match.path) : match.label}:${match.line}`}
              onclick={() => run(() => match.bufferId ? (mount(match.bufferId), jumpToLine(match.line)) : openRootFile(match.path!, match.line))}
            >
              <span>·</span>
              <span class="file-name">{match.label}:{match.line} {match.preview}</span>
              {#if match.unsaved}<span class="file-unsaved-tag">unsaved</span>{/if}
            </button>
          {/each}
        {/if}
      {:else}
        {#if unsavedBuffers.length}
          <div class="sidebar-section-label">Unsaved ({unsavedBuffers.length})</div>
          {#each unsavedBuffers as buffer (buffer.id)}
            <button
              class="library-row"
              class:current={workspace.active === buffer.id}
              title={buffer.path ? `${files.displayPath(buffer.path)} (outside root)` : `${files.title(buffer)} (Unsaved)`}
              onclick={() => run(() => { mount(buffer.id); view?.focus(); })}
            >
              <span>·</span>
              <span class="file-name">{files.title(buffer)}{files.modified(buffer) ? ' ●' : ''}</span>
              <span class="file-unsaved-tag">unsaved</span>
            </button>
          {/each}
        {/if}
        {#if workspace.root}
          {#if unsavedBuffers.length || explorerEntries.length}
            <div class="sidebar-section-label">Root files</div>
          {/if}
          {#each explorerEntries as entry (entry.path)}
            {@const openBuffer = workspace.buffers.find(b => b.path && files.samePath(b.path, entry.path))}
            <button
              class="library-row"
              class:current={Boolean(active?.path && files.samePath(active.path, entry.path))}
              title={files.displayPath(entry.path)}
              disabled={entry.directory}
              style:padding-left={`${12 + (entry.relative.split('/').length - 1) * 12}px`}
              onclick={() => run(() => openRootFile(entry.path))}
            >
              <span>{entry.directory ? '▾' : '·'}</span>
              <span class="file-name">{entry.relative.split('/').at(-1)}{openBuffer && files.modified(openBuffer) ? ' ●' : ''}</span>
            </button>
          {/each}
        {/if}
        {#if !unsavedBuffers.length && !explorerEntries.length}
          <p class="empty-library">{workspace.root ? 'No files yet.' : 'No unsaved or root files yet.'}</p>
        {/if}
      {/if}
    </aside>{/if}
    <div class="editor-column">
      <div class="tab-strip" role="tablist" aria-label="Open files">
        {#each workspace.buffers as buffer, index (buffer.id)}
          <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
          <div class="note-tab" class:active={workspace.active === buffer.id} onclick={event => { if (event.target === event.currentTarget) run(() => mount(buffer.id)); }} oncontextmenu={event => tabContext(event, buffer.id)}>
            <button class="tab-label" role="tab" aria-selected={workspace.active === buffer.id} aria-controls="file-panel" tabindex={workspace.active === buffer.id ? 0 : -1} title={buffer.path ? files.displayPath(buffer.path) : `${files.title(buffer)} (Unsaved)`} disabled={busy || frozen} onkeydown={event => { const target = event.key === 'ArrowRight' ? (index+1)%workspace.buffers.length : event.key === 'ArrowLeft' ? (index+workspace.buffers.length-1)%workspace.buffers.length : event.key === 'Home' ? 0 : event.key === 'End' ? workspace.buffers.length-1 : -1; if (target >= 0) { event.preventDefault(); run(() => mount(workspace.buffers[target].id)); } }} onclick={() => run(() => mount(buffer.id))}>{buffer.pinned ? '◆ ' : ''}{files.title(buffer)}{files.modified(buffer) ? ' ●' : ''}{buffer.external ? ' !' : ''}</button>
            <button class="tab-action" aria-label={`Close ${files.title(buffer)}`} onclick={() => run(() => closeFile(buffer.id))}>×</button>
          </div>
        {/each}
        <button class="tab-new-btn" title="New file (Ctrl+N)" onclick={() => run(newFile)}>＋</button>
      </div>
      <div bind:this={tabMenu} popover class="command-menu" aria-label="Tab actions">
        <button onclick={() => command(() => beginRename(menuBuffer!))}>Rename</button><button onclick={() => command(() => alterTab('pin'))}>Pin / Unpin</button><button onclick={() => command(() => alterTab(-1))}>Move left</button><button onclick={() => command(() => alterTab(1))}>Move right</button><button onclick={() => command(() => closeFile(menuBuffer!))}>Close</button>
      </div>
      {#if renamingId}<form class="rename-bar" onsubmit={submitRename}><label for="rename-input" class="text-xs text-[var(--text-secondary)]">Rename</label><input id="rename-input" aria-label="New file name" bind:value={renameValue} onkeydown={event => { if (event.key === 'Escape') { event.preventDefault(); renamingId = null; view?.focus(); } }}/><button>Apply</button><button type="button" onclick={() => { renamingId = null; view?.focus(); }}>Cancel</button></form>{/if}
      {#if findVisible}<form class="find-bar" onsubmit={find}><input id="find" aria-label="Find in file" bind:value={findQuery} placeholder="Find…" onkeydown={event => { if (event.key === 'Escape') { findVisible = false; view?.focus(); } }}/><button>Find next</button><button type="button" aria-label="Close find" onclick={() => { findVisible = false; view?.focus(); }}>×</button></form>{/if}
      <div id="file-panel" role="tabpanel" tabindex="0" aria-label="Active file" class="relative min-h-0 min-w-0 flex-1">
        <div class="h-full" class:hidden={!active} bind:this={host}></div>
        {#if !active && !loading}<div class="empty-workspace"><h1 class="empty-title">Simple Note</h1><p class="empty-desc">All files stay in your root folder. Unsaved tabs remain searchable and recovered until saved.</p><div class="empty-actions"><button onclick={() => run(newFile)}>New file</button><button onclick={() => run(setRootFolder)}>{workspace.root ? 'Change root folder…' : 'Set root folder…'}</button><button onclick={() => run(() => showPanel('explorer'))}>Explorer</button></div></div>{/if}
      </div>
    </div>
  </div>
  <footer class="status-bar"><span role="status">{loading ? 'Loading…' : busy || frozen ? 'Working…' : active && files.modified(active) ? 'Modified' : 'Saved'}{recoveryError ? ' · Recovery failed' : ''}</span>{#if notice}<span role="status" class="status-notice">{notice}</span>{/if}<div class="status-tools"><button onclick={() => run(wrap)}>Wrap: {workspace.wrap ? 'On' : 'Off'}</button><span>{active ? files.bufferLanguage(active) : 'markdown'} · UTF-8</span></div></footer>
</main>
<dialog bind:this={closeDialog} class="settings-dialog" aria-labelledby="close-heading" oncancel={() => { closeTarget = null; }}>
  <h2 id="close-heading">Save changes before closing?</h2><p>Unsaved changes will be lost if you choose Don’t Save.</p>
  <div class="dialog-actions"><button disabled={busy} onclick={() => run(() => closeFile(closeTarget!,'save'))}>Save</button><button disabled={busy} onclick={() => run(() => closeFile(closeTarget!,'discard'))}>Don’t Save</button><button disabled={busy} onclick={() => { closeDialog.close(); closeTarget = null; }}>Cancel</button></div>
</dialog>
{#if annotation}<AnnotationEditor block={annotation.block} blob={annotation.blob} status={recoveryError || 'Changes retained in recovery until Save'} onchange={annotate} onoutput={outputAnnotation} onclose={() => { annotation = undefined; view?.focus(); }}/>{/if}

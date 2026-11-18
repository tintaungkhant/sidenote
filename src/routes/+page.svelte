<script lang="ts">
  import '../app.css';
  import { onMount, tick } from 'svelte';
  import { Compartment, type EditorState } from '@codemirror/state';
  import { EditorView, drawSelection, highlightActiveLine, keymap, lineNumbers } from '@codemirror/view';
  import { defaultKeymap, historyKeymap, indentWithTab, undo, redo } from '@codemirror/commands';
  import { bracketMatching, indentOnInput, ensureSyntaxTree } from '@codemirror/language';
  import { SearchQuery, findNext, findPrevious, search, setSearchQuery } from '@codemirror/search';
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
  import * as vcs from '$lib/vcs';

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
  let panel = $state<'explorer' | 'search' | 'git'>('explorer'), query = $state(''), findQuery = $state(''), findVisible = $state(false);
  let explorerEntries = $state.raw<files.Entry[]>([]);
  let searchEntries = $state.raw<files.Entry[]>([]);
  let searchMatches = $derived(files.searchWorkspace(query, workspace.buffers, searchEntries, workspace.root));
  let searchGroups = $derived(files.groupSearchMatches(searchMatches));
  let collapsedGroups = $state.raw<Record<string, boolean>>({});
  let searchTimer: ReturnType<typeof setTimeout> | undefined;
  let gitAvailable = $state<boolean | null>(null);
  let gitBannerDismissed = $state(false);
  let vcsState = $state.raw<vcs.VcsStatus | null>(null);
  let sidebarCommitMsg = $state('Update note');
  let renamingTarget = $state<{ kind: 'buffer'; id: string } | { kind: 'path'; path: string } | null>(null), renameValue = $state('');
  let renameInitial = '', renameSubmitting = false;
  let closeTarget = $state<string | null>(null);
  let closeDialog: HTMLDialogElement;
  let deleteTarget = $state<{ path: string; name: string } | null>(null);
  let deleteDialog: HTMLDialogElement;
  let savePrompt = $state<{ id: string; saveCopy: boolean; closeAfter: boolean } | null>(null);
  let saveName = $state('');
  let saveError = $state('');
  let saveDialog: HTMLDialogElement;
  let commitTargetId = $state<string | null>(null);
  let commitMessage = $state('');
  let commitError = $state('');
  let commitDialog: HTMLDialogElement;
  let restoreTarget = $state<{ id: string; path: string; hash: string; shortHash: string } | null>(null);
  let restoreDialog: HTMLDialogElement;
  let diffView = $state.raw<{
    bufferId: string;
    path: string;
    label: string;
    hash: string | null;
    subject: string;
    mode: 'parent' | 'working';
    isInitial: boolean;
    summary: vcs.DiffSummary;
    metaChanged: boolean;
    assetsChanged: boolean;
  } | null>(null);
  let annotation = $state.raw<{ block: ImageBlock; blob: Blob } | undefined>();
  let closed: files.Buffer[] = [];
  let snapshotQueue: Promise<void> = Promise.resolve();
  let action: Promise<void> = Promise.resolve();
  let timer: ReturnType<typeof setTimeout> | undefined;
  let disposed = false, polling = false, started = false;
  let exitWork: Promise<void> | undefined;
  let boot: Promise<void> = Promise.resolve();
  let outputWork: Promise<string> | undefined;
  type MenuEntry = { label: string; shortcut?: string; disabled?: boolean; danger?: boolean; action: () => Promise<void> | void } | 'separator';
  let contextItems = $state.raw<MenuEntry[]>([]);
  let contextPopover: HTMLDivElement;

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
    if (diffView && diffView.bufferId !== id) diffView = null;
    workspace = { ...workspace, active: id };
    const buffer = workspace.buffers.find(b => b.id === id);
    if (workspace.sidebar && panel === 'git') void refreshVcs(buffer);
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
    syncEditorSearch();
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
  function activeSearchTerm(): string {
    if (findVisible) return findQuery;
    if (workspace.sidebar && panel === 'search') return query;
    return '';
  }
  function syncEditorSearch(jumpFrom?: number) {
    if (!view) return;
    const term = activeSearchTerm();
    const sq = new SearchQuery({ search: term, caseSensitive: false, literal: true });
    if (jumpFrom !== undefined && term) {
      const docText = view.state.doc.toString().toLowerCase();
      const needle = term.toLowerCase();
      let idx = docText.indexOf(needle, jumpFrom);
      if (idx < 0 && jumpFrom > 0) idx = docText.indexOf(needle, 0);
      if (idx >= 0) {
        view.dispatch({
          effects: setSearchQuery.of(sq),
          selection: { anchor: idx, head: idx + term.length },
          scrollIntoView: true,
        });
        return;
      }
    }
    view.dispatch({ effects: setSearchQuery.of(sq) });
  }
  function toggleSearchGroup(key: string) {
    collapsedGroups = { ...collapsedGroups, [key]: !collapsedGroups[key] };
  }
  function jumpToLine(line: number, term = activeSearchTerm()) {
    if (!view) return;
    const target = view.state.doc.line(Math.max(1, Math.min(line, view.state.doc.lines)));
    const sq = new SearchQuery({ search: term, caseSensitive: false, literal: true });
    if (term) {
      const col = target.text.toLowerCase().indexOf(term.toLowerCase());
      if (col >= 0) {
        view.dispatch({
          effects: setSearchQuery.of(sq),
          selection: { anchor: target.from + col, head: target.from + col + term.length },
          scrollIntoView: true,
        });
        view.focus();
        return;
      }
    }
    view.dispatch({ effects: setSearchQuery.of(sq), selection: { anchor: target.from }, scrollIntoView: true });
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
        if (renamingTarget?.kind === 'buffer' && renamingTarget.id === buffer.id) renamingTarget = null;
      } else {
        retained.push(buffer);
      }
    }
    if (renamingTarget?.kind === 'path' && !files.withinRoot(renamingTarget.path, nextRoot)) renamingTarget = null;
    const nextActive = retained.some(b => b.id === workspace.active) ? workspace.active : retained[0]?.id ?? null;
    workspace = { ...workspace, root: nextRoot, buffers: retained, active: nextActive };
    mount(nextActive);
  }
  async function migrateWorkspaceRoot(rootPath: string) {
    const migrated = await files.operation<Record<string, string>>({ kind: 'migrateRoot', root: rootPath });
    const pairs = Object.entries(migrated);
    if (!pairs.length) return;
    capture();
    for (const [oldPath, newPath] of pairs) {
      const match = workspace.buffers.find(b => b.path && files.samePath(b.path, oldPath));
      if (!match) continue;
      const wasDirty = files.modified(match);
      const disk = await files.readFile(newPath);
      if (!wasDirty) {
        patch(match.id, { ...disk, baseline: files.fingerprint(disk.text, disk.metadata), external: null });
        if (viewId === match.id && view) view.dispatch({ effects: setMetadata.of(disk.metadata) });
        else states.delete(match.id);
      } else {
        const withoutMain: files.Metadata = { version: disk.metadata.version, images: disk.metadata.images };
        const sameBase = files.fingerprint(disk.text, withoutMain) === match.baseline;
        const nextMeta = { ...match.metadata, ...(disk.metadata.main ? { main: disk.metadata.main } : {}) };
        patch(match.id, {
          path: disk.path,
          revision: disk.revision,
          metadata: nextMeta,
          baseline: files.fingerprint(disk.text, disk.metadata),
          external: sameBase ? null : 'changed',
        });
        if (viewId === match.id && view) view.dispatch({ effects: setMetadata.of(nextMeta) });
        else if (states.has(match.id)) states.set(match.id, states.get(match.id)!.update({ effects: setMetadata.of(nextMeta) }).state);
      }
    }
  }
  async function setRootFolder() {
    const root = await files.picker('folder');
    if (!root) return;
    applyRootFolder(root);
    await migrateWorkspaceRoot(root);
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
  async function focusSaveInput() {
    await tick();
    const input = document.getElementById('save-name-input') as HTMLInputElement | null;
    if (!input) return;
    input.focus();
    const dot = saveName.lastIndexOf('.');
    input.setSelectionRange(0, dot > 0 ? dot : saveName.length);
  }
  async function commitBufferSave(
    current: files.Buffer,
    rootPath: string,
    createOpts?: { targetName: string; exact: boolean },
  ): Promise<void> {
    // Recovery must succeed before a multi-file save; a partial disk failure remains recoverable.
    await checkpoint();
    let disk: files.DiskDocument;
    if (createOpts) {
      const targetName = createOpts.targetName;
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
        exact: createOpts.exact,
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
  }
  async function saveFile(id = workspace.active, saveCopy = false, closeAfter = false): Promise<boolean> {
    capture();
    const buffer = workspace.buffers.find(b => b.id === id);
    if (!buffer) return false;
    if (renamingTarget) renamingTarget = null;
    let rootPath = workspace.root;
    if (!rootPath) {
      const picked = await files.picker('folder');
      if (!picked) return false;
      applyRootFolder(picked, buffer.id);
      await migrateWorkspaceRoot(picked);
      await refresh();
      rootPath = picked;
    } else if (!workspace.sidebar) {
      explorerEntries = await files.operation<files.Entry[]>({ kind: 'browse', root: rootPath, query: '' });
    }
    const current = workspace.buffers.find(b => b.id === id);
    if (!current) return false;
    const createInRoot = saveCopy || !current.path || current.external === 'deleted' || !files.withinRoot(current.path, rootPath);
    if (!createInRoot) {
      await commitBufferSave(current, rootPath);
      return true;
    }
    if (closeDialog?.open) { closeDialog.close(); closeTarget = null; }
    const initial = files.suggestSaveName(current, workspace.buffers, explorerEntries);
    savePrompt = { id: current.id, saveCopy, closeAfter };
    saveName = initial;
    saveError = '';
    saveDialog.showModal();
    await focusSaveInput();
    return false;
  }
  function cancelSavePrompt() {
    if (busy) return;
    saveDialog.close();
    savePrompt = null;
    saveError = '';
    view?.focus();
  }
  function confirmSavePrompt() {
    if (!savePrompt || busy || frozen) return;
    const prompt = savePrompt;
    run(async () => {
      try {
        capture();
        const current = workspace.buffers.find(b => b.id === prompt.id);
        const rootPath = workspace.root;
        if (!current || !rootPath) {
          if (saveDialog?.open) saveDialog.close();
          savePrompt = null;
          saveError = '';
          return;
        }
        const prev = current.path?.split(/[/\\]/).at(-1) ?? current.name ?? '';
        const dot = prev.lastIndexOf('.');
        const fallbackExt = dot > 0 && dot < prev.length - 1 ? prev.slice(dot + 1) : 'md';
        const targetName = files.validateFileName(saveName, fallbackExt);
        await commitBufferSave(current, rootPath, { targetName, exact: true });
        if (saveDialog?.open) saveDialog.close();
        savePrompt = null;
        saveError = '';
        if (prompt.closeAfter) await removeOpenBuffer(prompt.id);
        else view?.focus();
      } catch (error) {
        saveError = errorMessage(error);
      }
    });
    void action.then(() => { if (savePrompt) void focusSaveInput(); });
  }
  async function refreshVcs(target = active) {
    if (!target?.path || !workspace.root || !files.withinRoot(target.path, workspace.root) || target.external === 'deleted') {
      vcsState = null;
      return;
    }
    try {
      const next = await vcs.vcsStatus(workspace.root, target.path);
      if (active?.id !== target.id && (!active?.path || !files.samePath(active.path, target.path))) return;
      gitAvailable = next.available;
      if (!next.available) gitBannerDismissed = false;
      vcsState = next;
      if (!sidebarCommitMsg || sidebarCommitMsg === 'Initial commit' || sidebarCommitMsg === 'Update note') {
        sidebarCommitMsg = vcs.defaultCommitMessage(Boolean(next.initialized));
      }
    } catch (error) {
      notice = errorMessage(error);
    }
  }
  async function focusCommitInput() {
    await tick();
    const input = document.getElementById('commit-message-input') as HTMLInputElement | null;
    if (!input) return;
    input.focus();
    input.select();
  }
  async function beginCommit(id = workspace.active) {
    if (!id || busy || frozen) return;
    capture();
    const buffer = workspace.buffers.find(b => b.id === id);
    if (!buffer) return;
    const check = await vcs.checkGit();
    gitAvailable = check.available;
    if (!check.available) {
      gitBannerDismissed = false;
      throw new Error('Git is not installed. Please install Git (https://git-scm.com) first.');
    }
    if (!workspace.root || files.isUnsavedBuffer(buffer, workspace.root)) {
      notice = 'Save this note in the root folder before committing.';
      await saveFile(buffer.id);
      return;
    }
    const status = await vcs.vcsStatus(workspace.root, buffer.path!);
    if (!status.available) {
      gitAvailable = false;
      gitBannerDismissed = false;
      throw new Error('Git is not installed. Please install Git (https://git-scm.com) first.');
    }
    if (!status.supported) throw new Error('Version control is available for saved note folders in the root.');
    if (active && active.id === buffer.id) vcsState = status;
    if (status.initialized && !status.dirty && !files.modified(buffer)) {
      notice = 'No changes to commit.';
      return;
    }
    commitTargetId = buffer.id;
    commitMessage = vcs.defaultCommitMessage(status.initialized);
    commitError = '';
    commitDialog.showModal();
    await focusCommitInput();
  }
  async function beginCommitPath(path: string) {
    const open = workspace.buffers.find(b => b.path && files.samePath(b.path, path));
    if (open) { await beginCommit(open.id); return; }
    await openRootFile(path);
    const opened = workspace.buffers.find(b => b.path && files.samePath(b.path, path));
    if (opened) await beginCommit(opened.id);
  }
  function cancelCommitDialog() {
    if (busy) return;
    commitDialog.close();
    commitTargetId = null;
    commitError = '';
    view?.focus();
  }
  function confirmCommitDialog() {
    if (!commitTargetId || busy || frozen) return;
    const targetId = commitTargetId;
    run(async () => {
      try {
        capture();
        const current = workspace.buffers.find(b => b.id === targetId);
        const rootPath = workspace.root;
        if (!current || !rootPath || !current.path) {
          if (commitDialog?.open) commitDialog.close();
          commitTargetId = null;
          commitError = '';
          return;
        }
        if (files.modified(current)) await commitBufferSave(current, rootPath);
        const latest = workspace.buffers.find(b => b.id === targetId);
        if (!latest?.path) throw new Error('Note is not saved on disk.');
        const nextStatus = await vcs.vcsCommit(rootPath, latest.path, commitMessage);
        if (active && latest.id === active.id) vcsState = nextStatus;
        if (commitDialog?.open) commitDialog.close();
        commitTargetId = null;
        commitError = '';
        if (diffView && diffView.bufferId === latest.id && diffView.hash === null) diffView = null;
        sidebarCommitMsg = 'Update note';
        notice = `Committed: ${nextStatus.commits[0]?.subject ?? 'Saved snapshot'}`;
        view?.focus();
      } catch (error) {
        commitError = errorMessage(error);
      }
    });
    void action.then(() => { if (commitTargetId) void focusCommitInput(); });
  }
  async function commitFromSidebar() {
    if (!active || !workspace.root || files.isUnsavedBuffer(active, workspace.root)) return;
    capture();
    const targetId = active.id;
    try {
      if (files.modified(active)) await commitBufferSave(active, workspace.root);
      const latest = workspace.buffers.find(b => b.id === targetId);
      if (!latest?.path || !workspace.root) return;
      const nextStatus = await vcs.vcsCommit(workspace.root, latest.path, sidebarCommitMsg);
      if (active?.id === latest.id) vcsState = nextStatus;
      sidebarCommitMsg = 'Update note';
      if (diffView && diffView.bufferId === latest.id && diffView.hash === null) diffView = null;
      notice = `Committed: ${nextStatus.commits[0]?.subject ?? 'Saved snapshot'}`;
    } catch (error) {
      if (typeof error === 'object' && error !== null && 'code' in error && error.code === 'git_missing') {
        gitAvailable = false;
        gitBannerDismissed = false;
      }
      throw error;
    }
  }
  async function openDiff(hash: string | null, subject: string, mode: 'parent' | 'working' = 'parent') {
    capture();
    const target = active;
    if (!target?.path || !workspace.root || !files.withinRoot(target.path, workspace.root)) return;
    try {
      const effectiveMode = hash === null ? 'working' : mode;
      const payload = await vcs.vcsDiff(workspace.root, target.path, hash, effectiveMode);
      const dirtyWorking = effectiveMode === 'working' && files.modified(target);
      const newText = dirtyWorking ? target.text : payload.newText;
      const hasPendingAssets = dirtyWorking && Object.keys(target.metadata.images).some(k => k.startsWith('recovery:'));
      const metaEditedInRam = dirtyWorking && (hasPendingAssets || target.text === payload.newText);
      diffView = {
        bufferId: target.id,
        path: target.path,
        label: files.title(target),
        hash,
        subject,
        mode: effectiveMode,
        isInitial: payload.isInitial,
        summary: vcs.diffLines(payload.oldText, newText),
        metaChanged: payload.metaChanged || metaEditedInRam,
        assetsChanged: payload.assetsChanged || hasPendingAssets,
      };
    } catch (error) {
      if (typeof error === 'object' && error !== null && 'code' in error && error.code === 'git_missing') {
        gitAvailable = false;
        gitBannerDismissed = false;
      }
      throw error;
    }
  }
  function closeDiff() {
    diffView = null;
    view?.focus();
  }
  function promptRestore(commit: Pick<vcs.CommitEntry, 'hash' | 'shortHash'>) {
    if (!active?.path || !workspace.root || !files.withinRoot(active.path, workspace.root)) return;
    restoreTarget = { id: active.id, path: active.path, hash: commit.hash, shortHash: commit.shortHash };
    restoreDialog.showModal();
  }
  async function confirmRestore() {
    const target = restoreTarget;
    if (!target || !workspace.root) { restoreDialog.close(); restoreTarget = null; return; }
    const disk = await vcs.vcsRestore(workspace.root, target.path, target.hash);
    restoreDialog.close();
    restoreTarget = null;
    if (viewId === target.id) unmount();
    states.delete(target.id);
    patch(target.id, { ...disk, baseline: files.fingerprint(disk.text, disk.metadata), external: null });
    if (workspace.active === target.id) mount(target.id);
    diffView = null;
    await refresh();
    await refreshVcs();
    await checkpoint();
    notice = `Restored ${target.shortHash}.`;
    view?.focus();
  }
  function commitContext(event: MouseEvent, commit: vcs.CommitEntry) {
    event.preventDefault();
    event.stopPropagation();
    const items: MenuEntry[] = [
      { label: 'View changes', action: () => openDiff(commit.hash, `${commit.shortHash}: ${commit.subject}`, 'parent') },
      { label: 'Compare with current', action: () => openDiff(commit.hash, `${commit.shortHash} ↔ Current`, 'working') },
      'separator',
      { label: 'Restore this version', action: () => promptRestore(commit) },
      {
        label: 'Copy commit hash',
        action: async () => {
          await invoke<void>('clipboard_write_text', { text: commit.hash });
          notice = `Copied ${commit.shortHash}.`;
        },
      },
    ];
    void showContextMenu(event.clientX, event.clientY, items);
  }
  async function ensureExplorerVisible() {
    if (!workspace.sidebar || panel !== 'explorer') {
      panel = 'explorer';
      workspace = { ...workspace, sidebar: true };
      await refresh();
      scheduleRecovery();
    }
  }
  async function focusRenameInput() {
    await tick();
    const input = document.getElementById('rename-input') as HTMLInputElement | null;
    if (!input) return;
    input.scrollIntoView({ block: 'nearest' });
    input.focus();
    const dot = renameValue.lastIndexOf('.');
    input.setSelectionRange(0, dot > 0 ? dot : renameValue.length);
  }
  async function beginRename(id = workspace.active) {
    if (frozen || !id) return;
    const buffer = workspace.buffers.find(b => b.id === id);
    if (!buffer) return;
    await ensureExplorerVisible();
    const latest = workspace.buffers.find(b => b.id === id) ?? buffer;
    const initial = latest.path ? (latest.path.split(/[/\\]/).at(-1) ?? '') : (latest.name ?? 'Untitled-1');
    renamingTarget = { kind: 'buffer', id: latest.id };
    renameValue = initial;
    renameInitial = initial;
    await focusRenameInput();
  }
  async function beginRenamePath(path: string) {
    if (frozen || !path || !workspace.root || !files.withinRoot(path, workspace.root)) return;
    const open = workspace.buffers.find(b => b.path && files.samePath(b.path, path));
    if (open) { await beginRename(open.id); return; }
    await ensureExplorerVisible();
    const initial = path.split(/[/\\]/).at(-1) ?? '';
    renamingTarget = { kind: 'path', path };
    renameValue = initial;
    renameInitial = initial;
    await focusRenameInput();
  }
  function cancelRename() {
    renamingTarget = null;
    view?.focus();
  }
  function commitRename(source: 'enter' | 'blur') {
    if (!renamingTarget || renameSubmitting || busy || frozen) return;
    const target = renamingTarget;
    const raw = renameValue.trim();
    if (!raw || raw === renameInitial) {
      renamingTarget = null;
      if (source === 'enter') view?.focus();
      return;
    }
    renameSubmitting = true;
    run(async () => {
      try {
        capture();
        if (target.kind === 'path') {
          const open = workspace.buffers.find(b => b.path && files.samePath(b.path, target.path));
          if (!open) {
            if (!workspace.root || !files.withinRoot(target.path, workspace.root)) { renamingTarget = null; return; }
            const currentName = target.path.split(/[/\\]/).at(-1) ?? '';
            const dot = currentName.lastIndexOf('.');
            const fallbackExt = dot >= 0 && dot < currentName.length - 1 ? currentName.slice(dot + 1) : 'md';
            const valid = files.validateFileName(renameValue, fallbackExt);
            const disk = await files.operation<files.DiskDocument>({ kind: 'rename', root: workspace.root, path: target.path, name: valid });
            renamingTarget = null;
            await refresh();
            notice = `Renamed to ${disk.path.split(/[/\\]/).at(-1)}.`;
            view?.focus();
            return;
          }
        }
        const id = target.kind === 'buffer'
          ? target.id
          : workspace.buffers.find(b => b.path && files.samePath(b.path, target.path))?.id;
        const buffer = id ? workspace.buffers.find(b => b.id === id) : undefined;
        if (!buffer) { renamingTarget = null; return; }
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
          const nextMetadata = dirty
            ? { ...buffer.metadata, ...(disk.metadata.main ? { main: disk.metadata.main } : {}) }
            : disk.metadata;
          if (oldLanguage !== newLanguage) {
            if (viewId === buffer.id) unmount();
            states.delete(buffer.id);
          }
          patch(buffer.id, {
            path: disk.path,
            revision: disk.revision,
            name: undefined,
            metadata: nextMetadata,
            ...(dirty ? {} : { text: disk.text, baseline: files.fingerprint(disk.text, disk.metadata), external: null }),
          });
          if (oldLanguage === newLanguage) {
            if (viewId === buffer.id && view) view.dispatch({ effects: setMetadata.of(nextMetadata) });
            else if (states.has(buffer.id)) states.set(buffer.id, states.get(buffer.id)!.update({ effects: setMetadata.of(nextMetadata) }).state);
          }
          if (oldLanguage !== newLanguage && workspace.active === buffer.id) mount(buffer.id);
          renamingTarget = null;
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
          renamingTarget = null;
          await checkpoint();
          notice = `Renamed to ${valid}.`;
          view?.focus();
        }
      } catch (error) {
        if (source === 'blur') renamingTarget = null;
        else void focusRenameInput();
        throw error;
      } finally {
        renameSubmitting = false;
      }
    });
  }
  function onRenameKeydown(event: KeyboardEvent) {
    if (event.isComposing) return;
    if (event.key === 'Escape') {
      event.preventDefault();
      event.stopPropagation();
      cancelRename();
    } else if (event.key === 'Enter') {
      event.preventDefault();
      event.stopPropagation();
      commitRename('enter');
    }
  }
  function onRenameBlur(event: FocusEvent) {
    if (!renamingTarget || renameSubmitting) return;
    if ((event.relatedTarget as HTMLElement | null)?.closest('.command-menu')) return;
    commitRename('blur');
  }
  function isRenamingEntry(entry: files.Entry, openBuffer?: files.Buffer): boolean {
    if (!renamingTarget || entry.directory) return false;
    if (renamingTarget.kind === 'path') return files.samePath(renamingTarget.path, entry.path);
    return Boolean(openBuffer && !files.isUnsavedBuffer(openBuffer, workspace.root) && renamingTarget.id === openBuffer.id);
  }
  async function removeOpenBuffer(id: string) {
    capture();
    const buffer = workspace.buffers.find(b => b.id === id);
    if (!buffer) return;
    const previous = workspace;
    if (renamingTarget?.kind === 'buffer' && renamingTarget.id === id) renamingTarget = null;
    if (diffView && diffView.bufferId === id) diffView = null;
    if (viewId === id) unmount();
    workspace = files.removeBuffer(workspace, id);
    states.delete(id);
    if (closeDialog?.open) closeDialog.close();
    closeTarget = null;
    mount(workspace.active);
    try { await checkpoint(); }
    catch (error) { unmount(); workspace = previous; mount(previous.active); throw error; }
    closed.push(buffer.path ? { ...buffer, baseline: buffer.baseline } : files.untitled());
  }
  async function closeFile(id: string, choice?: 'save' | 'discard') {
    capture();
    const buffer = workspace.buffers.find(b => b.id === id);
    if (!buffer) return;
    if (!choice && files.closeDecision(buffer) === 'prompt') { closeTarget = id; closeDialog.showModal(); return; }
    if (choice === 'save' && !await saveFile(id, false, true)) return;
    await removeOpenBuffer(id);
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
      if (workspace.sidebar && panel === 'git') await refreshVcs();
    } else {
      explorerEntries = [];
      searchEntries = [];
      vcsState = null;
    }
  }
  function onSearchInput() {
    capture();
    clearTimeout(searchTimer);
    syncEditorSearch();
    if (!query || !workspace.root) { searchEntries = []; return; }
    const current = query;
    searchTimer = setTimeout(() => {
      if (disposed || busy || frozen || !workspace.root || query !== current || panel !== 'search') return;
      void files.operation<files.Entry[]>({ kind: 'browse', root: workspace.root, query: current })
        .then(result => { if (query === current) searchEntries = result; })
        .catch(error => { notice = errorMessage(error); });
    }, 120);
  }
  async function showPanel(next: 'explorer' | 'search' | 'git') {
    panel = next;
    workspace = { ...workspace, sidebar: true };
    syncEditorSearch();
    await refresh();
    if (next === 'git') await refreshVcs();
    scheduleRecovery();
    if (next === 'search') {
      await tick();
      document.getElementById('workspace-search')?.focus();
    }
  }
  function wrap() { workspace = { ...workspace, wrap: !workspace.wrap }; view?.dispatch({ effects: wrapSlot.reconfigure(workspace.wrap ? EditorView.lineWrapping : []) }); scheduleRecovery(); }
  async function openFind() {
    findVisible = true;
    syncEditorSearch();
    await tick();
    const input = document.getElementById('find') as HTMLInputElement | null;
    input?.focus();
    input?.select();
  }
  function onFindInput() {
    if (!view) return;
    const from = Math.min(view.state.selection.main.from, view.state.selection.main.to);
    syncEditorSearch(from);
  }
  function stepFind(backwards = false) {
    if (!view || !findQuery) return;
    view.dispatch({ effects: setSearchQuery.of(new SearchQuery({ search: findQuery, caseSensitive: false, literal: true })) });
    if (backwards) findPrevious(view);
    else findNext(view);
  }
  function find(event: SubmitEvent) {
    event.preventDefault();
    stepFind(false);
  }
  function closeFind() {
    findVisible = false;
    syncEditorSearch();
    view?.focus();
  }
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
    if (polling || busy || frozen || loading || annotation || renamingTarget || savePrompt || commitTargetId || restoreTarget || startupError) return;
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
    if (saveDialog?.open) saveDialog.close();
    savePrompt = null;
    if (commitDialog?.open) commitDialog.close();
    commitTargetId = null;
    if (restoreDialog?.open) restoreDialog.close();
    restoreTarget = null;
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
      catch (error) { notice = `Exit failed: ${errorMessage(error)}`; await storage.cancelExit(); frozen = false; view?.dispatch({ effects: editable.reconfigure(EditorView.editable.of(true)) }); }
      finally { exitWork = undefined; }
    })();
    return exitWork;
  }
  function menu(event: MouseEvent, id: string) {
    document.querySelectorAll<HTMLElement>('.command-menu').forEach(m => m.hidePopover());
    const element = document.getElementById(id)!;
    const rect = (event.currentTarget as HTMLElement).getBoundingClientRect();
    element.style.left = `${rect.left}px`; element.style.top = `${rect.bottom}px`; element.showPopover();
    element.querySelector<HTMLButtonElement>('button')?.focus();
  }
  function command(work: () => Promise<void> | void) { document.querySelectorAll<HTMLElement>('.command-menu').forEach(m => m.hidePopover()); run(work); }
  async function showContextMenu(x: number, y: number, items: MenuEntry[]) {
    document.querySelectorAll<HTMLElement>('.command-menu').forEach(m => m.hidePopover());
    if (!items.length || busy || frozen || loading || annotation || closeDialog?.open || deleteDialog?.open || saveDialog?.open || commitDialog?.open || restoreDialog?.open) return;
    contextItems = items;
    await tick();
    if (!contextPopover) return;
    contextPopover.style.left = '0px';
    contextPopover.style.top = '0px';
    contextPopover.showPopover();
    const rect = contextPopover.getBoundingClientRect();
    contextPopover.style.left = `${Math.max(4, Math.min(x, window.innerWidth - rect.width - 6))}px`;
    contextPopover.style.top = `${Math.max(4, Math.min(y, window.innerHeight - rect.height - 6))}px`;
    contextPopover.querySelector<HTMLButtonElement>('button:not(:disabled)')?.focus();
  }
  async function revealPath(path: string) {
    await files.operation<void>({ kind: 'reveal', path });
  }
  async function copyPath(path: string) {
    await invoke<void>('clipboard_write_text', { text: files.displayPath(path) });
    notice = 'Copied path.';
  }
  async function closeOthers(keepId: string) {
    mount(keepId);
    for (const buffer of [...workspace.buffers]) {
      if (buffer.id === keepId || buffer.pinned) continue;
      if (files.closeDecision(buffer) === 'prompt') {
        mount(buffer.id);
        await closeFile(buffer.id);
        return;
      }
      await closeFile(buffer.id, 'discard');
    }
  }
  function promptDelete(path: string, name: string) {
    if (!workspace.root || !files.withinRoot(path, workspace.root)) return;
    deleteTarget = { path, name };
    deleteDialog.showModal();
  }
  async function confirmDelete() {
    const target = deleteTarget;
    if (!target || !workspace.root) { deleteDialog.close(); deleteTarget = null; return; }
    await files.operation<void>({ kind: 'delete', root: workspace.root, path: target.path });
    deleteDialog.close();
    deleteTarget = null;
    capture();
    for (const buffer of [...workspace.buffers]) {
      if (buffer.path && files.samePath(buffer.path, target.path)) {
        if (viewId === buffer.id) unmount();
        workspace = files.removeBuffer(workspace, buffer.id);
        states.delete(buffer.id);
      }
    }
    if (renamingTarget?.kind === 'buffer') {
      const targetId = renamingTarget.id;
      if (!workspace.buffers.some(b => b.id === targetId)) renamingTarget = null;
    }
    if (renamingTarget?.kind === 'path' && files.samePath(renamingTarget.path, target.path)) renamingTarget = null;
    if (diffView && files.samePath(diffView.path, target.path)) diffView = null;
    closed = closed.filter(b => !(b.path && files.samePath(b.path, target.path)));
    mount(workspace.active);
    await refresh();
    await checkpoint();
    notice = `Deleted ${target.name}.`;
  }
  function alterTab(id: string, direction: 'pin' | -1 | 1) {
    const buffers = [...workspace.buffers], index = buffers.findIndex(b => b.id === id);
    if (index < 0) return;
    if (direction === 'pin') buffers[index] = { ...buffers[index], pinned: !buffers[index].pinned };
    else if (buffers[index + direction]?.pinned === buffers[index].pinned) [buffers[index], buffers[index + direction]] = [buffers[index + direction], buffers[index]];
    buffers.sort((a,b) => Number(b.pinned)-Number(a.pinned)); workspace = { ...workspace, buffers }; scheduleRecovery();
  }
  function tabContext(event: MouseEvent, id: string) {
    event.preventDefault();
    event.stopPropagation();
    const buffer = workspace.buffers.find(b => b.id === id);
    if (!buffer) return;
    const index = workspace.buffers.findIndex(b => b.id === id);
    const canMoveLeft = index > 0 && workspace.buffers[index - 1]?.pinned === buffer.pinned;
    const canMoveRight = index >= 0 && index < workspace.buffers.length - 1 && workspace.buffers[index + 1]?.pinned === buffer.pinned;
    const onDisk = Boolean(buffer.path && buffer.external !== 'deleted');
    const inRoot = Boolean(onDisk && workspace.root && files.withinRoot(buffer.path, workspace.root));
    const items: MenuEntry[] = [
      { label: 'Save', shortcut: 'Ctrl+S', action: () => saveFile(buffer.id).then(() => {}) },
      { label: 'Commit…', action: () => beginCommit(buffer.id) },
      ...(onDisk ? [
        { label: 'Reveal in Explorer', action: () => revealPath(buffer.path!) },
        { label: 'Copy path', action: () => copyPath(buffer.path!) },
      ] : []),
      'separator',
      { label: buffer.pinned ? 'Unpin' : 'Pin', action: () => alterTab(buffer.id, 'pin') },
      { label: 'Move left', disabled: !canMoveLeft, action: () => alterTab(buffer.id, -1) },
      { label: 'Move right', disabled: !canMoveRight, action: () => alterTab(buffer.id, 1) },
      'separator',
      { label: 'Close', shortcut: 'Ctrl+W', action: () => closeFile(buffer.id) },
      ...(workspace.buffers.length > 1 ? [{ label: 'Close others', action: () => closeOthers(buffer.id) }] : []),
      ...(inRoot ? [{ label: 'Delete', danger: true, action: () => promptDelete(buffer.path!, files.title(buffer)) }] : []),
    ];
    void showContextMenu(event.clientX, event.clientY, items);
  }
  function unsavedEntryContext(event: MouseEvent, buffer: files.Buffer) {
    event.preventDefault();
    event.stopPropagation();
    const items: MenuEntry[] = [
      { label: 'Open', action: () => { mount(buffer.id); view?.focus(); } },
      { label: 'Save', shortcut: 'Ctrl+S', action: () => saveFile(buffer.id).then(() => {}) },
      { label: 'Rename', shortcut: 'F2', action: () => beginRename(buffer.id) },
      'separator',
      { label: 'Close', shortcut: 'Ctrl+W', action: () => closeFile(buffer.id) },
    ];
    void showContextMenu(event.clientX, event.clientY, items);
  }
  function rootEntryContext(event: MouseEvent, entry: files.Entry) {
    event.preventDefault();
    event.stopPropagation();
    if (entry.directory) {
      void showContextMenu(event.clientX, event.clientY, [
        { label: 'Reveal in Explorer', action: () => revealPath(entry.path) },
        { label: 'Copy path', action: () => copyPath(entry.path) },
      ]);
      return;
    }
    const openBuffer = workspace.buffers.find(b => b.path && files.samePath(b.path, entry.path));
    const items: MenuEntry[] = [
      { label: 'Open', action: () => openRootFile(entry.path) },
      ...(openBuffer && files.modified(openBuffer) ? [{ label: 'Save', shortcut: 'Ctrl+S', action: () => saveFile(openBuffer.id).then(() => {}) }] : []),
      { label: 'Commit…', action: () => beginCommitPath(entry.path) },
      { label: 'Rename', shortcut: 'F2', action: () => beginRenamePath(entry.path) },
      { label: 'Reveal in Explorer', action: () => revealPath(entry.path) },
      { label: 'Copy path', action: () => copyPath(entry.path) },
      'separator',
      ...(openBuffer ? [{ label: 'Close tab', action: () => closeFile(openBuffer.id) }] : []),
      { label: 'Delete', danger: true, action: () => promptDelete(entry.path, files.entryLabel(entry)) },
    ];
    void showContextMenu(event.clientX, event.clientY, items);
  }
  function searchMatchContext(event: MouseEvent, match: files.SearchMatch) {
    event.preventDefault();
    event.stopPropagation();
    const items: MenuEntry[] = [
      { label: 'Open', action: () => match.bufferId ? (mount(match.bufferId), jumpToLine(match.line, query)) : openRootFile(match.path!, match.line) },
      ...(match.path ? [
        { label: 'Reveal in Explorer', action: () => revealPath(match.path!) },
        { label: 'Copy path', action: () => copyPath(match.path!) },
      ] : []),
    ];
    void showContextMenu(event.clientX, event.clientY, items);
  }
  function sidebarContext(event: MouseEvent) {
    if ((event.target as HTMLElement | null)?.closest('input, textarea')) return;
    event.preventDefault();
    event.stopPropagation();
    void showContextMenu(event.clientX, event.clientY, [
      { label: 'New file', shortcut: 'Ctrl+N', action: newFile },
      { label: 'Refresh', action: refresh },
      { label: workspace.root ? 'Change root folder…' : 'Set root folder…', shortcut: 'Ctrl+O', action: setRootFolder },
    ]);
  }
  function editorContext(event: MouseEvent) {
    event.preventDefault();
    event.stopPropagation();
    if (!active || !view || diffView) return;
    const hasSelection = !view.state.selection.main.empty;
    const targetId = active.id;
    const items: MenuEntry[] = [
      {
        label: 'Cut', shortcut: 'Ctrl+X', disabled: !hasSelection, action: async () => {
          if (!view) return;
          const s = view.state.selection.main;
          if (s.empty) return;
          await invoke<void>('clipboard_write_text', { text: view.state.sliceDoc(s.from, s.to) });
          view.dispatch({ changes: { from: s.from, to: s.to }, selection: { anchor: s.from }, userEvent: 'delete.cut' });
          view.focus();
        },
      },
      {
        label: 'Copy', shortcut: 'Ctrl+C', action: async () => {
          if (!view) return;
          const s = view.state.selection.main;
          const line = view.state.doc.lineAt(s.from);
          const from = s.empty ? line.from : s.from;
          const to = s.empty ? line.to : s.to;
          await invoke<void>('clipboard_write_text', { text: view.state.sliceDoc(from, to) });
          view.focus();
        },
      },
      {
        label: 'Paste', shortcut: 'Ctrl+V', action: async () => {
          const text = await invoke<string | null>('clipboard_read_text');
          await pasteImage(text ?? '');
          view?.focus();
        },
      },
      { label: 'Paste screenshot', action: () => pasteImage() },
      'separator',
      {
        label: 'Select all', shortcut: 'Ctrl+A', action: () => {
          if (!view) return;
          view.dispatch({ selection: { anchor: 0, head: view.state.doc.length } });
          view.focus();
        },
      },
      { label: 'Find', shortcut: 'Ctrl+F', action: openFind },
      'separator',
      { label: 'Save', shortcut: 'Ctrl+S', action: () => saveFile(targetId).then(() => {}) },
    ];
    void showContextMenu(event.clientX, event.clientY, items);
  }
  function inputMenuItems(target: HTMLInputElement | HTMLTextAreaElement): MenuEntry[] {
    const canEdit = !target.readOnly && !target.disabled;
    const start = target.selectionStart ?? 0, end = target.selectionEnd ?? 0;
    const hasSelection = end > start;
    return [
      {
        label: 'Cut', shortcut: 'Ctrl+X', disabled: !hasSelection || !canEdit, action: async () => {
          await invoke<void>('clipboard_write_text', { text: target.value.slice(start, end) });
          target.setRangeText('', start, end, 'start');
          target.dispatchEvent(new Event('input', { bubbles: true }));
          target.focus();
        },
      },
      {
        label: 'Copy', shortcut: 'Ctrl+C', disabled: !hasSelection, action: async () => {
          await invoke<void>('clipboard_write_text', { text: target.value.slice(start, end) });
          target.focus();
        },
      },
      {
        label: 'Paste', shortcut: 'Ctrl+V', disabled: !canEdit, action: async () => {
          const text = await invoke<string | null>('clipboard_read_text');
          if (text !== null) {
            target.setRangeText(text, start, end, 'end');
            target.dispatchEvent(new Event('input', { bubbles: true }));
          }
          target.focus();
        },
      },
      'separator',
      { label: 'Select all', shortcut: 'Ctrl+A', action: () => { target.focus(); target.select(); } },
    ];
  }
  function onWindowContextMenu(event: MouseEvent) {
    event.preventDefault();
    if (annotation || closeDialog?.open || deleteDialog?.open || saveDialog?.open || commitDialog?.open || restoreDialog?.open) return;
    const target = event.target;
    if (target instanceof HTMLInputElement || target instanceof HTMLTextAreaElement) {
      void showContextMenu(event.clientX, event.clientY, inputMenuItems(target));
    }
  }
  function shortcut(event: KeyboardEvent) {
    if (annotation || closeDialog?.open || deleteDialog?.open || saveDialog?.open || commitDialog?.open || restoreDialog?.open || event.isComposing || event.altKey && event.key.toLowerCase() !== 'z') return;
    if (event.key === 'Escape' && diffView && !findVisible && !renamingTarget) { event.preventDefault(); closeDiff(); return; }
    const key = event.key.toLowerCase();
    if ((event.ctrlKey || event.metaKey) && (key === 's' || key === 'o')) { event.preventDefault(); run(() => key === 's' ? saveFile(workspace.active,event.shiftKey).then(() => {}) : setRootFolder()); return; }
    if (event.key === 'F2' && !event.ctrlKey && !event.metaKey && !event.altKey && !event.shiftKey && workspace.active && !renamingTarget && !busy && !frozen) { event.preventDefault(); run(() => beginRename(workspace.active)); return; }
    const action = tabShortcut(event); if (!action) return; event.preventDefault();
    if (action === 'quit') { void storage.requestExit(); return; }
    run(async () => {
      if (action === 'new') await newFile(); else if (action === 'close' && workspace.active) await closeFile(workspace.active);
      else if (action === 'reopen') await reopen(); else if (action === 'wrap') wrap();
      else if (action === 'sidebar') { workspace = { ...workspace, sidebar: !workspace.sidebar }; await refresh(); syncEditorSearch(); scheduleRecovery(); }
      else if (action === 'explorer' || action === 'search' || action === 'git') await showPanel(action);
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
        if (workspace.root) {
          try { await migrateWorkspaceRoot(workspace.root); } catch (error) { notice = errorMessage(error); }
        }
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
      try {
        const gitCheck = await vcs.checkGit();
        if (!disposed) gitAvailable = gitCheck.available;
      } catch {
        // Ignore unexpected startup check failure; interactive VCS commands check directly.
      }
      try { await refresh(); } catch (error) { notice = errorMessage(error); }
    })();
    void boot.catch(error => { startupError = errorMessage(error); }).finally(() => { loading = false; void poll(); });
    const interval = setInterval(() => { void poll(); }, 2000);
    window.addEventListener('keydown', shortcut);
    window.addEventListener('contextmenu', onWindowContextMenu);
    const focus = () => { void poll(); }; window.addEventListener('focus', focus);
    return () => { disposed = true; clearInterval(interval); clearTimeout(timer); clearTimeout(searchTimer); unlisten?.(); window.removeEventListener('keydown',shortcut); window.removeEventListener('contextmenu', onWindowContextMenu); window.removeEventListener('focus',focus); view?.destroy(); };
  });
</script>

<svelte:head><title>Simple Note</title></svelte:head>
<main class="flex h-dvh flex-col bg-[var(--bg-app)] text-sm text-[var(--text-primary)]" aria-busy={busy || loading || frozen}>
  <header class="app-header">
    <div class="app-brand"><img class="app-mark" src="/app-icon.png" alt=""/><span class="app-title">Simple Note</span></div>
    <nav class="app-menubar" aria-label="Application menus">
      {#each ['File','Edit','View'] as name}<button type="button" aria-haspopup="true" onclick={event => menu(event,`app-${name}`)}>{name}</button>{/each}
    </nav>
    <div class="header-center"><button class="header-search-pill" onclick={() => run(() => showPanel('search'))}>Search <kbd>Ctrl+Shift+F</kbd></button></div>
    <button class="header-btn" disabled={!active || busy || loading || frozen} onclick={() => run(() => saveFile().then(() => {}))}>Save <kbd>Ctrl+S</kbd></button>
    <button class="header-btn" disabled={!active || busy || loading || frozen} onclick={() => run(() => beginCommit(workspace.active))}>Commit</button>
    <div id="app-File" popover class="command-menu">
      <button onclick={() => command(newFile)}>New file <kbd>Ctrl+N</kbd></button>
      <button onclick={() => command(setRootFolder)}>{workspace.root ? 'Change root folder…' : 'Set root folder…'} <kbd>Ctrl+O</kbd></button>
      <button disabled={!active} onclick={() => command(() => saveFile().then(() => {}))}>Save <kbd>Ctrl+S</kbd></button>
      <button disabled={!active} onclick={() => command(() => beginCommit(workspace.active))}>Commit…</button>
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
      <button onclick={() => command(() => showPanel('git'))}>Source Control <kbd>Ctrl+Shift+G</kbd></button>
      <button onclick={() => command(() => { workspace = { ...workspace, sidebar: !workspace.sidebar }; syncEditorSearch(); scheduleRecovery(); })}>Toggle sidebar <kbd>Ctrl+B</kbd></button>
      <button onclick={() => command(wrap)}>Toggle word wrap <kbd>Alt+Z</kbd></button>
    </div>
  </header>
  {#if gitAvailable === false && !gitBannerDismissed}
    <div role="alert" class="git-missing-banner">
      <span>Git is not installed. Please install Git (https://git-scm.com) first to commit and view note history.</span>
      <button type="button" onclick={() => { gitBannerDismissed = true; }}>Dismiss</button>
    </div>
  {/if}
  {#if startupError}<div role="alert" class="p-3 bg-red-950">Could not load workspace: {startupError}</div>{/if}
  {#if recoveryError}<div role="alert" class="p-3 bg-red-950">Recovery failed: {recoveryError} <button onclick={() => run(() => checkpoint())}>Retry</button></div>{/if}
  {#if active?.external}<div role="alert" class="p-3 bg-amber-950">{active.external === 'deleted' ? 'File deleted or moved on disk.' : active.external === 'changed' ? 'File changed on disk.' : 'File unreadable.'} <button onclick={() => run(() => saveFile(workspace.active,true).then(() => {}))}>Save copy in root</button>{#if active.external !== 'deleted'}<button onclick={() => run(reloadDisk)}>Reload from disk</button>{/if}</div>{/if}
  <div class="workspace">
    <nav class="activity-bar" aria-label="Workspace views">
      <button title="Explorer (Ctrl+Shift+E)" aria-label="Explorer" aria-pressed={workspace.sidebar && panel === 'explorer'} onclick={() => run(() => showPanel('explorer'))}><svg viewBox="0 0 24 24" aria-hidden="true"><path d="M8 3h8l4 4v14H8zM16 3v5h4M5 6H3v15h2"/></svg></button>
      <button title="Search (Ctrl+Shift+F)" aria-label="Search" aria-pressed={workspace.sidebar && panel === 'search'} onclick={() => run(() => showPanel('search'))}><svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="10.5" cy="10.5" r="7.5"/><path d="m16 16 5 5"/></svg></button>
      <button title="Source Control (Ctrl+Shift+G)" aria-label="Source Control" aria-pressed={workspace.sidebar && panel === 'git'} onclick={() => run(() => showPanel('git'))}><svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="6" cy="6" r="2.5"/><circle cx="18" cy="8" r="2.5"/><circle cx="6" cy="18" r="2.5"/><path d="M6 8.5v7M6 15.5c0-4 12-1 12-5"/></svg></button>
    </nav>
    {#if workspace.sidebar}<aside class="note-sidebar" aria-label={panel} oncontextmenu={sidebarContext}>
      <div class="sidebar-header"><span class="sidebar-title">{panel === 'git' ? 'SOURCE CONTROL' : panel.toUpperCase()}</span><div class="sidebar-actions"><button onclick={() => run(setRootFolder)}>{workspace.root ? 'Change root…' : 'Set root…'}</button><button class="sidebar-btn" aria-label="Refresh folder" title="Refresh" onclick={() => run(refresh)}>↻</button></div></div>
      {#if workspace.root}<p class="sidebar-root-info">{files.displayPath(workspace.root)}</p>{/if}
      {#if panel === 'search'}
        <form class="library-search" onsubmit={event => { event.preventDefault(); run(refresh); }}>
          <input id="workspace-search" aria-label="Search workspace text" bind:value={query} oninput={onSearchInput} placeholder="Search…"/>
          <button>Search</button>
        </form>
        {#if query && !searchGroups.length}
          <p class="empty-library">No results</p>
        {:else if query}
          {#each searchGroups as group (group.key)}
            <button
              class="search-group-header"
              aria-expanded={!collapsedGroups[group.key]}
              title={group.path ? files.displayPath(group.path) : group.label}
              onclick={() => toggleSearchGroup(group.key)}
            >
              <span>{collapsedGroups[group.key] ? '▸' : '▾'}</span>
              <span class="file-name">{group.label}</span>
              {#if group.unsaved}<span class="file-unsaved-tag">unsaved</span>{/if}
              <span class="search-count-badge">{group.matches.length}</span>
            </button>
            {#if !collapsedGroups[group.key]}
              {#each group.matches as match (match.key)}
                <button
                  class="search-match-row"
                  title={`${match.path ? files.displayPath(match.path) : match.label}:${match.line}`}
                  onclick={() => run(() => match.bufferId ? (mount(match.bufferId), jumpToLine(match.line, query)) : openRootFile(match.path!, match.line))}
                  oncontextmenu={event => searchMatchContext(event, match)}
                >
                  <span class="search-line-num">{match.line}</span>
                  <span class="search-preview">{#each files.highlightText(match.preview, query) as part}{#if part.match}<mark class="search-match-mark">{part.text}</mark>{:else}{part.text}{/if}{/each}</span>
                </button>
              {/each}
            {/if}
          {/each}
        {/if}
      {:else if panel === 'git'}
        {#if !active}
          <p class="empty-library">Open a note to view history</p>
        {:else if gitAvailable === false}
          <p class="empty-library">Git is not installed. Please install Git (https://git-scm.com) first.</p>
        {:else if !workspace.root || files.isUnsavedBuffer(active, workspace.root)}
          <div class="vcs-commit-form">
            <p class="empty-library">Save this note in the root folder to use Git history.</p>
            <button type="button" class="vcs-commit-submit header-btn-primary" disabled={busy || frozen} onclick={() => run(() => saveFile(active.id).then(() => {}))}>Save to root…</button>
          </div>
        {:else if vcsState && !vcsState.supported}
          <p class="empty-library">History is available for saved note packages in the workspace root.</p>
        {:else if vcsState}
          {@const dirty = vcsState.dirty || files.modified(active)}
          <div class="vcs-note-header">
            <span class="vcs-note-name" title={files.displayPath(active.path!)}>{files.title(active)}</span>
            <span class={`vcs-status-pill ${dirty ? 'dirty' : 'clean'}`}>{!vcsState.initialized ? 'Untracked' : dirty ? 'Modified' : 'Clean'}</span>
          </div>
          <form class="vcs-commit-form" onsubmit={event => { event.preventDefault(); run(commitFromSidebar); }}>
            <input id="sidebar-commit-input" aria-label="Commit message" bind:value={sidebarCommitMsg} spellcheck="false" autocomplete="off"/>
            <button type="submit" class="vcs-commit-submit header-btn-primary" disabled={busy || frozen || (vcsState.initialized && !dirty)}>Commit</button>
          </form>
          {#if dirty}
            <div class="sidebar-section-label">Changes</div>
            <button
              class="vcs-change-row"
              class:current={Boolean(diffView && diffView.hash === null)}
              onclick={() => run(() => openDiff(null, 'Working Changes'))}
            >
              <span class="file-name">{files.title(active)}</span>
              <span class={`vcs-change-badge ${vcsState.initialized ? 'modified' : 'untracked'}`}>{vcsState.initialized ? 'M' : 'U'}</span>
            </button>
          {/if}
          <div class="sidebar-section-label">Timeline ({vcsState.commits.length})</div>
          {#if !vcsState.commits.length}
            <p class="empty-library">No commits yet</p>
          {:else}
            <div class="vcs-timeline">
              {#each vcsState.commits as commit, index (commit.hash)}
                <button
                  class="vcs-commit-row"
                  class:head={index === 0}
                  class:selected={Boolean(diffView && diffView.hash === commit.hash)}
                  title={`${commit.shortHash} — ${commit.subject}`}
                  onclick={() => run(() => openDiff(commit.hash, `${commit.shortHash}: ${commit.subject}`, 'parent'))}
                  oncontextmenu={event => commitContext(event, commit)}
                >
                  <span class="vcs-commit-dot" aria-hidden="true"></span>
                  <div class="vcs-commit-body">
                    <span class="vcs-commit-subject">{commit.subject}</span>
                    <span class="vcs-commit-meta">
                      <span class="vcs-sha">{commit.shortHash}</span>
                      <span>·</span>
                      <span>{vcs.formatRelativeTime(commit.timestamp)}</span>
                    </span>
                  </div>
                </button>
              {/each}
            </div>
          {/if}
        {/if}
      {:else}
        {#if unsavedBuffers.length}
          <div class="sidebar-section-label">Unsaved ({unsavedBuffers.length})</div>
          {#each unsavedBuffers as buffer (buffer.id)}
            {#if renamingTarget?.kind === 'buffer' && renamingTarget.id === buffer.id}
              <div class="library-row renaming">
                <span>·</span>
                <input id="rename-input" class="sidebar-rename-input" aria-label="Rename file" bind:value={renameValue} onkeydown={onRenameKeydown} onblur={onRenameBlur}/>
              </div>
            {:else}
              <button
                class="library-row"
                class:current={workspace.active === buffer.id}
                title={buffer.path ? `${files.displayPath(buffer.path)} (outside root)` : `${files.title(buffer)} (Unsaved)`}
                onclick={() => run(() => { mount(buffer.id); view?.focus(); })}
                onkeydown={event => { if (event.key === 'F2') { event.preventDefault(); event.stopPropagation(); run(() => beginRename(buffer.id)); } }}
                oncontextmenu={event => unsavedEntryContext(event, buffer)}
              >
                <span>·</span>
                <span class="file-name">{files.title(buffer)}{files.modified(buffer) ? ' ●' : ''}</span>
                <span class="file-unsaved-tag">unsaved</span>
              </button>
            {/if}
          {/each}
        {/if}
        {#if workspace.root}
          {#if unsavedBuffers.length || explorerEntries.length}
            <div class="sidebar-section-label">Root files</div>
          {/if}
          {#each explorerEntries as entry (entry.path)}
            {@const openBuffer = workspace.buffers.find(b => b.path && files.samePath(b.path, entry.path))}
            {#if isRenamingEntry(entry, openBuffer)}
              <div class="library-row renaming" style:padding-left={`${12 + files.entryDepth(entry) * 12}px`}>
                <span>·</span>
                <input id="rename-input" class="sidebar-rename-input" aria-label="Rename file" bind:value={renameValue} onkeydown={onRenameKeydown} onblur={onRenameBlur}/>
              </div>
            {:else}
              <button
                class="library-row"
                class:current={Boolean(active?.path && files.samePath(active.path, entry.path))}
                title={files.displayPath(entry.path)}
                aria-disabled={entry.directory}
                style:padding-left={`${12 + files.entryDepth(entry) * 12}px`}
                onclick={() => { if (!entry.directory) run(() => openRootFile(entry.path)); }}
                onkeydown={event => { if (event.key === 'F2' && !entry.directory) { event.preventDefault(); event.stopPropagation(); run(() => beginRenamePath(entry.path)); } }}
                oncontextmenu={event => rootEntryContext(event, entry)}
              >
                <span>{entry.directory ? '▾' : '·'}</span>
                <span class="file-name">{files.entryLabel(entry)}{openBuffer && files.modified(openBuffer) ? ' ●' : ''}</span>
              </button>
            {/if}
          {/each}
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
      <div bind:this={contextPopover} popover class="command-menu" aria-label="Context menu">
        {#each contextItems as item}
          {#if item === 'separator'}<div class="menu-separator"></div>
          {:else}<button class:danger={item.danger} disabled={item.disabled} onclick={() => command(item.action)}>{item.label}{#if item.shortcut}<kbd>{item.shortcut}</kbd>{/if}</button>{/if}
        {/each}
      </div>
      {#if findVisible}<form class="find-bar" onsubmit={find}><input id="find" aria-label="Find in file" bind:value={findQuery} oninput={onFindInput} placeholder="Find…" onkeydown={event => { if (event.key === 'Escape') { event.preventDefault(); closeFind(); } else if (event.key === 'Enter' && event.shiftKey) { event.preventDefault(); stepFind(true); } }}/><button type="button" aria-label="Previous match" title="Previous match (Shift+Enter)" onclick={() => stepFind(true)}>↑</button><button type="submit" aria-label="Next match" title="Next match (Enter)">↓</button><button type="button" aria-label="Close find" onclick={closeFind}>×</button></form>{/if}
      <div id="file-panel" role="tabpanel" tabindex="0" aria-label="Active file" class="relative min-h-0 min-w-0 flex-1" oncontextmenu={editorContext}>
        <div class="h-full" class:hidden={!active || Boolean(diffView)} bind:this={host}></div>
        {#if diffView}
          <div class="diff-viewer" aria-label="Diff viewer">
            <div class="diff-header">
              <span class="diff-title" title={diffView.subject}>{diffView.label} · {diffView.subject}</span>
              {#if diffView.hash !== null}
                <div class="diff-mode-group" role="group" aria-label="Comparison mode">
                  <button type="button" aria-pressed={diffView.mode === 'parent'} onclick={() => run(() => openDiff(diffView!.hash, diffView!.subject, 'parent'))}>Commit</button>
                  <button type="button" aria-pressed={diffView.mode === 'working'} onclick={() => run(() => openDiff(diffView!.hash, diffView!.subject, 'working'))}>vs Current</button>
                </div>
              {/if}
              <span class="diff-stats">
                <span class="diff-stat-add">+{diffView.summary.added}</span>
                <span class="diff-stat-del">-{diffView.summary.removed}</span>
              </span>
              {#if diffView.metaChanged || diffView.assetsChanged}
                <span class="diff-extra-badge">
                  {#if diffView.metaChanged && diffView.assetsChanged}Annotations &amp; assets changed
                  {:else if diffView.metaChanged}Annotations changed
                  {:else}Assets changed{/if}
                </span>
              {/if}
              {#if diffView.hash !== null}
                <button type="button" disabled={busy || frozen} onclick={() => promptRestore({ hash: diffView!.hash!, shortHash: diffView!.hash!.slice(0, 7) })}>Restore</button>
              {/if}
              <button type="button" aria-label="Close diff" title="Close (Esc)" onclick={closeDiff}>×</button>
            </div>
            <div class="diff-scroll" class:wrap={workspace.wrap}>
              {#if !diffView.summary.lines.length}
                <p class="empty-library">Empty file</p>
              {:else}
                {#each diffView.summary.lines as line}
                  <div class={`diff-row ${line.kind === 'add' ? 'add' : line.kind === 'del' ? 'del' : ''}`}>
                    <span class="diff-num">{line.oldLine ?? ''}</span>
                    <span class="diff-num">{line.newLine ?? ''}</span>
                    <span class="diff-sign">{line.kind === 'add' ? '+' : line.kind === 'del' ? '-' : ' '}</span>
                    <span class="diff-code">{line.text || ' '}</span>
                  </div>
                {/each}
              {/if}
            </div>
          </div>
        {/if}
        {#if !active && !loading && !diffView}<div class="empty-workspace"><h1 class="empty-title">Simple Note</h1><div class="empty-actions"><button onclick={() => run(newFile)}>New file</button><button onclick={() => run(setRootFolder)}>{workspace.root ? 'Change root folder…' : 'Set root folder…'}</button><button onclick={() => run(() => showPanel('explorer'))}>Explorer</button></div></div>{/if}
      </div>
    </div>
  </div>
  <footer class="status-bar"><span role="status">{loading ? 'Loading…' : busy || frozen ? 'Working…' : active && files.modified(active) ? 'Modified' : 'Saved'}{recoveryError ? ' · Recovery failed' : ''}</span>{#if notice}<span role="status" class="status-notice">{notice}</span>{/if}<div class="status-tools"><button onclick={() => run(wrap)}>Wrap: {workspace.wrap ? 'On' : 'Off'}</button><span>{active ? files.bufferLanguage(active) : 'markdown'} · UTF-8</span></div></footer>
</main>
<dialog bind:this={closeDialog} class="settings-dialog" aria-labelledby="close-heading" oncancel={() => { closeTarget = null; }}>
  <h2 id="close-heading">Save changes before closing?</h2>
  <div class="dialog-actions"><button disabled={busy} onclick={() => run(() => closeFile(closeTarget!,'save'))}>Save</button><button disabled={busy} onclick={() => run(() => closeFile(closeTarget!,'discard'))}>Don’t Save</button><button disabled={busy} onclick={() => { closeDialog.close(); closeTarget = null; }}>Cancel</button></div>
</dialog>
<dialog bind:this={deleteDialog} class="settings-dialog" aria-labelledby="delete-heading" oncancel={() => { deleteTarget = null; }}>
  <h2 id="delete-heading">Delete {deleteTarget?.name ?? ''}?</h2>
  <div class="dialog-actions"><button disabled={busy} onclick={() => run(confirmDelete)}>Delete</button><button disabled={busy} onclick={() => { deleteDialog.close(); deleteTarget = null; }}>Cancel</button></div>
</dialog>
<dialog bind:this={saveDialog} class="settings-dialog save-dialog" aria-labelledby="save-heading" oncancel={event => { if (busy) { event.preventDefault(); return; } savePrompt = null; saveError = ''; view?.focus(); }}>
  <form class="save-dialog-form" onsubmit={event => { event.preventDefault(); confirmSavePrompt(); }}>
    <h2 id="save-heading">{savePrompt?.saveCopy ? 'Save copy' : 'Save file'}</h2>
    <input id="save-name-input" aria-label="File name" bind:value={saveName} readonly={busy} spellcheck="false" autocomplete="off" oninput={() => { if (saveError) saveError = ''; }} />
    {#if saveError}<p role="alert" class="save-dialog-error">{saveError}</p>{/if}
    <div class="dialog-actions"><button type="button" disabled={busy} onclick={cancelSavePrompt}>Cancel</button><button type="submit" class="header-btn-primary" disabled={busy}>Save</button></div>
  </form>
</dialog>
<dialog bind:this={commitDialog} class="settings-dialog save-dialog" aria-labelledby="commit-heading" oncancel={event => { if (busy) { event.preventDefault(); return; } commitTargetId = null; commitError = ''; view?.focus(); }}>
  <form class="save-dialog-form" onsubmit={event => { event.preventDefault(); confirmCommitDialog(); }}>
    <h2 id="commit-heading">Commit snapshot</h2>
    <input id="commit-message-input" aria-label="Commit message" bind:value={commitMessage} readonly={busy} spellcheck="false" autocomplete="off" oninput={() => { if (commitError) commitError = ''; }} />
    {#if commitError}<p role="alert" class="save-dialog-error">{commitError}</p>{/if}
    <div class="dialog-actions"><button type="button" disabled={busy} onclick={cancelCommitDialog}>Cancel</button><button type="submit" class="header-btn-primary" disabled={busy}>Commit</button></div>
  </form>
</dialog>
<dialog bind:this={restoreDialog} class="settings-dialog" aria-labelledby="restore-heading" oncancel={() => { restoreTarget = null; }}>
  <h2 id="restore-heading">Restore version {restoreTarget?.shortHash ?? ''}?</h2>
  <p class="mt-2 text-xs text-[var(--text-secondary)]">Note text, image annotations, and assets will be restored to your working copy. Existing commit history stays intact.</p>
  <div class="dialog-actions"><button disabled={busy} onclick={() => run(confirmRestore)}>Restore</button><button disabled={busy} onclick={() => { restoreDialog.close(); restoreTarget = null; }}>Cancel</button></div>
</dialog>
{#if annotation}<AnnotationEditor block={annotation.block} blob={annotation.blob} status={recoveryError} onchange={annotate} onoutput={outputAnnotation} onclose={() => { annotation = undefined; view?.focus(); }}/>{/if}

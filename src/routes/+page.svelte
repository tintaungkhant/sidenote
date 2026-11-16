<script lang="ts">
  import '../app.css';
  import { onMount } from 'svelte';
  import { Compartment, EditorState } from '@codemirror/state';
  import { EditorView, drawSelection, highlightActiveLine, keymap, lineNumbers, type Command } from '@codemirror/view';
  import { defaultKeymap, historyKeymap, indentWithTab, undo, redo, moveLineUp, moveLineDown, copyLineUp, copyLineDown, transposeChars } from '@codemirror/commands';
  import { bracketMatching, defaultHighlightStyle, indentOnInput, syntaxHighlighting } from '@codemirror/language';
  import { findNext, search, setSearchQuery } from '@codemirror/search';
  import { javascript } from '@codemirror/lang-javascript';
  import { json } from '@codemirror/lang-json';
  import { html } from '@codemirror/lang-html';
  import { markdown } from '@codemirror/lang-markdown';
  import { listen } from '@tauri-apps/api/event';
  import { createNoteState, imageField, insertImage, moveRange, textSearch, annotateImage } from '$lib/editor/state';
  import AnnotationEditor from '$lib/editor/AnnotationEditor.svelte';
  import { decodeDocument, type Annotation, type AttachmentId, type ImageBlock, type ImageId } from '$lib/editor/document';
  import { flattenImage } from '$lib/editor/image-output';
  import { imagePresentation } from '$lib/editor/view';
  import { Autosave, errorMessage, type SaveStatus } from '$lib/autosave';
  import { ExitGuard } from '$lib/exit';
  import { emptySession, normalizeSession, openTab, closeTab, changeTab, automaticTitle, tabShortcut } from '$lib/tabs';
  import * as storage from '$lib/storage';

  let host: HTMLDivElement;
  let view: EditorView | undefined;
  let saver: Autosave | undefined;
  let note: storage.Note | undefined;
  let session = $state.raw<storage.Session>(emptySession());
  let titles = $state<Record<string, string>>({});
  let busy = $state(false);
  let renameValue = $state('');
  let renaming = $state(false);
  let transition: Promise<void> = Promise.resolve();
  const notes = new Map<storage.NoteId, storage.Note>();
  const states = new Map<storage.NoteId, EditorState>();
  const automatic = new Set<storage.NoteId>();
  let closed = $state.raw<storage.SessionTab[]>([]);
  let bootstrap: Promise<void> = Promise.resolve();
  let insertion: Promise<void> | undefined;
  let recovery: Promise<void> | undefined;
  let imageOutput: Promise<string> | undefined;
  let backupWork: Promise<void> | undefined;
  let backupPending = $state(false);
  let backupStatus = $state.raw<storage.BackupStatus>({ directory: '', lastBackup: null, error: null });
  let restorePreview = $state.raw<storage.RestorePreview | null>(null);
  let disposed = false;
  let loading = $state(true);
  let frozen = $state(false);
  let inserting = $state(false);
  let annotation = $state.raw<{ block: ImageBlock; blob: Blob } | undefined>();
  let startupError = $state('');
  let saveError = $state('');
  let notice = $state('');
  let saveStatus = $state<SaveStatus>('Saving');
  let confirmDiscard = $state(false);
  let language = $state<storage.NoteLanguage>('plaintext');
  let wrap = $state(true);
  let query = $state('');
  let libraryQuery = $state('');
  let library = $state.raw<storage.NoteSummary[]>([]);
  let trash = $state(false);
  let libraryMore = $state(false);
  let purge = $state.raw<storage.NoteSummary | undefined>();
  const languageSlot = new Compartment();
  const wrapSlot = new Compartment();
  const editabilitySlot = new Compartment();
  const sources = new Map<AttachmentId, Blob>();
  const languageNames: Record<storage.NoteLanguage, string> = { plaintext: 'Plain text', markdown: 'Markdown', javascript: 'JavaScript', typescript: 'TypeScript', json: 'JSON', html: 'HTML' };
  // Text-only reordering commands cannot preserve sidecar identity. Use explicit range movement instead.
  const textOnlyCommands = new Set<Command>([moveLineUp, moveLineDown, copyLineUp, copyLineDown, transposeChars]);
  const editingKeys = defaultKeymap.map(binding => ({ ...binding,
    run: binding.run && textOnlyCommands.has(binding.run) ? (editor: EditorView) =>
      editor.state.field(imageField).length > 0 || binding.run!(editor) : binding.run
  }));

  const exit = new ExitGuard({
    freeze: setFrozen,
    /** Await startup, image insertion/output, recovery, and tab transitions before taking the final close snapshot. */
    prepare: async () => { await bootstrap.catch(() => {}); await insertion; await recovery; await imageOutput?.catch(() => {}); await backupWork; await transition; stageSession(); },
    flush: async () => {
      if (!saver) throw new Error(startupError || 'Storage has not finished loading');
      await saver.flush();
    },
    finish: storage.finishExit,
    cancel: storage.cancelExit,
    failed: error => { saveError = errorMessage(error); }
  });

  /** Reflect acknowledgements only; a delayed save response never replaces current editor content. */
  function refreshStatus() {
    if (!saver) return;
    saveStatus = saver.status;
    saveError = saver.error ? errorMessage(saver.error) : '';
  }

  /** Blur composition and freeze input during close/export; failed close resumes the same editor state. */
  function setFrozen(value: boolean) {
    if (value) annotation = undefined;
    if (value) view?.contentDOM.blur();
    frozen = value;
    saver?.pause(value);
    view?.dispatch({ effects: editabilitySlot.reconfigure([EditorState.readOnly.of(value), EditorView.editable.of(!value)]) });
  }

  /** Retain selection, scroll, and preferences while preserving other saved tabs for ticket 004. */
  function stageSession() {
    if (!saver) return;
    if (!view || !note) { saver.setSession(session); return; }
    const selection = view.state.selection.main;
    session = { ...session, wordWrap: wrap, activeNoteId: note.id,
      tabs: session.tabs.map(tab => tab.noteId === note!.id ? { ...tab, anchor: selection.anchor, head: selection.head, scrollTop: view!.scrollDOM.scrollTop } : tab) };
    saver.setSession(session);
  }

  /** Convert editor state back to ordered source/image blocks before scheduling a durable write. */
  function stageNote(immediate = false) {
    if (!saver || !view || !note) return;
    const document = decodeDocument({ text: view.state.doc.toString(), images: view.state.field(imageField) });
    if (automatic.has(note.id)) { note.title = automaticTitle(document); titles = { ...titles, [note.id]: note.title }; }
    note.document = document;
    note.language = language;
    saver.edit(note.id, { title: note.title, language, document }, immediate);
  }

  /** Load metadata in bounded pages; normalize only missing/deleted IDs, never storage failures. */
  async function loadWorkspace() {
    loading = true;
    startupError = '';
    try {
      const saved = await storage.loadSession();
      const summaries: storage.NoteSummary[] = [];
      for (let offset = 0; ; offset += 200) {
        const page = await storage.listNotes(200, offset);
        summaries.push(...page.filter(item => item.deletedAt === null));
        if (page.length < 200) break;
      }
      if (disposed) return;
      titles = Object.fromEntries(summaries.map(item => [item.id, item.title]));
      session = normalizeSession(saved.state ?? emptySession(), new Set(summaries.map(item => item.id)));
      wrap = session.wordWrap;
      saver = new Autosave(storage, saved, refreshStatus);
      saver.pause(frozen);
      if (!saved.state) {
        if (summaries.length) session = openTab(session, summaries[0].id);
        else {
          const created = await storage.createNote({ title: 'Untitled', language: 'plaintext', document: { version: 1, blocks: [{ kind: 'text', text: '' }] } });
          rememberNote(created);
          session = openTab(session, created.id);
        }
      }
      if (session.activeNoteId) await mountNote(session.activeNoteId);
      stageSession();
      if (session.sidebarVisible) await browseLibrary();
      refreshStatus();
    } catch (error) {
      startupError = errorMessage(error);
      throw error;
    } finally { loading = false; }
  }

  /** Register each note once; explicit custom titles remain stable across content edits. */
  function rememberNote(loaded: storage.Note) {
    notes.set(loaded.id, loaded);
    saver!.register(loaded);
    if (loaded.title === automaticTitle(loaded.document)) automatic.add(loaded.id);
    titles = { ...titles, [loaded.id]: loaded.title };
  }

  /** Cache immutable editor states, including history; only one EditorView exists at a time. */
  function unmountNote() {
    if (!view || !note) return;
    stageSession();
    states.set(note.id, view.state);
    view.scrollDOM.removeEventListener('scroll', stageSession);
    view.destroy(); view = undefined; note = undefined;
  }

  /** Prepare originals before replacing the active view so load failures preserve current work. */
  async function mountNote(id: storage.NoteId) {
      if (view && note?.id === id) return;
      if (!session.tabs.some(tab => tab.noteId === id) && session.tabs.length >= 1000) throw new Error('Close a tab before opening more than 1000 tabs');
      let loaded = notes.get(id);
      if (!loaded) {
        loaded = await storage.loadNote(id);
        if (loaded.deletedAt !== null) throw new Error('This note is in Trash');
        rememberNote(loaded);
      }
      if (disposed) return;
      unmountNote();
      session = openTab(session, id);
      note = loaded;
      language = loaded.language;
      renaming = false;
      const tab = session.tabs.find(tab => tab.noteId === id)!;
      const idForView = id;
      view = new EditorView({ parent: host, state: createNoteState(loaded.document, [
        lineNumbers(), drawSelection(), highlightActiveLine(), indentOnInput(), bracketMatching(),
        syntaxHighlighting(defaultHighlightStyle), search(), imagePresentation(async id => {
          const cached = sources.get(id);
          if (cached) return cached;
          const original = await storage.loadAttachment(idForView, id);
          return new Blob([new Uint8Array(original.bytes)], { type: original.mimeType });
        }, openAnnotation),
        EditorView.contentAttributes.of({ 'aria-label': 'Note editor' }),
        EditorView.domEventHandlers({ paste(event) { return pasteScreenshot(event); } }),
        keymap.of([{ key: 'Mod-f', run: () => { document.getElementById('find')?.focus(); return true; } }, indentWithTab, ...editingKeys, ...historyKeymap]),
        languageSlot.of(languageExtension()), wrapSlot.of(wrap ? EditorView.lineWrapping : []),
        editabilitySlot.of([EditorState.readOnly.of(frozen), EditorView.editable.of(!frozen)]),
        EditorView.updateListener.of(update => {
          const imagesChanged = update.startState.field(imageField) !== update.state.field(imageField);
          if (update.docChanged || imagesChanged) stageNote(!update.docChanged && imagesChanged);
          if (update.docChanged || update.selectionSet) stageSession();
        })
      ]) });
      const cached = states.get(id);
      if (cached) {
        view.setState(cached);
        view.dispatch({ effects: [languageSlot.reconfigure(languageExtension()), wrapSlot.reconfigure(wrap ? EditorView.lineWrapping : []), editabilitySlot.reconfigure([EditorState.readOnly.of(frozen), EditorView.editable.of(!frozen)])] });
      } else view.dispatch({ selection: { anchor: Math.min(tab.anchor, view.state.doc.length), head: Math.min(tab.head, view.state.doc.length) } });
      view.scrollDOM.scrollTop = tab.scrollTop;
      view.scrollDOM.addEventListener('scroll', stageSession);
      stageSession();
  }

  /** Serialize tab operations and include pending note creation/loading in the close handshake. */
  function runTab(action: () => Promise<void> | void) {
    if (busy || frozen || loading || insertion || annotation || startupError) return;
    busy = true;
    transition = Promise.resolve().then(action).catch(error => { notice = errorMessage(error); }).finally(() => { busy = false; });
  }

  /** Create durable identity before opening a fresh tab; failed creation never adds a phantom tab. */
  async function newNote() {
    if (session.tabs.length >= 1000) throw new Error('Close a tab before creating another note');
    const created = await storage.createNote({ title: 'Untitled', language: 'plaintext', document: { version: 1, blocks: [{ kind: 'text', text: '' }] } });
    rememberNote(created);
    await mountNote(created.id);
  }

  /** Close only the tab, retaining source, autosave generations, history, and reopen metadata. */
  async function closeNote(id: storage.NoteId) {
    stageSession();
    const tab = session.tabs.find(tab => tab.noteId === id);
    if (!tab) return;
    const next = closeTab(session, id);
    if (session.activeNoteId === id && next.activeNoteId) await mountNote(next.activeNoteId);
    else if (session.activeNoteId === id) unmountNote();
    closed = [...closed, tab];
    session = next;
    stageSession();
  }

  /** Reopen the most recently closed tab with its pin, selection, scroll, and cached undo state. */
  async function reopenNote() {
    const tab = closed.at(-1);
    if (!tab) return;
    await mountNote(tab.noteId);
    session = openTab(closeTab(session, tab.noteId), tab.noteId, tab);
    closed = closed.slice(0, -1);
    if (view) view.scrollDOM.scrollTop = tab.scrollTop;
    stageSession();
  }

  /** Persist tab metadata immediately into autosave's current session before any possible exit. */
  function alterTab(id: storage.NoteId, action: 'pin' | -1 | 1) {
    stageSession();
    session = changeTab(session, id, action);
    stageSession();
  }

  /** Commit a bounded custom title without changing source or undo history. */
  function renameNote(event: SubmitEvent) {
    event.preventDefault();
    if (!note || frozen) return;
    const title = renameValue.trim();
    if (!title || new TextEncoder().encode(title).length > 1024) { notice = 'Title must contain 1–1024 UTF-8 bytes.'; return; }
    automatic.delete(note.id);
    note.title = title;
    titles = { ...titles, [note.id]: title };
    renaming = false;
    stageNote();
  }

  /** Persist optional sidebar visibility even when every tab is closed. */
  async function toggleSidebar() {
    session = { ...session, sidebarVisible: !session.sidebarVisible }; stageSession();
    if (session.sidebarVisible) await browseLibrary();
  }

  /** Flush current edits before searching so results include the latest source and custom titles. */
  async function browseLibrary(append = false) {
    await saver!.flush();
    const page = await storage.browseNotes(trash, libraryQuery, append ? library.length : 0);
    library = append ? [...library, ...page] : page;
    libraryMore = page.length === 100;
  }

  /** Freeze editor input, drain saves, then apply a revision-checked native Trash transaction. */
  async function lifecycle(item: storage.NoteSummary, action: 'trash' | 'restore' | 'purge') {
    view?.contentDOM.blur();
    view?.dispatch({ effects: editabilitySlot.reconfigure([EditorState.readOnly.of(true), EditorView.editable.of(false)]) });
    try {
      stageSession();
      await saver!.flush();
      const saved = await storage.changeNoteLifecycle(item.id, saver!.revision(item.id) ?? item.revision, action);
      if (note?.id === item.id) {
        view?.scrollDOM.removeEventListener('scroll', stageSession);
        view?.destroy(); view = undefined; note = undefined;
      }
      saver!.forget(item.id, saved);
      notes.delete(item.id); states.delete(item.id); automatic.delete(item.id);
      closed = closed.filter(tab => tab.noteId !== item.id);
      session = saved.state ?? closeTab(session, item.id);
      if (action !== 'restore') { const next = { ...titles }; delete next[item.id]; titles = next; }
      else titles = { ...titles, [item.id]: item.title };
      purge = undefined;
      if (session.activeNoteId) await mountNote(session.activeNoteId);
      await browseLibrary();
    } finally {
      view?.dispatch({ effects: editabilitySlot.reconfigure([EditorState.readOnly.of(frozen), EditorView.editable.of(!frozen)]) });
    }
  }

  /** Roving tab focus supports arrows/Home/End without requiring pointer input. */
  function tabKey(event: KeyboardEvent, index: number) {
    const target = event.key === 'Home' ? 0 : event.key === 'End' ? session.tabs.length - 1
      : event.key === 'ArrowRight' ? (index + 1) % session.tabs.length
      : event.key === 'ArrowLeft' ? (index + session.tabs.length - 1) % session.tabs.length : -1;
    if (target < 0) return;
    event.preventDefault();
    const id = session.tabs[target].noteId;
    runTab(async () => { await mountNote(id); document.getElementById(`tab-${id}`)?.focus(); });
  }

  /** Subscribe before loading so startup close requests are retained by the native handshake. */
  onMount(() => {
    let unlisten: (() => void) | undefined;
    let unlistenBackup: (() => void) | undefined;
    bootstrap = (async () => {
      unlisten = await listen('flush-before-exit', () => { void exit.request(); });
      if (disposed) { unlisten(); return; }
      unlistenBackup = await listen<storage.BackupStatus>('backup-status', event => { backupStatus = event.payload; });
      if (disposed) { unlistenBackup(); return; }
      backupStatus = await storage.backupStatus();
      if (await storage.exitReady()) void exit.request();
      await loadWorkspace();
    })();
    void bootstrap.catch(error => { startupError = errorMessage(error); loading = false; });
    window.addEventListener('keydown', quitShortcut);
    // Destroy the view's listeners, observers, and widgets when the host unmounts.
    return () => {
      disposed = true;
      saver?.stop();
      unlisten?.();
      unlistenBackup?.();
      window.removeEventListener('keydown', quitShortcut);
      view?.scrollDOM.removeEventListener('scroll', stageSession);
      view?.destroy(); view = undefined;
      sources.clear();
    };
  });

  /** Retry failed startup without duplicating native listeners or replacing an editable note. */
  function retryStartup() {
    saver?.stop();
    view?.destroy(); view = undefined;
    notes.clear(); states.clear(); automatic.clear();
    sources.clear();
    bootstrap = loadWorkspace();
    void bootstrap.catch(() => {});
  }

  /** App shortcuts leave OS-reserved combinations and IME input untouched. */
  function quitShortcut(event: KeyboardEvent) {
    if (annotation) return;
    const action = tabShortcut(event);
    if (!action) return;
    event.preventDefault();
    if (action === 'quit') { void storage.requestExit().catch(error => { saveError = errorMessage(error); }); return; }
    runTab(async () => {
      if (action === 'new') await newNote();
      else if (action === 'close' && session.activeNoteId) await closeNote(session.activeNoteId);
      else if (action === 'reopen') await reopenNote();
      else if (action === 'sidebar') await toggleSidebar();
      else if (session.tabs.length) {
        const index = session.tabs.findIndex(tab => tab.noteId === session.activeNoteId);
        await mountNote(session.tabs[(index + (action === 'previous' ? session.tabs.length - 1 : 1)) % session.tabs.length].noteId);
      }
    });
  }

  /** Select syntax extensions from persisted identifiers rather than UI display labels. */
  function languageExtension() {
    return language === 'javascript' ? javascript() : language === 'typescript' ? javascript({ typescript: true })
      : language === 'json' ? json() : language === 'html' ? html() : language === 'markdown' ? markdown() : [];
  }

  /** Reconfigure syntax while retaining content, selections, and undo history, then save the preference. */
  function changeLanguage() {
    view?.dispatch({ effects: languageSlot.reconfigure(languageExtension()) });
    stageNote();
  }

  /** Change wrapping without recreating the mounted editor and persist session preferences. */
  function toggleWrap() {
    wrap = !wrap;
    view?.dispatch({ effects: wrapSlot.reconfigure(wrap ? EditorView.lineWrapping : []) });
    stageSession();
  }

  /** Move selected content one adjacent Unicode character, including any selected images. */
  function moveSelection(direction: -1 | 1) {
    if (!view || frozen) return;
    const { from, to, empty } = view.state.selection.main;
    if (empty) return;
    const adjacent = direction < 0 ? Array.from(view.state.sliceDoc(0, from)).at(-1) : Array.from(view.state.sliceDoc(to))[0];
    if (!adjacent) return;
    view.dispatch(moveRange(view.state, from, to, direction < 0 ? from - adjacent.length : to + adjacent.length));
    view.focus();
  }

  /** Find literal text, rejecting matches that include actual image positions. */
  function find(event: SubmitEvent) {
    event.preventDefault();
    if (!view || frozen) return;
    view.dispatch({ effects: setSearchQuery.of(textSearch(query)) });
    findNext(view);
  }

  /** Hold the target selection stable until native validation/storage finishes; failures never edit source. */
  function pasteScreenshot(event?: ClipboardEvent): boolean {
    event?.preventDefault();
    if (!view || frozen || insertion || busy || loading || view.state.readOnly) return true;
    const target = view;
    const selection = target.state.selection;
    const text = event?.clipboardData?.getData('text/plain') ?? '';
    inserting = true;
    target.dispatch({ effects: editabilitySlot.reconfigure([EditorState.readOnly.of(true), EditorView.editable.of(false)]) });
    insertion = (async () => {
      const original = await storage.pasteImage();
      if (disposed || view !== target) return;
      target.dispatch({ selection });
      if (original) {
        sources.set(original.id, new Blob([new Uint8Array(original.bytes)], { type: original.mimeType }));
        const block: ImageBlock = { kind: 'image', id: crypto.randomUUID() as ImageId, attachmentId: original.id, annotations: [] };
        target.dispatch(insertImage(target.state, block));
        stageNote(true);
      } else if (text) target.dispatch(target.state.replaceSelection(text), { userEvent: 'input.paste' });
    })().catch(error => { notice = errorMessage(error); }).finally(() => {
      inserting = false; insertion = undefined;
      if (!disposed && view === target) {
        target.dispatch({ effects: editabilitySlot.reconfigure([EditorState.readOnly.of(frozen), EditorView.editable.of(!frozen)]) });
        if (!frozen) target.focus();
      }
    });
    return true;
  }

  /** Load the immutable original before opening; keep tab identity stable while native I/O is pending. */
  function openAnnotation(block: ImageBlock) {
    if (!note || !view || frozen || busy || insertion || annotation) return;
    const target = view, noteId = note.id;
    inserting = true;
    target.dispatch({ effects: editabilitySlot.reconfigure([EditorState.readOnly.of(true), EditorView.editable.of(false)]) });
    insertion = (async () => {
      let blob = sources.get(block.attachmentId);
      if (!blob) {
        const original = await storage.loadAttachment(noteId, block.attachmentId);
        blob = new Blob([new Uint8Array(original.bytes)], { type: original.mimeType });
      }
      if (!disposed && !frozen && view === target) annotation = { block, blob };
    })().catch(error => { notice = errorMessage(error); }).finally(() => {
      inserting = false; insertion = undefined;
      if (!disposed && view === target) target.dispatch({ effects: editabilitySlot.reconfigure([EditorState.readOnly.of(frozen), EditorView.editable.of(!frozen)]) });
    });
  }

  /** Return to the unchanged editor selection and scroll; only unfinished canvas gestures are discarded. */
  function closeAnnotation() { annotation = undefined; view?.focus(); }

  /** Export the current completed snapshot, independent of save acknowledgements; guarded exit awaits native output. */
  function outputAnnotation(action: 'copy' | 'export', annotations: readonly Annotation[], width: number, height: number): Promise<string> {
    if (!annotation || frozen || imageOutput) return Promise.reject(new Error('Image output is unavailable. Reopen the image and retry.'));
    const original = annotation.blob;
    const pending = (async () => {
      const png = await flattenImage(original, annotations, width, height);
      if (action === 'copy') { await storage.copyImage(png); return 'Image copied.'; }
      return await storage.exportImage(png) ? 'PNG exported.' : 'PNG export cancelled.';
    })();
    imageOutput = pending;
    return pending.finally(() => { imageOutput = undefined; });
  }

  /** Retry saving in place; success preserves current content and undo history. */
  async function retrySave() {
    try { await saver?.flush(); } catch { /* Autosave retains the error and dirty generations for another retry or export. */ }
  }

  /** Freeze a consistent snapshot while the native save dialog exports source and available originals. */
  function exportCurrentRecovery() {
    if (recovery || frozen) return;
    recovery = writeCurrentRecovery().finally(() => { recovery = undefined; if (!exit.busy) setFrozen(false); });
  }

  /** Keep export completion tracked so native close waits for the selected destination to finish writing. */
  async function writeCurrentRecovery() {
    setFrozen(true);
    try {
      await insertion;
      stageSession();
      if (!saver) throw new Error('No workspace has loaded; the existing database remains unchanged');
      const exported = await storage.exportRecovery(saver.snapshot());
      notice = !exported.exported ? 'Recovery export cancelled.' : exported.missingOriginals
        ? `Recovery source exported; ${exported.missingOriginals} image originals were unavailable and are marked in the file.`
        : 'Recovery exported. Unsaved changes are still open.';
    } catch (error) { notice = errorMessage(error); }
  }

  /** Track native backup work in the exit barrier while leaving typing available during snapshot creation. */
  function runBackup(action: () => Promise<void>) {
    if (backupPending || frozen || loading || recovery || annotation) return;
    backupPending = true;
    backupWork = Promise.resolve().then(action).catch(error => { notice = errorMessage(error); }).finally(() => {
      backupPending = false; backupWork = undefined;
    });
  }

  /** Flush existing generations before opening the export picker; SQLite snapshots stay consistent during further typing. */
  function exportWorkspaceBackup() {
    runBackup(async () => {
      if (!saver) throw new Error('Load the workspace before exporting a backup');
      await saver.flush();
      notice = await storage.exportBackup() ? 'Workspace backup exported.' : 'Backup export cancelled.';
    });
  }

  /** A privately staged backup remains unchanged while the user reads the replacement warning and continues editing. */
  function selectWorkspaceBackup() {
    runBackup(async () => {
      restorePreview = null;
      restorePreview = await storage.prepareRestore();
      if (!restorePreview) notice = 'Backup selection cancelled.';
    });
  }

  /** Discard native candidate resources as well as hiding the confirmation. */
  function cancelWorkspaceRestore() {
    runBackup(async () => { await storage.cancelRestore(); restorePreview = null; });
  }

  /** Replace only after all in-flight editor operations and dirty generations settle; failure keeps current caches intact. */
  function confirmWorkspaceRestore() {
    if (!restorePreview) return;
    const token = restorePreview.token;
    runBackup(async () => {
      setFrozen(true);
      try {
        await insertion; await imageOutput?.catch(() => {}); await transition;
        if (saver) { stageSession(); await saver.flush(); }
        try { await storage.restoreBackup(token); }
        finally { restorePreview = null; }
        // Every cached revision, undo state, and original belongs to the replaced workspace and must be released.
        saver?.stop(); saver = undefined;
        view?.scrollDOM.removeEventListener('scroll', stageSession);
        view?.destroy(); view = undefined; note = undefined;
        notes.clear(); states.clear(); automatic.clear(); sources.clear();
        titles = {}; library = []; closed = []; session = emptySession(); purge = undefined; renaming = false;
        await loadWorkspace();
        notice = 'Workspace restored. A pre-restore copy remains in the backup directory.';
      } finally { if (!exit.busy) setFrozen(false); }
    });
  }
</script>

<svelte:head><title>Simple Note</title></svelte:head>

<main class="flex h-dvh flex-col bg-[#1e1e1e] text-sm text-[#d4d4d4]" aria-busy={loading || frozen}>
  <header class="flex flex-wrap items-center gap-3 border-b border-[#333] bg-[#252526] px-4 py-2">
    <h1 class="mr-auto font-semibold">Simple Note</h1>
    <button type="button" disabled={loading || frozen || busy || !!startupError} onclick={() => runTab(newNote)}>New note</button>
    <button type="button" disabled={loading || frozen || busy || !closed.length} onclick={() => runTab(reopenNote)}>Reopen closed tab</button>
    <button type="button" aria-pressed={session.sidebarVisible} disabled={loading || frozen || busy} onclick={() => runTab(toggleSidebar)}>Sidebar</button>
    <fieldset class="contents" disabled={loading || frozen || inserting || busy || !session.activeNoteId || !!startupError} aria-label="Editor tools">
      <label>Language <select class="bg-[#333] px-2 py-1" bind:value={language} onchange={changeLanguage}>
        {#each Object.entries(languageNames) as [value, name]}<option {value}>{name}</option>{/each}
      </select></label>
      <button type="button" aria-pressed={wrap} onclick={toggleWrap}>Wrap: {wrap ? 'On' : 'Off'}</button>
      <button type="button" onclick={() => view && undo(view)}>Undo</button>
      <button type="button" onclick={() => view && redo(view)}>Redo</button>
      <button type="button" disabled={inserting} onclick={() => pasteScreenshot()}>{inserting ? 'Adding image…' : 'Paste screenshot'}</button>
      <button type="button" onclick={() => moveSelection(-1)}>Move selection left</button>
      <button type="button" onclick={() => moveSelection(1)}>Move selection right</button>
    </fieldset>
    <button type="button" disabled={frozen} onclick={() => storage.requestExit().catch(error => { saveError = errorMessage(error); })}>Quit</button>
  </header>
  <details class="border-b border-[var(--border)] px-4 py-2">
    <summary>Backup and restore</summary>
    <p>Local SQLite backups include all notes, Trash, images, annotations, and tabs.</p>
    <p class="break-all">Automatic backups: {backupStatus.directory || 'Loading backup location…'}</p>
    {#if backupStatus.lastBackup}<p class="break-all">Latest automatic snapshot: {backupStatus.lastBackup}</p>{/if}
    <button type="button" disabled={loading || frozen || backupPending || !!startupError} onclick={exportWorkspaceBackup}>Export workspace backup</button>
    <button type="button" disabled={loading || frozen || backupPending} onclick={selectWorkspaceBackup}>Select backup to restore…</button>
    {#if backupPending}<p role="status">Working on backup or restore…</p>{/if}
    {#if restorePreview}
      <div role="alert" class="my-2 border border-[#a66] p-2">
        <p>Replace all local notes, Trash, images, and tabs with this backup ({restorePreview.notes} notes, {restorePreview.trashed} in Trash)? This does not merge workspaces.</p>
        <p>Current edits will be saved first. A pre-restore backup is kept. If the current store is unreadable, its original database files are preserved instead.</p>
        <button type="button" disabled={loading || frozen || backupPending} onclick={confirmWorkspaceRestore}>Confirm replacement and restore</button>
        <button type="button" disabled={frozen || backupPending} onclick={cancelWorkspaceRestore}>Cancel restore</button>
      </div>
    {/if}
  </details>
  {#if backupStatus.error}<div role="alert" class="bg-[#522] px-4 py-2">Automatic backup failed: {backupStatus.error}. Acknowledged saves remain intact. The next successful write retries the backup.</div>{/if}
  {#if startupError}
    <div class="flex flex-wrap gap-3 bg-[#522] px-4 py-2" role="alert">
      <span>Could not load notes: {startupError}</span>
      <button type="button" disabled={loading || frozen} onclick={retryStartup}>Retry loading</button>
      <button type="button" disabled={loading || frozen || backupPending} onclick={selectWorkspaceBackup}>Recover from a backup…</button>
      <button type="button" disabled={loading || frozen} onclick={() => { confirmDiscard = true; }}>Quit without changes</button>
    </div>
  {/if}
  {#if saveError}
    <div class="flex flex-wrap gap-3 bg-[#522] px-4 py-2" role="alert">
      <span>Could not save: {saveError}. Your current work remains open.</span>
      <button type="button" disabled={frozen} onclick={retrySave}>Retry saving</button>
      <button type="button" disabled={frozen} onclick={exportCurrentRecovery}>Export recovery</button>
      <button type="button" disabled={frozen} onclick={() => { confirmDiscard = true; }}>Quit without saving</button>
    </div>
  {/if}
  {#if confirmDiscard}
    <div class="flex flex-wrap gap-3 bg-[#522] px-4 py-2" role="alert">
      <span>Discard all unsaved changes and quit? Only acknowledged saves will remain.</span>
      <button type="button" disabled={frozen} onclick={() => { confirmDiscard = false; void exit.request(true); }}>Confirm discard and quit</button>
      <button type="button" disabled={frozen} onclick={() => { confirmDiscard = false; }}>Keep editing</button>
    </div>
  {/if}
  <div class="flex overflow-x-auto border-b border-[var(--border)] bg-[var(--surface)]" role="tablist" aria-label="Open notes">
    {#each session.tabs as tab, index (tab.noteId)}
      <div class="flex shrink-0 items-center border-r border-[var(--border)]">
        <button id={`tab-${tab.noteId}`} type="button" role="tab" aria-selected={session.activeNoteId === tab.noteId} aria-controls="note-panel"
          tabindex={session.activeNoteId === tab.noteId ? 0 : -1} disabled={frozen || busy || loading}
          class="max-w-48 truncate px-3 py-2 aria-selected:bg-[var(--editor)] aria-selected:text-white"
          onkeydown={event => tabKey(event, index)} onclick={() => runTab(() => mountNote(tab.noteId))}>{tab.pinned ? '◆ ' : ''}{titles[tab.noteId] ?? 'Untitled'}</button>
        <button type="button" class="px-1" aria-label={`Close ${titles[tab.noteId]}`} disabled={frozen || busy || loading} onclick={() => runTab(() => closeNote(tab.noteId))}>×</button>
      </div>
    {/each}
  </div>
  {#if session.activeNoteId}
    <div class="flex flex-wrap items-center gap-3 px-4 py-1">
      <button type="button" disabled={frozen || busy} onclick={() => { renameValue = note?.title ?? ''; renaming = true; }}>Rename</button>
      <button type="button" disabled={frozen || busy} aria-pressed={session.tabs.find(tab => tab.noteId === session.activeNoteId)?.pinned ?? false} onclick={() => alterTab(session.activeNoteId!, 'pin')}>Pin tab</button>
      <button type="button" disabled={frozen || busy} onclick={() => alterTab(session.activeNoteId!, -1)}>Move tab left</button>
      <button type="button" disabled={frozen || busy} onclick={() => alterTab(session.activeNoteId!, 1)}>Move tab right</button>
      {#if renaming}<form onsubmit={renameNote}><label>Note title <input class="bg-[#333] px-2" bind:value={renameValue} disabled={frozen || busy} /></label><button type="submit" disabled={frozen || busy}>Apply title</button><button type="button" onclick={() => { renaming = false; }}>Cancel</button></form>{/if}
    </div>
  {/if}
  <form class="flex items-center gap-3 border-b border-[#333] px-4 py-2" onsubmit={find}>
    <label for="find">Find</label><input id="find" class="min-w-0 flex-1 bg-[#333] px-2 py-1" bind:value={query} disabled={loading || frozen || inserting || busy || !session.activeNoteId || !!startupError} />
    <button type="submit" disabled={loading || frozen || inserting || busy || !session.activeNoteId || !!startupError}>Find next</button>
  </form>
  <div class="flex min-h-0 flex-1">
    {#if session.sidebarVisible}<aside class="w-52 shrink-0 overflow-y-auto border-r border-[var(--border)] bg-[var(--surface)] p-3" aria-label="Notes">
      <fieldset disabled={loading || frozen || busy || !!annotation}>
        <legend>Note library</legend>
        <button type="button" aria-pressed={!trash} onclick={() => runTab(async () => { trash = false; await browseLibrary(); })}>All Notes</button>
        <button type="button" aria-pressed={trash} onclick={() => runTab(async () => { trash = true; await browseLibrary(); })}>Trash</button>
        <form onsubmit={event => { event.preventDefault(); runTab(() => browseLibrary()); }}>
          <label>Search titles and text<input class="w-full bg-[#333] px-2" bind:value={libraryQuery} /></label>
          <button type="submit">Search / Refresh</button>
        </form>
        {#each library as item (item.id)}
          <div class="border-b border-[var(--border)] py-2">
            {#if trash}
              <p class="break-words">{item.title}</p>
              <button type="button" onclick={() => runTab(() => lifecycle(item, 'restore'))}>Restore</button>
              <button type="button" onclick={() => { purge = item; }}>Delete permanently…</button>
            {:else}
              <button type="button" class="block w-full truncate text-left" onclick={() => runTab(() => mountNote(item.id))}>{titles[item.id] ?? item.title}</button>
              <button type="button" onclick={() => runTab(async () => { await mountNote(item.id); renameValue = note?.title ?? ''; renaming = true; })}>Rename</button>
              <button type="button" onclick={() => runTab(() => lifecycle(item, 'trash'))}>Move to Trash</button>
            {/if}
          </div>
        {/each}
        {#if !library.length}<p>Choose All Notes or Trash to load notes. No results? Change the search.</p>{/if}
        {#if libraryMore}<button type="button" onclick={() => runTab(() => browseLibrary(true))}>Load more</button>{/if}
        {#if purge}<div role="alert"><p>Permanently delete “{purge.title}”? This cannot be undone.</p><button type="button" onclick={() => runTab(() => lifecycle(purge!, 'purge'))}>Confirm permanent deletion</button><button type="button" onclick={() => { purge = undefined; }}>Cancel</button></div>{/if}
      </fieldset>
    </aside>{/if}
    <div id="note-panel" role="tabpanel" tabindex="0" aria-label="Active note" class="relative min-h-0 min-w-0 flex-1">
      <div class="h-full" class:hidden={!session.activeNoteId} bind:this={host}></div>
      {#if !loading && !startupError && !session.activeNoteId}<div class="flex h-full flex-col items-center justify-center gap-3"><p>All tabs closed. Your notes are preserved.</p><button type="button" disabled={frozen || busy} onclick={() => runTab(newNote)}>Create a note</button><p class="text-xs">Open a note from the sidebar or reopen the last closed tab.</p></div>{/if}
    </div>
  </div>
  <footer class="bg-[#252526] px-4 py-1 text-xs text-[#b5b5b5]" role="status" aria-live="polite">
    {loading ? 'Loading notes…' : frozen ? 'Finishing current work…' : startupError ? 'Notes unavailable' : saveStatus === 'Error' ? 'Save failed — retry or export recovery' : saveStatus}
    {#if notice}<span class="ml-3">{notice}</span>{/if}
  </footer>
</main>
{#if annotation}
  <AnnotationEditor block={annotation.block} blob={annotation.blob} status={saveError ? `Could not save: ${saveError}. Your work remains open.` : saveStatus}
    onoutput={outputAnnotation}
    onchange={annotations => { if (view && annotation) view.dispatch(annotateImage(view.state, annotation.block.id, annotations)); }}
    onclose={closeAnnotation} />
{/if}

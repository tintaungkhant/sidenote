<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import type { Annotation, ImageBlock } from './document';
  import { AnnotationHistory, annotationKey, annotationSvg, imagePoint, translate, type Point, type Tool } from './annotations';
  let { block, blob, status, onchange, onclose }: { block: ImageBlock; blob: Blob; status: string; onchange: (annotations: readonly Annotation[]) => void; onclose: () => void } = $props();
  let annotations = $state.raw<readonly Annotation[]>(untrack(() => block.annotations));
  const operations = new AnnotationHistory(untrack(() => annotations));
  let tool = $state<Tool>('select'), color = $state('#ff4444'), width = $state(4);
  let selected = $state(-1), imageWidth = $state(0), imageHeight = $state(0), url = $state('');
  let surface = $state<SVGSVGElement>(null!), dialog: HTMLDialogElement;
  let gesture = $state.raw<{ start: Point; original?: Annotation; draft?: Annotation; pointer: number } | undefined>();
  let textDraft = $state('');
  let validation = $state('');
  const tools: Tool[] = ['select', 'pen', 'highlighter', 'arrow', 'text'];

  /** The focused view owns its decoded image URL and restores note focus through the caller. */
  onMount(() => { url = URL.createObjectURL(blob); dialog.showModal(); dialog.focus(); return () => { dialog.close(); URL.revokeObjectURL(url); }; });

  /** Each completed operation is immutable, immediately saved, and independently reversible. */
  function commit(next: readonly Annotation[]) {
    annotations = operations.commit(next); onchange(annotations);
  }
  /** Local history keeps annotation shortcuts independent of the note editor. */
  function history(redo = false) {
    gesture = undefined;
    const next = operations.step(redo);
    if (next === annotations) return;
    annotations = next; selected = -1; onchange(next);
  }
  /** Selection also seeds the explicit text editor without changing persisted text. */
  function select(index: number) { selected = index; validation = ''; const a = annotations[index]; textDraft = a?.kind === 'text' ? a.text : ''; }
  /** Replace one object while keeping all other placements and objects untouched. */
  function replace(annotation: Annotation) { commit(annotations.map((a, i) => i === selected ? annotation : a)); }
  /** A center placement gives keyboard-only users the same editable text creation path. */
  function addText(p: Point = { x: imageWidth / 2, y: imageHeight / 2 }) {
    if (!imageWidth || annotations.length >= 10000) return;
    commit([...annotations, { kind: 'text', ...p, text: 'Text', color, fontSize: 24 }]); select(annotations.length - 1);
  }
  /** Pointer coordinates always use the actual rendered image rectangle, never device pixels. */
  function point(event: PointerEvent) { return imagePoint(event.clientX, event.clientY, surface.getBoundingClientRect(), imageWidth, imageHeight); }
  /** Begin a captured gesture or create a keyboard-editable text object at the chosen image pixel. */
  function start(event: PointerEvent) {
    if (event.button !== 0 || !imageWidth || gesture) return;
    event.preventDefault(); surface.focus();
    const p = point(event);
    if (tool === 'select') {
      const element = (event.target as Element).closest('[data-annotation]');
      select(element ? Number(element.getAttribute('data-annotation')) : -1);
      if (selected < 0) return;
      gesture = { start: p, original: annotations[selected], pointer: event.pointerId };
    } else if (tool === 'text') {
      addText(p);
      return;
    } else {
      if (annotations.length >= 10000) return;
      select(-1);
      gesture = { start: p, draft: { kind: tool, points: [p, p], color, width: tool === 'highlighter' ? width * 4 : width }, pointer: event.pointerId };
    }
    surface.setPointerCapture(event.pointerId);
  }
  /** Preview only; pointer cancellation or exit discards this unfinished operation. */
  function move(event: PointerEvent) {
    if (!gesture || event.pointerId !== gesture.pointer) return;
    const p = point(event);
    if (gesture.original) gesture = { ...gesture, draft: translate(gesture.original, p.x - gesture.start.x, p.y - gesture.start.y, imageWidth, imageHeight) };
    else if (gesture.draft && gesture.draft.kind !== 'text') {
      const a = gesture.draft;
      gesture = { ...gesture, draft: { ...a, points: a.kind === 'arrow' ? [gesture.start, p] : a.points.length < 100000 ? [...a.points, p] : a.points } };
    }
  }
  /** Persist only a completed gesture, releasing capture even when it makes no changes. */
  function finish(event: PointerEvent) {
    if (!gesture || event.pointerId !== gesture.pointer) return;
    move(event);
    const { draft, original } = gesture; gesture = undefined;
    if (surface.hasPointerCapture(event.pointerId)) surface.releasePointerCapture(event.pointerId);
    if (draft) { if (original) replace(draft); else { commit([...annotations, draft]); select(annotations.length - 1); } }
  }
  /** Keyboard actions never bubble into note-level history, tab, or quit shortcuts. */
  function key(event: KeyboardEvent) {
    event.stopPropagation();
    const editing = event.target instanceof HTMLTextAreaElement || event.target instanceof HTMLInputElement || event.target instanceof HTMLSelectElement;
    const action = annotationKey(event, editing);
    if (!action) return;
    // Tab remains native in the toolbar and text field; the canvas cycles annotation selection.
    if (action === 'cycle' && event.target !== surface) return;
    event.preventDefault();
    if (action === 'exit') { gesture = undefined; onclose(); }
    else if (action === 'undo' || action === 'redo') history(action === 'redo');
    else if (tools.includes(action as Tool)) tool = action as Tool;
    else if (action === 'cycle' && annotations.length) select((selected + (event.shiftKey ? annotations.length - 1 : 1) + annotations.length) % annotations.length);
    else if (action === 'delete' && selected >= 0) { commit(annotations.filter((_, i) => i !== selected)); select(-1); }
    else if (selected >= 0 && ['left', 'right', 'up', 'down'].includes(action)) {
      const step = event.shiftKey ? 10 : 1;
      replace(translate(annotations[selected], action === 'left' ? -step : action === 'right' ? step : 0, action === 'up' ? -step : action === 'down' ? step : 0, imageWidth, imageHeight));
    }
  }
  /** Each input event is already completed work; closing never loses text still in the field. */
  function editText() {
    const a = annotations[selected];
    if (a?.kind !== 'text') return;
    if (new TextEncoder().encode(textDraft).length > 65536) {
      textDraft = a.text; validation = 'Text is limited to 64 KiB per annotation.'; return;
    }
    validation = ''; replace({ ...a, text: textDraft });
  }
  let preview = $derived(gesture?.draft ? gesture.original ? annotations.map((a, i) => i === selected ? gesture!.draft! : a) : [...annotations, gesture.draft] : annotations);
</script>

<dialog bind:this={dialog} aria-label="Image annotations" tabindex="-1" onkeydown={key} oncancel={event => { event.preventDefault(); onclose(); }}
  class="fixed inset-0 m-0 flex h-dvh max-h-none w-screen max-w-none flex-col overflow-hidden border-0 bg-[var(--editor)] p-0 text-[#d4d4d4]">
  <div class="flex flex-wrap items-center gap-3 border-b border-[var(--border)] p-2">
    {#each tools as item}<button type="button" aria-pressed={tool === item} onclick={() => { gesture = undefined; tool = item; }}>{item}</button>{/each}
    <label>Color <select bind:value={color}>{#each ['#ff4444', '#ffff00', '#44cc66', '#3399ff', '#ffffff', '#000000'] as value}<option {value}>{value}</option>{/each}</select></label>
    <label>Width <select bind:value={width}>{#each [2, 4, 8, 12] as value}<option {value}>{value}</option>{/each}</select></label>
    <button type="button" onclick={() => history()}>Undo</button><button type="button" onclick={() => history(true)}>Redo</button>
    <button type="button" disabled={!imageWidth} onclick={() => addText()}>Add text at center</button>
    <button type="button" disabled={selected < 0} onclick={() => { commit(annotations.filter((_, i) => i !== selected)); select(-1); }}>Delete</button>
    <button type="button" onclick={onclose}>Done (Esc)</button>
  </div>
  <p class="px-2 text-xs">V select · P pen · H highlighter · A arrow · T text. Canvas: Tab selects, arrows move, Delete removes.</p>
  <p class="px-2 text-xs" role="status">{status}</p>
  {#if validation}<p class="px-2 text-xs" role="alert">{validation}</p>{/if}
  {#if annotations[selected]?.kind === 'text'}
    <label class="flex gap-2 p-2">Text <textarea class="flex-1 bg-[var(--surface)]" aria-label="Annotation text" bind:value={textDraft} oninput={editText}></textarea></label>
  {/if}
  <div class="min-h-0 flex-1 overflow-auto p-4">
    <div class="relative mx-auto w-fit max-w-full">
      <img src={url || undefined} alt="Original screenshot for annotation" draggable="false" class="block max-h-[65vh] max-w-full"
        onload={event => { const image = event.currentTarget as HTMLImageElement; imageWidth = image.naturalWidth; imageHeight = image.naturalHeight; }} />
      {#if imageWidth}
        <!-- The application canvas implements selection, movement, and deletion through its keyboard handler. -->
        <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
        <svg bind:this={surface} viewBox={`0 0 ${imageWidth} ${imageHeight}`} class="absolute inset-0 h-full w-full touch-none" tabindex="0" role="application" aria-label="Annotation canvas"
          onpointerdown={start} onpointermove={move} onpointerup={finish} onpointercancel={() => { gesture = undefined; }} onlostpointercapture={() => { gesture = undefined; }}>
          {@html annotationSvg(preview, selected)}
        </svg>
      {/if}
    </div>
  </div>
</dialog>

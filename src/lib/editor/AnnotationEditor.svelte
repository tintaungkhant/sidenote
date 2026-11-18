<script lang="ts">
  import { onMount, untrack, tick } from 'svelte';
  import { errorMessage } from '$lib/autosave';
  import type { Annotation, ImageBlock } from './document';
  import { AnnotationHistory, MIN_ZOOM, MAX_ZOOM, annotationKey, annotationSvg, imagePoint, translate, annotationBounds, resizeAnnotation, clampViewport, fitViewport, zoomAtPoint, resetViewport, stepZoom, applyWheel, type Point, type Tool, type Viewport } from './annotations';
  let { block, blob, status, onchange, onclose, onoutput }: { block: ImageBlock; blob: Blob; status: string; onchange: (annotations: readonly Annotation[]) => void; onclose: () => void;
    onoutput: (action: 'copy' | 'export', annotations: readonly Annotation[], width: number, height: number) => Promise<string> } = $props();
  let annotations = $state.raw<readonly Annotation[]>(untrack(() => block.annotations));
  const operations = new AnnotationHistory(untrack(() => annotations));
  let tool = $state<Tool>('select'), color = $state('#ff4444'), width = $state(4);
  let selected = $state(-1), imageWidth = $state(0), imageHeight = $state(0), url = $state('');
  let surface = $state<SVGSVGElement>(null!), stage = $state<HTMLDivElement>(null!), dialog: HTMLDialogElement;
  let stageWidth = $state(0), stageHeight = $state(0);
  let viewport = $state<Viewport>({ zoom: 1, x: 0, y: 0 });
  let fitted = $state(true), spaceHeld = $state(false);
  let panGesture = $state.raw<{ pointer: number; startX: number; startY: number; originX: number; originY: number } | undefined>();
  let gesture = $state.raw<{ start: Point; original?: Annotation; draft?: Annotation; pointer: number; resize?: boolean } | undefined>();
  // Track completed clicks before SVG capture/selection redraws can discard the native dblclick target.
  let captionClick: { index: number; time: number; x: number; y: number } | undefined;
  let pointerOrigin: { x: number; y: number } | undefined;
  let textDraft = $state('');
  let editingCaption = $state(false);
  let captionInput = $state<HTMLTextAreaElement>();
  let captionDraft = $state('');
  const captionIndex = $derived(annotations.findIndex(a => a.kind === 'caption'));
  const hasCaption = $derived(captionIndex >= 0 && !!(annotations[captionIndex] as Extract<Annotation, { kind: 'caption' }>).text);
  let editingText = $state(false);
  let textInput = $state<HTMLTextAreaElement>();
  let validation = $state('');
  let outputBusy = $state(false), outputMessage = $state('');
  const tools: Tool[] = ['select', 'pen', 'highlighter', 'arrow', 'rectangle', 'circle', 'text'];
  const toolNames: Record<Tool, string> = { select: 'Select', pen: 'Pen', highlighter: 'Highlight', arrow: 'Arrow', rectangle: 'Rectangle', circle: 'Ellipse', text: 'Text' };
  const toolKeys: Record<Tool, string> = { select: 'V', pen: 'P', highlighter: 'H', arrow: 'A', rectangle: 'R', circle: 'C', text: 'T' };
  const toolIcons: Record<Tool, string> = {
    select: 'M5 3l13 9-6 1-3 6z', pen: 'M4 20l4-1L20 7l-4-4L4 15zM14 5l4 4',
    highlighter: 'M5 14l8-10 6 5-8 10zM3 21h12', arrow: 'M4 20L20 4M8 4h12v12', rectangle: 'M4 5h16v14H4z', circle: 'M12 3a9 9 0 1 0 0 18 9 9 0 1 0 0-18', text: 'M4 5h16M12 5v15M8 20h8'
  };
  const colors = [ { value: '#ff4444', name: 'Red' }, { value: '#ffff00', name: 'Yellow' },
    { value: '#44cc66', name: 'Green' }, { value: '#3399ff', name: 'Blue' },
    { value: '#ffffff', name: 'White' }, { value: '#000000', name: 'Black' } ];

  /** The focused view owns its decoded image URL, Figma-style wheel/resize listeners, and focus lifecycle. */
  onMount(() => {
    url = URL.createObjectURL(blob);
    dialog.showModal();
    dialog.focus();
    stageWidth = stage.clientWidth;
    stageHeight = stage.clientHeight;
    const observer = new ResizeObserver(() => {
      if (!stage) return;
      const nextW = stage.clientWidth, nextH = stage.clientHeight;
      if (nextW === stageWidth && nextH === stageHeight) return;
      const prevW = stageWidth, prevH = stageHeight;
      stageWidth = nextW; stageHeight = nextH;
      if (!imageWidth) return;
      if (fitted) viewport = fitViewport(nextW, nextH, imageWidth, imageHeight, true);
      else if (prevW && prevH) viewport = clampViewport({ ...viewport, x: viewport.x + (nextW - prevW) / 2, y: viewport.y + (nextH - prevH) / 2 }, nextW, nextH, imageWidth, imageHeight);
    });
    observer.observe(stage);
    const onWheel = (event: WheelEvent) => {
      const inStage = stage?.contains(event.target as Node);
      if (event.ctrlKey || event.metaKey) event.preventDefault();
      if (!inStage || outputBusy || !imageWidth) return;
      if (!event.ctrlKey && !event.metaKey && event.target instanceof HTMLTextAreaElement && event.target.scrollHeight > event.target.clientHeight) return;
      event.preventDefault();
      const rect = stage.getBoundingClientRect();
      fitted = false;
      viewport = applyWheel(viewport, event, event.clientX - rect.left, event.clientY - rect.top, rect.width, rect.height, imageWidth, imageHeight);
    };
    const onKeyUp = (event: KeyboardEvent) => {
      if (event.code === 'Space' || event.key === ' ') spaceHeld = false;
    };
    const onWindowBlur = () => { spaceHeld = false; panGesture = undefined; };
    dialog.addEventListener('wheel', onWheel, { passive: false });
    window.addEventListener('keyup', onKeyUp);
    window.addEventListener('blur', onWindowBlur);
    return () => {
      observer.disconnect();
      dialog.removeEventListener('wheel', onWheel);
      window.removeEventListener('keyup', onKeyUp);
      window.removeEventListener('blur', onWindowBlur);
      dialog.close();
      URL.revokeObjectURL(url);
    };
  });

  function zoomBy(direction: 1 | -1) {
    if (!imageWidth || !stageWidth) return;
    fitted = false;
    viewport = zoomAtPoint(viewport, stepZoom(viewport.zoom, direction), stageWidth / 2, stageHeight / 2, stageWidth, stageHeight, imageWidth, imageHeight);
  }
  function zoomToFit(capAtActualSize = false) {
    if (!imageWidth || !stageWidth) return;
    fitted = capAtActualSize;
    viewport = fitViewport(stageWidth, stageHeight, imageWidth, imageHeight, capAtActualSize);
  }
  function zoomToActual() {
    if (!imageWidth || !stageWidth) return;
    fitted = false;
    viewport = resetViewport(viewport, stageWidth, stageHeight, imageWidth, imageHeight);
  }

  /** Space+drag or middle-mouse drag pans the viewport regardless of active tool, matching Figma. */
  function startPanCapture(event: PointerEvent) {
    if (outputBusy || !imageWidth || gesture || panGesture) return;
    if (event.button === 1 || (event.button === 0 && spaceHeld)) {
      event.preventDefault();
      event.stopPropagation();
      panGesture = { pointer: event.pointerId, startX: event.clientX, startY: event.clientY, originX: viewport.x, originY: viewport.y };
      stage.setPointerCapture(event.pointerId);
    }
  }
  function startStage(event: PointerEvent) {
    if (outputBusy || !imageWidth || gesture || panGesture) return;
    if (event.button === 0 && event.target === stage) {
      editingCaption = false;
      editingText = false;
      select(-1);
      surface?.focus();
    }
  }
  function moveStage(event: PointerEvent) {
    if (!panGesture || event.pointerId !== panGesture.pointer) return;
    fitted = false;
    viewport = clampViewport({
      ...viewport,
      x: panGesture.originX + (event.clientX - panGesture.startX),
      y: panGesture.originY + (event.clientY - panGesture.startY)
    }, stageWidth, stageHeight, imageWidth, imageHeight);
  }
  function finishStage(event: PointerEvent) {
    if (!panGesture || event.pointerId !== panGesture.pointer) return;
    panGesture = undefined;
    if (stage.hasPointerCapture(event.pointerId)) stage.releasePointerCapture(event.pointerId);
  }

  /** Each completed operation is immutable, immediately saved, and independently reversible. */
  function commit(next: readonly Annotation[]) {
    annotations = operations.commit(next); onchange(annotations);
  }
  /** Local history keeps annotation shortcuts independent of the note editor. */
  function history(redo = false) {
    gesture = undefined; editingText = false; editingCaption = false;
    const next = operations.step(redo);
    if (next === annotations) return;
    annotations = next; selected = -1; onchange(next);
  }
  /** Selection also seeds the explicit text editor without changing persisted text. */
  function select(index: number) { editingText = false; selected = index; validation = ''; const a = annotations[index]; textDraft = a?.kind === 'text' ? a.text : ''; }
  /** Replace one object while keeping all other placements and objects untouched. */
  function replace(annotation: Annotation) { commit(annotations.map((a, i) => i === selected ? annotation : a)); }
  /** A center placement gives keyboard-only users the same editable text creation path. */
  function addText(p: Point = { x: imageWidth / 2, y: imageHeight / 2 }) {
    if (!imageWidth || annotations.length >= 10000) return;
    commit([...annotations, { kind: 'text', x: Math.min(p.x, Math.max(0, imageWidth - 24)), y: Math.min(p.y, Math.max(0, imageHeight - 30)), text: '', color, fontSize: 24 }]); select(annotations.length - 1);
    void beginTextEditing();
  }
  async function openCaption() {
    if (!imageWidth || outputBusy) return;
    select(-1); gesture = undefined; tool = 'select';
    captionDraft = captionIndex < 0 ? '' : (annotations[captionIndex] as Extract<Annotation, { kind: 'caption' }>).text;
    editingCaption = true;
    await tick(); captionInput?.focus();
  }
  function saveCaption() {
    if (new TextEncoder().encode(captionDraft).length > 65536) {
      validation = 'Caption is limited to 64 KiB.'; return;
    }
    validation = '';
    if (captionIndex < 0) {
      if (!captionDraft || annotations.length >= 10000) return;
      commit([...annotations, { kind: 'caption', text: captionDraft }]);
    } else if (captionDraft) {
      commit(annotations.map((a, i) => i === captionIndex ? { kind: 'caption', text: captionDraft } : a));
    } else commit(annotations.filter((_, i) => i !== captionIndex));
  }
  /** Focus the caption at its image position; native text editing retains caret, selection, multiline input, and IME. */
  async function beginTextEditing() {
    if (annotations[selected]?.kind !== 'text' || outputBusy) return;
    gesture = undefined; editingText = true; tool = 'select';
    await tick();
    textInput?.focus();
  }

  /** Double-clicking an existing caption enters the same in-place editor used after placement. */
  function editCaption(event: MouseEvent) {
    if (spaceHeld || panGesture) return;
    const element = (event.target as Element).closest('[data-annotation]');
    if (outputBusy || editingText) return;
    const index = element ? Number(element.getAttribute('data-annotation')) : selected;
    if (annotations[index]?.kind === 'caption') { void openCaption(); return; }
    if (annotations[index]?.kind !== 'text') return;
    select(index); void beginTextEditing();
  }

  /** Pointer coordinates always use the actual rendered image rectangle, never device pixels. */
  function point(event: PointerEvent) { return imagePoint(event.clientX, event.clientY, surface.getBoundingClientRect(), imageWidth, imageHeight); }
  /** Begin a captured gesture or create a keyboard-editable text object at the chosen image pixel. */
  function start(event: PointerEvent) {
    if (outputBusy || spaceHeld || panGesture || event.button !== 0 || !imageWidth || gesture || (event.target as Element).closest('textarea')) return;
    const target = (event.target as Element).closest('[data-annotation]');
    if (target && annotations[Number(target.getAttribute('data-annotation'))]?.kind === 'caption') return;
    editingCaption = false;
    event.preventDefault(); surface.focus();
    const p = point(event);
    pointerOrigin = { x: event.clientX, y: event.clientY };
    const element = (event.target as Element).closest('[data-annotation]');
    // Visible selection controls edit their object even while a drawing tool remains active.
    if (tool === 'select' || (element && Number(element.getAttribute('data-annotation')) === selected)) {
      select(element ? Number(element.getAttribute('data-annotation')) : -1);
      if (selected < 0) return;
      gesture = { start: p, original: annotations[selected], pointer: event.pointerId, resize: !!(event.target as Element).closest('[data-resize]') };
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
    if (gesture.original) gesture = { ...gesture, draft: gesture.resize ? resizeAnnotation(gesture.original, p, imageWidth, imageHeight) : translate(gesture.original, p.x - gesture.start.x, p.y - gesture.start.y, imageWidth, imageHeight) };
    else if (gesture.draft && gesture.draft.kind !== 'text' && gesture.draft.kind !== 'caption') {
      const a = gesture.draft;
      gesture = { ...gesture, draft: { ...a, points: a.kind === 'arrow' || a.kind === 'rectangle' || a.kind === 'circle' ? [gesture.start, p] : a.points.length < 100000 ? [...a.points, p] : a.points } };
    }
  }
  /** Persist only a completed gesture, releasing capture even when it makes no changes. */
  function finish(event: PointerEvent) {
    if (!gesture || event.pointerId !== gesture.pointer) return;
    const click = !!gesture.original && !gesture.resize && !!pointerOrigin && Math.hypot(event.clientX - pointerOrigin.x, event.clientY - pointerOrigin.y) <= 4;
    if (!click) move(event);
    const { draft, original } = gesture; gesture = undefined;
    if (surface.hasPointerCapture(event.pointerId)) surface.releasePointerCapture(event.pointerId);
    if (click && original?.kind === 'text') {
      const previous = captionClick;
      captionClick = { index: selected, time: event.timeStamp, x: event.clientX, y: event.clientY };
      if (previous && previous.index === selected && event.timeStamp - previous.time <= 500 &&
          Math.hypot(event.clientX - previous.x, event.clientY - previous.y) <= 4) {
        captionClick = undefined; void beginTextEditing();
      }
      return;
    }
    captionClick = undefined;
    // A selection click is not a completed move and must not replace SVG nodes or create undo entries.
    if (!click && draft) { if (original) replace(draft); else { commit([...annotations, draft]); select(annotations.length - 1); } }
  }

  /** Keyboard actions never bubble into note-level history, tab, or quit shortcuts. */
  function key(event: KeyboardEvent) {
    event.stopPropagation();
    if (outputBusy) { if (event.key !== 'Tab') event.preventDefault(); return; }
    if (editingCaption && event.target === captionInput && event.key === 'Enter' && !event.shiftKey && !event.isComposing) {
      event.preventDefault(); editingCaption = false; surface?.focus(); return;
    }
    if (editingCaption && event.key === 'Escape' && !event.isComposing) {
      event.preventDefault(); editingCaption = false; surface?.focus(); return;
    }
    if (editingText && event.target === textInput && event.key === 'Escape' && !event.isComposing) {
      event.preventDefault(); editingText = false; surface?.focus(); return;
    }
    if (!editingText && event.target === surface && event.key === 'Enter' && annotations[selected]?.kind === 'caption') {
      event.preventDefault(); void openCaption(); return;
    }
    if (!editingText && event.target === surface && event.key === 'Enter' && annotations[selected]?.kind === 'text') {
      event.preventDefault(); void beginTextEditing(); return;
    }
    const editing = event.target instanceof HTMLTextAreaElement || event.target instanceof HTMLInputElement || event.target instanceof HTMLSelectElement;
    if ((event.code === 'Space' || event.key === ' ') && !editing && !event.isComposing && !(event.target instanceof HTMLButtonElement)) {
      event.preventDefault();
      spaceHeld = true;
      return;
    }
    const action = annotationKey(event, editing);
    if (!action) return;
    // Tab remains native in the toolbar and text field; the canvas cycles annotation selection.
    if (action === 'cycle' && event.target !== surface) return;
    event.preventDefault();
    if (action === 'exit') { gesture = undefined; panGesture = undefined; onclose(); }
    else if (action === 'undo' || action === 'redo') history(action === 'redo');
    else if (action === 'zoom-in') zoomBy(1);
    else if (action === 'zoom-out') zoomBy(-1);
    else if (action === 'zoom-reset') zoomToActual();
    else if (action === 'zoom-fit') zoomToFit(false);
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
  /** Only completed operations enter output; failure/cancellation leaves source and annotation history untouched. */
  async function output(action: 'copy' | 'export') {
    if (outputBusy || !imageWidth) return;
    gesture = undefined; panGesture = undefined; outputBusy = true; outputMessage = action === 'copy' ? 'Copying image…' : 'Exporting PNG…';
    try { outputMessage = await onoutput(action, annotations, imageWidth, imageHeight); }
    catch (error) { outputMessage = `Could not ${action === 'copy' ? 'copy image' : 'export PNG'}: ${errorMessage(error)}`; }
    finally { outputBusy = false; }
  }
  let preview = $derived(gesture?.draft ? gesture.original ? annotations.map((a, i) => i === selected ? gesture!.draft! : a) : [...annotations, gesture.draft] : annotations);
</script>

<dialog bind:this={dialog} aria-label="Image annotations" aria-busy={outputBusy} tabindex="-1" onkeydown={key} oncancel={event => { event.preventDefault(); if (!outputBusy) onclose(); }}
  class="annotation-dialog fixed inset-0 m-0 flex h-dvh max-h-none w-screen max-w-none flex-col overflow-hidden border-0 bg-[#0e0f12] p-0 text-[#f3f4f6]">
  <fieldset class="contents" disabled={outputBusy} aria-label="Annotation tools">
  <!-- Presentation groups reuse the same gesture, history, and immediate-save operations. -->
  <header class="annotation-header">
    <h2>Image editor</h2>
    <div class="output-actions">
      <button type="button" disabled={!imageWidth} onclick={() => output('copy')}>Copy image</button>
      <button type="button" disabled={!imageWidth} onclick={() => output('export')}>Export PNG</button>
      <button class="primary-button" type="button" onclick={onclose}>Done <kbd>Esc</kbd></button>
    </div>
  </header>
  <div class="annotation-workspace">
    <aside class="annotation-tools" aria-label="Drawing tools">
      <h3>TOOLS</h3>
      {#each tools as item}
        <button type="button" class="tool-button" aria-pressed={tool === item} title={`${toolNames[item]} (${toolKeys[item]})`} onclick={() => { gesture = undefined; tool = item; surface?.focus(); }}>
          <svg viewBox="0 0 24 24" aria-hidden="true"><path d={toolIcons[item]} /></svg><span>{toolNames[item]}</span><kbd>{toolKeys[item]}</kbd>
        </button>
      {/each}
      <div class="tool-divider"></div>
      <button type="button" disabled={!imageWidth || (!hasCaption && annotations.length >= 10000)} onclick={openCaption}>{hasCaption ? 'Edit caption' : '＋ Add caption'}</button>
    </aside>
    <section class="annotation-main" aria-label="Image canvas workspace">
      <div class="annotation-properties">
        <div class="color-palette" role="group" aria-label="Drawing color"><span class="property-label">Color</span>
          {#each colors as swatch}<button type="button" class="color-swatch" style:--swatch={swatch.value} aria-label={swatch.name} title={swatch.name} aria-pressed={color === swatch.value} onclick={() => { color = swatch.value; surface?.focus(); }}></button>{/each}
        </div>
        <label class="width-control">Stroke <select aria-label="Stroke width" bind:value={width}>{#each [2, 4, 8, 12] as value}<option {value}>{value} px</option>{/each}</select></label>
        <div class="annotation-right-controls">
          <div class="zoom-controls" role="group" aria-label="Canvas zoom">
            <button type="button" aria-label="Zoom out" title="Zoom out (-)" disabled={!imageWidth || viewport.zoom <= MIN_ZOOM} onclick={() => { zoomBy(-1); surface?.focus(); }}>−</button>
            <button type="button" class="zoom-readout" aria-label="Reset zoom to 100%" title="Reset zoom to 100% (Ctrl/Cmd+0)" disabled={!imageWidth} onclick={() => { zoomToActual(); surface?.focus(); }}>{imageWidth ? `${Math.round(viewport.zoom * 100)}%` : '100%'}</button>
            <button type="button" aria-label="Zoom in" title="Zoom in (+)" disabled={!imageWidth || viewport.zoom >= MAX_ZOOM} onclick={() => { zoomBy(1); surface?.focus(); }}>+</button>
            <button type="button" aria-label="Zoom to fit" title="Zoom to fit (Shift+1)" disabled={!imageWidth} onclick={() => { zoomToFit(false); surface?.focus(); }}>Fit</button>
          </div>
          <div class="annotation-history"><button type="button" title="Undo (Ctrl/Cmd+Z)" onclick={() => { history(); surface?.focus(); }}>↶ Undo</button><button type="button" title="Redo (Ctrl/Cmd+Shift+Z)" onclick={() => { history(true); surface?.focus(); }}>↷ Redo</button></div>
        </div>
      </div>
      {#if outputMessage}<p class="annotation-message" role="status">{outputMessage}</p>{/if}
      {#if validation}<p class="annotation-message" role="alert">{validation}</p>{/if}
      <!-- Viewport pointer gestures complement the keyboard zoom/pan shortcuts handled on the dialog. -->
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div bind:this={stage} class="annotation-stage" class:can-pan={spaceHeld && !panGesture} class:is-panning={!!panGesture}
        style:background-position={`${viewport.x}px ${viewport.y}px`}
        onmousedown={event => { if (event.button === 1) event.preventDefault(); }}
        onpointerdowncapture={startPanCapture} onpointerdown={startStage} onpointermove={moveStage} onpointerup={finishStage}
        onpointercancel={() => { panGesture = undefined; }} onlostpointercapture={() => { panGesture = undefined; }}>
        <div class="annotation-image" class:is-ready={imageWidth > 0}
          style:width={imageWidth ? `${imageWidth * viewport.zoom}px` : undefined}
          style:height={imageHeight ? `${imageHeight * viewport.zoom}px` : undefined}
          style:transform={imageWidth ? `translate(${viewport.x}px, ${viewport.y}px)` : undefined}>
          <img src={url || undefined} alt="Original screenshot for annotation" draggable="false" class="annotation-original"
            class:pixelated={viewport.zoom >= 3}
            onload={event => {
              const image = event.currentTarget as HTMLImageElement;
              imageWidth = image.naturalWidth; imageHeight = image.naturalHeight;
              if (stage) { stageWidth = stage.clientWidth; stageHeight = stage.clientHeight; }
              viewport = fitViewport(stageWidth, stageHeight, imageWidth, imageHeight, true);
              fitted = true;
              void tick().then(() => { if (document.activeElement === dialog) surface?.focus(); });
            }} />
          {#if imageWidth}
            <!-- The application canvas implements selection, movement, and deletion through its keyboard handler. -->
            <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
            <svg bind:this={surface} viewBox={`0 0 ${imageWidth} ${imageHeight}`}
              class="annotation-surface absolute inset-0 h-full w-full touch-none"
              style:cursor={tool === 'text' ? 'text' : tool === 'select' ? 'default' : 'crosshair'}
              tabindex="0" role="application" aria-label="Annotation canvas"
              ondblclick={editCaption} onpointerdown={start} onpointermove={move} onpointerup={finish} onpointercancel={() => { gesture = undefined; }} onlostpointercapture={() => { gesture = undefined; }}>
              {@html annotationSvg(preview, selected, editingText ? selected : editingCaption ? captionIndex : -1, imageWidth, imageHeight, viewport.zoom)}
              {#if editingText && annotations[selected]?.kind === 'text'}
                {@const caption = annotations[selected] as Extract<Annotation, { kind: 'text' }>}
                {@const bounds = annotationBounds(caption)}
                <foreignObject x={caption.x} y={caption.y} width={Math.max(1, imageWidth - caption.x)} height={Math.max(1, imageHeight - caption.y)} style="overflow:visible;pointer-events:none">
                  <div class="caption-editor" style:font-size={`${caption.fontSize}px`}>
                    <span class="caption-measure" aria-hidden="true">{textDraft || 'Type text…'}</span>
                  <textarea bind:this={textInput} class="inline-caption" aria-label="Edit caption on image" placeholder="Type text…" wrap="off"
                    style:color={caption.color} style:font-size={`${caption.fontSize}px`}
                    style:border-width={`${1 / viewport.zoom}px`}
                    style:height={`${Math.max(caption.fontSize * 1.2 + 8, bounds.bottom - bounds.top + 8)}px`}
                    bind:value={textDraft} oninput={editText} onblur={() => { editingText = false; }}></textarea>
                  </div>
                </foreignObject>
              {/if}
            </svg>
          {/if}
        </div>
        {#if editingCaption}
          <div class="image-caption-overlay">
            <div class="image-caption-field">
              <span aria-hidden="true">{captionDraft || 'Add a caption…'}{'\n'}</span>
              <textarea bind:this={captionInput} aria-label="Image caption" placeholder="Add a caption…" rows="1" bind:value={captionDraft} oninput={saveCaption} onblur={() => { editingCaption = false; }}></textarea>
            </div>
          </div>
        {/if}
      </div>
    </section>
  </div>
  <footer class="annotation-footer"><span role="status">{status}</span><span>{imageWidth ? `${imageWidth} × ${imageHeight}` : 'Loading image…'}</span></footer>
  </fieldset>
</dialog>


<style>
  /* Modern developer studio overlay */
  .annotation-dialog {
    border: 1px solid #282a35;
    border-radius: 12px;
    background: #111216;
    color: #f3f4f6;
    box-shadow: 0 24px 64px rgba(0, 0, 0, 0.8), 0 0 0 1px rgba(255, 255, 255, 0.05);
    overflow: hidden;
  }
  .annotation-dialog::backdrop {
    background: rgba(0, 0, 0, 0.75);
    backdrop-filter: blur(8px);
  }
  .annotation-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    padding: 12px 18px;
    border-bottom: 1px solid #1f2128;
    background: #14151b;
  }
  h2 {
    font-size: 14px;
    font-weight: 600;
    margin: 0;
    color: #f3f4f6;
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .output-actions {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
  }
  .output-actions button {
    font-size: 11px;
    font-weight: 500;
    padding: 4px 10px;
    border-radius: 5px;
    background: #1c1d25;
    border: 1px solid #282a35;
    color: #d1d5db;
  }
  .output-actions button:hover:not(:disabled) {
    background: #252631;
    color: #fff;
    border-color: #3b3e4e;
  }
  .output-actions button.primary-button {
    background: #6366f1;
    border-color: #4f46e5;
    color: #fff;
    box-shadow: 0 1px 3px rgba(0, 0, 0, 0.3);
  }
  .output-actions button.primary-button:hover:not(:disabled) {
    background: #4f46e5;
  }
  kbd {
    font-family: ui-monospace, monospace;
    font-size: 10px;
    opacity: 0.75;
    margin-left: 6px;
    background: rgba(0, 0, 0, 0.25);
    padding: 1px 4px;
    border-radius: 3px;
  }
  .annotation-workspace {
    display: flex;
    flex: 1;
    min-height: 0;
  }
  .annotation-tools {
    width: 154px;
    flex-shrink: 0;
    padding: 14px 10px;
    border-right: 1px solid #1f2128;
    background: #111216;
    overflow-y: auto;
  }
  h3 {
    font-size: 10px;
    font-weight: 600;
    letter-spacing: 0.1em;
    color: #6b7280;
    margin: 0 8px 10px;
    text-transform: uppercase;
  }
  .annotation-tools > button {
    width: 100%;
    text-align: left;
    font-size: 12px;
    margin-bottom: 3px;
    padding: 7px 10px;
    border-radius: 6px;
    color: #9ca3af;
    border: 1px solid transparent;
  }
  .annotation-tools > button:hover:not(:disabled) {
    background: rgba(255, 255, 255, 0.05);
    color: #fff;
  }
  .tool-button {
    display: flex;
    align-items: center;
    gap: 9px;
  }
  .tool-button svg {
    width: 16px;
    height: 16px;
    fill: none;
    stroke: currentColor;
    stroke-width: 1.6;
    stroke-linejoin: round;
    stroke-linecap: round;
    flex-shrink: 0;
  }
  .tool-button kbd {
    margin-left: auto;
    font-size: 9px;
    color: #818cf8;
    background: rgba(99, 102, 241, 0.15);
    border: 1px solid rgba(99, 102, 241, 0.3);
  }
  .tool-button[aria-pressed='true'] {
    background: rgba(99, 102, 241, 0.2);
    color: #ffffff;
    border-color: rgba(99, 102, 241, 0.4);
    font-weight: 500;
  }
  .tool-divider {
    height: 1px;
    background: #1f2128;
    margin: 10px 4px;
  }
  .image-caption-overlay {
    position: absolute;
    left: 50%;
    bottom: 16px;
    transform: translateX(-50%);
    width: min(640px, calc(100% - 32px));
    padding: 10px 14px;
    border-radius: 8px;
    border: 1px solid #2d303d;
    background: rgba(14, 15, 20, 0.94);
    backdrop-filter: blur(8px);
    box-shadow: 0 12px 32px rgba(0, 0, 0, 0.65);
    z-index: 2;
  }
  .image-caption-field {
    position: relative;
    display: grid;
    max-height: 120px;
    overflow: hidden;
    color: #fff;
    font: 14px/1.5 sans-serif;
    text-align: center;
  }
  .image-caption-field span {
    grid-area: 1 / 1;
    padding: 4px 8px;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    visibility: hidden;
  }
  .image-caption-field textarea {
    position: absolute;
    inset: 0;
    box-sizing: border-box;
    width: 100%;
    height: 100%;
    min-height: 0;
    margin: 0;
    padding: 4px 8px;
    resize: none;
    color: inherit;
    background: transparent;
    border: 0;
    border-radius: 4px;
    outline: none;
    box-shadow: none;
    font: inherit;
    text-align: inherit;
    overflow-wrap: anywhere;
    user-select: text;
  }
  .image-caption-field textarea::placeholder {
    color: rgba(255, 255, 255, 0.5);
  }
  .annotation-main {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-width: 0;
    min-height: 0;
    background: #0d0e12;
  }
  .annotation-properties {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 16px;
    padding: 8px 16px;
    border-bottom: 1px solid #1f2128;
    background: #14151b;
  }
  .color-palette {
    display: flex;
    align-items: center;
    gap: 7px;
  }
  .property-label, .width-control {
    font-size: 11px;
    color: #9ca3af;
    font-weight: 500;
  }
  .property-label {
    margin-right: 3px;
  }
  .color-swatch {
    width: 20px;
    height: 20px;
    padding: 0;
    border-radius: 50%;
    border: 2px solid #14151b;
    background: var(--swatch);
    box-shadow: 0 0 0 1px #4b5563;
    transition: transform 100ms ease, box-shadow 100ms ease;
  }
  .color-swatch:hover:not(:disabled) {
    transform: scale(1.1);
    background: var(--swatch);
  }
  .color-swatch[aria-pressed='true'] {
    background: var(--swatch);
    box-shadow: 0 0 0 2px #6366f1;
    transform: scale(1.15);
  }
  .width-control {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .width-control select {
    font-size: 11px;
    padding: 3px 6px;
    border-radius: 4px;
    background: #1c1d25;
    border-color: #282a35;
    color: #d1d5db;
  }
  .annotation-right-controls {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-left: auto;
  }
  .zoom-controls,
  .annotation-history {
    display: flex;
    align-items: center;
    gap: 4px;
    font-size: 12px;
  }
  .zoom-controls button,
  .annotation-history button {
    padding: 3px 8px;
    font-size: 11px;
    border-radius: 4px;
    background: #1c1d25;
    border: 1px solid #282a35;
    color: #9ca3af;
  }
  .zoom-controls button:hover:not(:disabled),
  .annotation-history button:hover:not(:disabled) {
    background: #252631;
    color: #fff;
  }
  .zoom-controls .zoom-readout {
    min-width: 50px;
    text-align: center;
    font-variant-numeric: tabular-nums;
    color: #d1d5db;
  }
  .caption-editor {
    position: relative;
    display: inline-grid;
    max-width: 100%;
    font-family: sans-serif;
    line-height: 1.2;
    vertical-align: top;
  }
  .caption-measure {
    grid-area: 1 / 1;
    visibility: hidden;
    white-space: pre;
    padding-inline: 4px;
  }
  .inline-caption {
    position: absolute;
    top: 0;
    left: 0;
    display: block;
    box-sizing: border-box;
    width: 100%;
    min-width: 0;
    min-height: 1.2em;
    margin: 0;
    padding: 0;
    border: 1px solid rgba(255, 255, 255, 0.28);
    border-radius: 3px;
    outline: none;
    box-shadow: none;
    background: rgba(14, 15, 18, 0.9);
    font-family: sans-serif;
    line-height: 1.2;
    resize: none;
    overflow: auto;
    pointer-events: auto;
    user-select: text;
  }
  .annotation-message {
    margin: 0;
    padding: 6px 16px;
    font-size: 12px;
    background: #1e1b4b;
    color: #c7d2fe;
    border-bottom: 1px solid #3730a3;
    overflow-wrap: anywhere;
  }
  .annotation-stage {
    position: relative;
    flex: 1;
    min-height: 0;
    overflow: clip;
    touch-action: none;
    user-select: none;
    background-color: #0c0d10;
    background-image: radial-gradient(rgba(255, 255, 255, 0.06) 1px, transparent 1px);
    background-size: 20px 20px;
  }
  .annotation-stage.can-pan,
  .annotation-stage.can-pan :global(*) {
    cursor: grab !important;
  }
  .annotation-stage.is-panning,
  .annotation-stage.is-panning :global(*) {
    cursor: grabbing !important;
  }
  .annotation-image {
    position: absolute;
    top: 0;
    left: 0;
    transform-origin: 0 0;
    border-radius: 4px;
    overflow: hidden;
    box-shadow: 0 12px 40px rgba(0, 0, 0, 0.7), 0 0 0 1px rgba(255, 255, 255, 0.08);
    opacity: 0;
    pointer-events: none;
    will-change: transform, width, height;
  }
  .annotation-image.is-ready {
    opacity: 1;
    pointer-events: auto;
  }
  .annotation-surface:focus {
    outline: none;
  }
  .annotation-original {
    display: block;
    width: 100%;
    height: 100%;
    max-width: none;
    max-height: none;
    user-select: none;
  }
  .annotation-original.pixelated {
    image-rendering: pixelated;
  }
  .annotation-footer {
    display: flex;
    align-items: center;
    gap: 20px;
    padding: 6px 16px;
    border-top: 1px solid #1f2128;
    background: #111216;
    color: #6b7280;
    font-size: 11px;
  }
  .annotation-footer > :last-child {
    margin-left: auto;
  }
  @media (max-width: 720px) {
    .annotation-header { padding: 10px; }
    .annotation-tools { width: 116px; padding: 12px 5px; }
    .tool-button { gap: 6px; }
    .tool-button kbd { display: none; }
    .annotation-properties { padding: 10px; gap: 12px; }
  }
</style>

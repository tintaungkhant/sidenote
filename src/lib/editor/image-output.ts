/** Temporary raster output only; editable documents and immutable originals never pass through this module for storage. */
import { annotationSvg } from './annotations';
import type { Annotation } from './document';

/** Reject oversized surfaces before allocating canvas pixels; CSS size and display scale never affect output. */
export function validateOutputSize(width: number, height: number): void {
  if (!Number.isSafeInteger(width) || !Number.isSafeInteger(height) || width <= 0 || height <= 0 || width * height > 40_000_000) {
    throw new Error('Image output requires valid dimensions within 40 megapixels.');
  }
}

/** Render exactly the annotation-view vectors without selection decorations or unfinished gestures. */
export function outputSvg(annotations: readonly Annotation[], width: number, height: number): string {
  validateOutputSize(width, height);
  return `<svg xmlns="http://www.w3.org/2000/svg" width="${width}" height="${height}" viewBox="0 0 ${width} ${height}">${annotationSvg(annotations, -1, -1, width, height)}</svg>`;
}

/** Compose onto transparent pixels with ordinary source-over alpha, original first and annotations last. */
export function drawOutput(context: Pick<CanvasRenderingContext2D, 'clearRect' | 'drawImage' | 'globalCompositeOperation' | 'globalAlpha'>, original: CanvasImageSource, overlay: CanvasImageSource, width: number, height: number): void {
  validateOutputSize(width, height);
  context.globalCompositeOperation = 'source-over'; context.globalAlpha = 1;
  context.clearRect(0, 0, width, height);
  context.drawImage(original, 0, 0, width, height);
  context.drawImage(overlay, 0, 0, width, height);
}

/** Decode only local object URLs, reporting failures instead of producing a blank or partial image. */
async function loadImage(url: string): Promise<HTMLImageElement> {
  const image = new Image();
  image.src = url;
  try { await image.decode(); return image; }
  catch { image.removeAttribute('src'); throw new Error('Could not decode the image for output. Reopen the image and retry.'); }
}

/** Rasterize a completed snapshot at source resolution and release all temporary decoded images and canvas pixels. */
export async function flattenImage(original: Blob, annotations: readonly Annotation[], width: number, height: number): Promise<Uint8Array> {
  const svg = outputSvg(annotations, width, height);
  const originalUrl = URL.createObjectURL(original), overlayUrl = URL.createObjectURL(new Blob([svg], { type: 'image/svg+xml' }));
  const canvas = document.createElement('canvas');
  let image: HTMLImageElement | undefined, overlay: HTMLImageElement | undefined;
  try {
    image = await loadImage(originalUrl); overlay = await loadImage(overlayUrl);
    if (image.naturalWidth !== width || image.naturalHeight !== height) throw new Error('Original image dimensions changed. Reopen the image and retry.');
    canvas.width = width; canvas.height = height;
    const context = canvas.getContext('2d');
    if (!context) throw new Error('Could not allocate image output. Try a smaller image or free memory.');
    drawOutput(context, image, overlay, width, height);
    const png = await new Promise<Blob>((resolve, reject) => canvas.toBlob(blob => blob ? resolve(blob) : reject(new Error('PNG encoding failed. Try a smaller image.')), 'image/png'));
    if (png.size > 64 * 1024 * 1024) throw new Error('Output PNG exceeds 64 MiB. Try a smaller image.');
    return new Uint8Array(await png.arrayBuffer());
  } finally {
    image?.removeAttribute('src'); overlay?.removeAttribute('src');
    canvas.width = 0; canvas.height = 0;
    URL.revokeObjectURL(originalUrl); URL.revokeObjectURL(overlayUrl);
  }
}

// The PDF view's pdf.js side (the file previews design), in its own module so pdf.js loads only when a PDF opens.
// The document is untrusted: pdf.js 6 has no eval path at all (the isEvalSupported option is gone with it), its
// JavaScript never runs (the core renderer runs no document scripts; the
// scripting sandbox is a viewer component we don't load), XFA forms are off, and pages are drawn to canvas with their
// text laid over them as plain text (pdf.js's text layer, WEB-U-277): a screen reader reads it and a person can select
// it. No annotation layer and no images handler, so no link in a PDF is ever clickable. The bytes arrive as an
// ArrayBuffer; the PDF never gets a URL of its own. The worker is a chunk of this build, served from our own origin.
import {
  getDocument,
  GlobalWorkerOptions,
  TextLayer,
  version,
} from "pdfjs-dist";

/** The pages a preview draws; the note says how many there are. */
export const MAX_PAGES = 20;

let worker: Worker | undefined;
function workerPort(): Worker {
  worker ??= new Worker(
    new URL("pdfjs-dist/build/pdf.worker.min.mjs", import.meta.url),
    { type: "module" },
  );
  return worker;
}

/**
 * Draws the first pages of `bytes` into `into`, each `width` CSS pixels wide, and says how many pages there are.
 * `signal` stops it (the view closed or the file changed).
 */
export async function renderPdf(
  bytes: ArrayBuffer,
  into: HTMLElement,
  width: number,
  label: (page: number) => string,
  signal: AbortSignal,
): Promise<{ pages: number; shown: number }> {
  GlobalWorkerOptions.workerPort = workerPort();
  const task = getDocument({
    // A copy: pdf.js takes the buffer over, and the query cache keeps the original for a remount.
    data: new Uint8Array(bytes.slice(0)),
    enableXfa: false,
    // The standard fonts' data and the CMaps from our own origin, for this exact pdf.js (scripts/sync-pdfjs.mjs): no
    // CDN, under the app's font-src and connect-src 'self'.
    standardFontDataUrl: `${window.location.origin}/pdfjs/${version}/standard_fonts/`,
    cMapUrl: `${window.location.origin}/pdfjs/${version}/cmaps/`,
    cMapPacked: true,
  });
  const stop = () => void task.destroy();
  signal.addEventListener("abort", stop);
  try {
    const document = await task.promise;
    const shown = Math.min(document.numPages, MAX_PAGES);
    const ratio = window.devicePixelRatio || 1;
    for (let number = 1; number <= shown && !signal.aborted; number++) {
      const page = await document.getPage(number);
      const scale = width / page.getViewport({ scale: 1 }).width;
      const viewport = page.getViewport({ scale: scale * ratio });
      // The page: a group named "Page N" holding its picture, hidden from assistive technology, and its text over it.
      const frame = into.ownerDocument.createElement("div");
      frame.className = "file-pdf-page";
      frame.setAttribute("role", "group");
      frame.setAttribute("aria-label", label(number));
      frame.style.width = `${Math.floor(viewport.width / ratio)}px`;
      // What pdf.js's text layer sizes itself and its text by: the page's CSS scale, rounded to whole pixels.
      frame.style.setProperty("--total-scale-factor", String(scale));
      frame.style.setProperty("--scale-round-x", "1px");
      frame.style.setProperty("--scale-round-y", "1px");
      const canvas = into.ownerDocument.createElement("canvas");
      canvas.width = Math.floor(viewport.width);
      canvas.height = Math.floor(viewport.height);
      canvas.style.width = `${Math.floor(viewport.width / ratio)}px`;
      canvas.setAttribute("aria-hidden", "true");
      const text = into.ownerDocument.createElement("div");
      text.className = "textLayer";
      frame.append(canvas, text);
      into.append(frame);
      await page.render({ canvas, viewport }).promise;
      // Its text as plain text over the picture, at the CSS scale (pdf.js applies the screen's density itself).
      if (!signal.aborted)
        await new TextLayer({
          textContentSource: page.streamTextContent(),
          container: text,
          viewport: page.getViewport({ scale }),
        }).render();
      page.cleanup();
    }
    return { pages: document.numPages, shown };
  } finally {
    signal.removeEventListener("abort", stop);
    // The pages stay drawn; the document, its worker port and memory go.
    void task.destroy();
  }
}

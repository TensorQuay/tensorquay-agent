import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

// pdf.js itself draws in the browser spec (page-sheet-previews, WEB-U-091); here it is stood in, to check what this
// module asks of it: the options, the worker, the pages drawn and what is let go.
const destroy = vi.fn(() => Promise.resolve());
const getDocument = vi.fn();
const GlobalWorkerOptions: { workerPort?: unknown } = {};
/** pdf.js's text layer, stood in: what each page asked it to lay out, and its text put in place. */
const layers: {
  textContentSource: unknown;
  container: HTMLElement;
  viewport: { width: number };
}[] = [];
class TextLayer {
  constructor(
    readonly options: {
      textContentSource: unknown;
      container: HTMLElement;
      viewport: { width: number };
    },
  ) {
    layers.push(options);
  }
  render() {
    const span = document.createElement("span");
    span.textContent = "Quarterly report";
    this.options.container.append(span);
    return Promise.resolve();
  }
}
vi.mock("pdfjs-dist", () => ({
  getDocument,
  GlobalWorkerOptions,
  TextLayer,
  version: "6.3.289",
}));

const workers: unknown[] = [];
class FakeWorker {
  constructor(
    readonly url: URL,
    readonly options: WorkerOptions,
  ) {
    workers.push(this);
  }
}

/** A stand-in document of `pages` pages, each 600 × 800 at scale 1. */
function documentOf(pages: number) {
  const drawn: number[] = [];
  const cleaned: number[] = [];
  const page = (number: number) => ({
    getViewport: ({ scale }: { scale: number }) => ({
      width: 600 * scale,
      height: 800 * scale,
    }),
    render: () => {
      drawn.push(number);
      return { promise: Promise.resolve() };
    },
    cleanup: () => cleaned.push(number),
    streamTextContent: () => `text of page ${number}`,
  });
  getDocument.mockImplementation(() => ({
    promise: Promise.resolve({
      numPages: pages,
      getPage: (number: number) => Promise.resolve(page(number)),
    }),
    destroy,
  }));
  return { drawn, cleaned };
}

beforeEach(() => {
  vi.stubGlobal("Worker", FakeWorker);
  vi.stubGlobal("devicePixelRatio", 2);
  getDocument.mockReset();
  destroy.mockClear();
  layers.length = 0;
});
afterEach(() => vi.unstubAllGlobals());

const label = (page: number) => `Page ${page}`;

describe("WEB-U-091 pdf-render", () => {
  it("draws every page of a short PDF at the width asked, sharp on a dense screen, then lets the document go", async () => {
    const { renderPdf } = await import("./pdf-render");
    const { drawn, cleaned } = documentOf(2);
    const into = document.createElement("div");
    const bytes = new TextEncoder().encode("%PDF").buffer;
    const done = await renderPdf(
      bytes,
      into,
      300,
      label,
      new AbortController().signal,
    );
    expect(done).toEqual({ pages: 2, shown: 2 });
    expect(drawn).toEqual([1, 2]);
    expect(cleaned).toEqual([1, 2]);
    // WEB-U-277: each page is a group named for it, its picture hidden from assistive technology and its text laid
    // over it by pdf.js's text layer, at the page's CSS scale, from the page's own text.
    const pages = [...into.querySelectorAll<HTMLElement>(".file-pdf-page")];
    expect(pages.map((page) => page.getAttribute("role"))).toEqual([
      "group",
      "group",
    ]);
    expect(pages.map((page) => page.getAttribute("aria-label"))).toEqual([
      "Page 1",
      "Page 2",
    ]);
    expect(pages[0]?.style.getPropertyValue("--total-scale-factor")).toBe(
      "0.5",
    );
    const canvases = [...into.querySelectorAll("canvas")];
    expect(canvases[0]?.getAttribute("aria-hidden")).toBe("true");
    expect(canvases[0]?.getAttribute("role")).toBeNull();
    expect(layers.map((layer) => layer.textContentSource)).toEqual([
      "text of page 1",
      "text of page 2",
    ]);
    expect(layers[0]?.container.className).toBe("textLayer");
    expect(layers[0]?.container.parentElement).toBe(pages[0]);
    expect(layers[0]?.viewport.width).toBe(300);
    expect(pages[0]).toHaveTextContent("Quarterly report");
    // Nothing in the page is a link or a control.
    expect(into.querySelector("a, button, input, [role='link']")).toBeNull();
    // 300 CSS pixels wide, drawn at twice that for the screen's density.
    expect(canvases[0]?.width).toBe(600);
    expect(canvases[0]?.height).toBe(800);
    expect(canvases[0]?.style.width).toBe("300px");
    // The document's options: XFA forms off, and its own copy of the bytes.
    const [options] = getDocument.mock.calls[0] as [
      {
        data: Uint8Array;
        enableXfa: boolean;
        standardFontDataUrl: string;
        cMapUrl: string;
        cMapPacked: boolean;
      },
    ];
    expect(options.enableXfa).toBe(false);
    // WEB-E2E-030: the standard fonts' data and the CMaps from our own origin, for this exact pdf.js.
    expect(options.standardFontDataUrl).toBe(
      `${window.location.origin}/pdfjs/6.3.289/standard_fonts/`,
    );
    expect(options.cMapUrl).toBe(
      `${window.location.origin}/pdfjs/6.3.289/cmaps/`,
    );
    expect(options.cMapPacked).toBe(true);
    expect(new TextDecoder().decode(options.data)).toBe("%PDF");
    expect(options.data.buffer).not.toBe(bytes);
    expect(destroy).toHaveBeenCalledOnce();
  });

  it("draws only the first 20 pages of a long one, and says how many there are", async () => {
    const { renderPdf, MAX_PAGES } = await import("./pdf-render");
    const { drawn } = documentOf(140);
    const into = document.createElement("div");
    const done = await renderPdf(
      new ArrayBuffer(4),
      into,
      300,
      label,
      new AbortController().signal,
    );
    expect(MAX_PAGES).toBe(20);
    expect(done).toEqual({ pages: 140, shown: 20 });
    expect(drawn).toHaveLength(20);
  });

  it("runs pdf.js in one module worker of this build, made once", async () => {
    const { renderPdf } = await import("./pdf-render");
    documentOf(1);
    const into = document.createElement("div");
    await renderPdf(
      new ArrayBuffer(4),
      into,
      300,
      label,
      new AbortController().signal,
    );
    await renderPdf(
      new ArrayBuffer(4),
      into,
      300,
      label,
      new AbortController().signal,
    );
    expect(workers).toHaveLength(1);
    const worker = workers[0] as FakeWorker;
    expect(worker.options).toEqual({ type: "module" });
    expect(worker.url.pathname).toMatch(/pdf\.worker\.min\.mjs$/);
    expect(GlobalWorkerOptions.workerPort).toBe(worker);
  });

  it("stops drawing when told to, and lets the document go", async () => {
    const { renderPdf } = await import("./pdf-render");
    const { drawn } = documentOf(5);
    const stop = new AbortController();
    stop.abort();
    const into = document.createElement("div");
    await renderPdf(new ArrayBuffer(4), into, 300, label, stop.signal);
    expect(drawn).toEqual([]);
    expect(destroy).toHaveBeenCalled();
  });

  it("passes pdf.js's own error on, so the panel can say what went wrong", async () => {
    const { renderPdf } = await import("./pdf-render");
    const failure = Object.assign(new Error("bad"), {
      name: "InvalidPDFException",
    });
    getDocument.mockImplementation(() => ({
      promise: Promise.reject(failure),
      destroy,
    }));
    await expect(
      renderPdf(
        new ArrayBuffer(4),
        document.createElement("div"),
        300,
        label,
        new AbortController().signal,
      ),
    ).rejects.toBe(failure);
    expect(destroy).toHaveBeenCalled();
  });
});

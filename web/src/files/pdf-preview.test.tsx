import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, within } from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { FileContent } from "../protocol/generated/api";
import { conversationId } from "../test/frames";
import { LIMITS } from "../test/user";

import { FilePreview } from "./FilePreview";
import { pdfFailure } from "./pdf-failure";

// pdf.js itself runs in the browser spec (page-sheet-previews); here its module is a stand-in that draws named
// canvases or fails as asked.
const renderPdf = vi.fn();
vi.mock("./pdf-render", () => ({ renderPdf }));

const content = (path: string, size: number): FileContent => ({
  path,
  size,
  modified_at: "2026-01-01T00:00:00Z",
  etag: "e1",
  binary: true,
  truncated: false,
  text: null,
});

let files: Record<string, FileContent>;
const fetch = vi.fn();
beforeEach(() => {
  files = {};
  renderPdf.mockReset();
  fetch.mockReset().mockImplementation((input) => {
    const url = new URL(input as string, "http://localhost");
    if (url.pathname.endsWith("/files/download"))
      return Promise.resolve(
        new Response(new TextEncoder().encode("%PDF-1.4"), { status: 200 }),
      );
    return Promise.resolve(
      Response.json(files[url.searchParams.get("path") ?? ""], {
        status: 200,
      }),
    );
  });
  vi.stubGlobal("fetch", fetch);
});
afterEach(() => vi.unstubAllGlobals());

function show(path: string) {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  client.setQueryData(["session"], {
    id: "user-1",
    email: "person@example.com",
    role: "member",
    internet: "none",
    limits: LIMITS,
  });
  const wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={client}>{children}</QueryClientProvider>
  );
  return render(
    <FilePreview
      conversationId={conversationId}
      path={path}
      onBack={vi.fn()}
    />,
    { wrapper },
  );
}
const downloads = () =>
  fetch.mock.calls.filter(([input]) =>
    String(input).includes("/files/download"),
  ).length;

/** A stand-in renderPdf that draws `shown` pages of `pages`, each a group named for it, as pdf-render's are. */
function draws(pages: number, shown: number) {
  renderPdf.mockImplementation(
    (
      _bytes: ArrayBuffer,
      into: HTMLElement,
      _width: number,
      label: (page: number) => string,
    ) => {
      for (let page = 1; page <= shown; page++) {
        const frame = document.createElement("div");
        frame.setAttribute("role", "group");
        frame.setAttribute("aria-label", label(page));
        frame.append(document.createElement("canvas"));
        into.append(frame);
      }
      return Promise.resolve({ pages, shown });
    },
  );
}

describe("WEB-U-091 a PDF in the Files panel", () => {
  it("draws the pages from the file's bytes, with no note when all are shown", async () => {
    files["brief.pdf"] = content("brief.pdf", 900);
    draws(2, 2);
    show("brief.pdf");
    const pages = await screen.findByRole("region", {
      name: "Pages of brief.pdf",
    });
    expect(
      await within(pages).findByRole("group", { name: "Page 2" }),
    ).toBeTruthy();
    expect(renderPdf).toHaveBeenCalledOnce();
    // The bytes come from the download, as an ArrayBuffer: the PDF never gets a URL of its own.
    const [bytes] = renderPdf.mock.calls[0] as [ArrayBuffer];
    expect(new TextDecoder().decode(bytes)).toBe("%PDF-1.4");
    expect(screen.queryByText(/Download it for the rest/)).toBeNull();
    expect(screen.getByRole("link", { name: "Download" })).toBeTruthy();
  });

  it("says how many pages there are when it draws the first 20", async () => {
    files["long.pdf"] = content("long.pdf", 900);
    draws(140, 20);
    show("long.pdf");
    expect(
      await screen.findByText(
        "Showing 20 of 140 pages. Download it for the rest.",
      ),
    ).toBeTruthy();
  });

  it("doesn't fetch a PDF over the preview limit, and says so with both sizes", async () => {
    files["huge.pdf"] = content("huge.pdf", LIMITS.preview_pdf_bytes + 1);
    show("huge.pdf");
    expect(
      await screen.findByText(
        "This PDF is too large to preview (20 MB of 20 MB). Download it to open it.",
      ),
    ).toBeTruthy();
    expect(downloads()).toBe(0);
    expect(renderPdf).not.toHaveBeenCalled();
  });

  it.each([
    ["InvalidPDFException", "This PDF can't be shown: it's damaged."],
    ["FormatError", "This PDF can't be shown: it's damaged."],
    [
      "PasswordException",
      "This PDF can't be shown: it's protected with a password.",
    ],
    [
      "UnknownErrorException",
      "This PDF couldn't be loaded. Download it to open it.",
    ],
  ])("says what went wrong: %s", async (name, words) => {
    files["bad.pdf"] = content("bad.pdf", 900);
    renderPdf.mockImplementation(() => {
      const error = new Error("from pdf.js");
      error.name = name;
      return Promise.reject(error);
    });
    show("bad.pdf");
    expect(await screen.findByText(words)).toBeTruthy();
    expect(screen.getByRole("link", { name: "Download" })).toBeTruthy();
  });

  it("opens wide on Expand, drawn again at the dialog's width", async () => {
    files["brief.pdf"] = content("brief.pdf", 900);
    draws(1, 1);
    show("brief.pdf");
    await screen.findByRole("group", { name: "Page 1" });
    fireEvent.click(screen.getByRole("button", { name: "Expand" }));
    const dialog = await screen.findByRole("dialog", { name: "brief.pdf" });
    expect(
      await within(dialog).findByRole("group", { name: "Page 1" }),
    ).toBeTruthy();
    expect(screen.getAllByRole("group", { name: "Page 1" })).toHaveLength(1);
    // Drawn twice (the panel, then the dialog) from one download.
    expect(renderPdf).toHaveBeenCalledTimes(2);
    expect(downloads()).toBe(1);
  });

  it("names pdf.js's errors by what the person can do", () => {
    const named = (name: string) => Object.assign(new Error("x"), { name });
    expect(pdfFailure(named("PasswordException"))).toBe("protected");
    expect(pdfFailure(named("InvalidPDFException"))).toBe("damaged");
    expect(pdfFailure(named("FormatError"))).toBe("damaged");
    expect(pdfFailure(named("MissingPDFException"))).toBe("failed");
    expect(pdfFailure("not an error")).toBe("failed");
  });
});

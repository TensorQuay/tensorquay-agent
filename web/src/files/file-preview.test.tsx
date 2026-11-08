import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { FileContent } from "../protocol/generated/api";
import { conversationId } from "../test/frames";
import { settleLazy } from "../test/lazy";

import { FilePreview } from "./FilePreview";

const content = (
  path: string,
  text: string | null,
  overrides: Partial<FileContent> = {},
): FileContent => ({
  path,
  size: text === null ? 1200 : new TextEncoder().encode(text).length,
  modified_at: "2026-01-01T00:00:00Z",
  etag: "e1",
  binary: text === null,
  truncated: false,
  text,
  ...overrides,
});

let answers: Record<string, FileContent>;
const fetch = vi.fn();
const createObjectURL = vi.fn(() => "blob:preview-1");
const revokeObjectURL = vi.fn();
beforeEach(() => {
  answers = {};
  fetch.mockReset().mockImplementation((input) => {
    const url = new URL(input as string, "http://localhost");
    const path = url.searchParams.get("path") ?? "";
    if (url.pathname.endsWith("/files/download"))
      return Promise.resolve(
        new Response(new Uint8Array([137, 80, 78, 71]), { status: 200 }),
      );
    return Promise.resolve(Response.json(answers[path], { status: 200 }));
  });
  vi.stubGlobal("fetch", fetch);
  Object.assign(URL, { createObjectURL, revokeObjectURL });
  createObjectURL.mockClear();
  try {
    localStorage.clear();
  } catch {
    /* no storage in this environment */
  }
});
afterEach(() => vi.unstubAllGlobals());

function show(path: string) {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
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

// The files panel shows what the agent made, not only code.
describe("WEB-U-022 previews by kind", () => {
  it("F1: shows a PNG as an image, from its bytes typed by the file's extension", async () => {
    answers["out/chart.png"] = content("out/chart.png", null);
    show("out/chart.png");
    const image = await screen.findByRole("img", { name: "chart.png" });
    await waitFor(() => expect(image).toHaveAttribute("src", "blob:preview-1"));
    const [blob] = createObjectURL.mock.calls[0] as unknown as [Blob];
    expect(blob.type).toBe("image/png");
    expect(screen.getByRole("link", { name: "Download" })).toBeVisible();
  });

  // A blob: URL has the page's origin, so an SVG opened from it in a new tab would run its script with the session. An
  // SVG shows from a data: URL: a data: document has an opaque origin, so its script can't reach this origin or the
  // session.
  it("F1: shows an SVG only as an image from a data: URL, never blob: and never as markup", async () => {
    answers["logo.svg"] = content(
      "logo.svg",
      '<svg xmlns="http://www.w3.org/2000/svg"><script>alert(1)</script></svg>',
    );
    const { container } = show("logo.svg");
    const image = await screen.findByRole("img", { name: "logo.svg" });
    await waitFor(() =>
      expect(image.getAttribute("src")).toMatch(
        /^data:image\/svg\+xml;base64,/,
      ),
    );
    expect(createObjectURL).not.toHaveBeenCalled();
    expect(container.querySelector("svg script")).toBeNull();
  });

  it("F1: doesn't fetch an image too large to preview, and says so", async () => {
    answers["scan.jpg"] = content("scan.jpg", null, { size: 11 * 1024 * 1024 });
    show("scan.jpg");
    expect(
      await screen.findByText(
        /^Too large to preview \(11.*\)\. Download it to open it\.$/,
      ),
    ).toBeVisible();
    expect(downloads()).toBe(0);
    expect(screen.queryByRole("img")).toBeNull();
  });

  it("F2: renders a Markdown document, with its source a toggle away, remembered", async () => {
    answers["brief.md"] = content(
      "brief.md",
      "# Findings\n\n**Three** points for the team.\n",
    );
    const { unmount } = show("brief.md");
    // The renderer loads lazily: settled first, never raced against findBy's deadline (WEB-U-014).
    await screen.findByRole("button", { name: /^Show the / });
    await settleLazy();
    expect(screen.getByRole("heading", { name: "Findings" })).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "Show the source" }));
    expect(
      screen.getByRole("region", { name: "Contents of brief.md" }),
    ).toHaveTextContent("# Findings");
    unmount();
    show("brief.md");
    expect(
      await screen.findByRole("region", { name: "Contents of brief.md" }),
    ).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "Show the document" }));
    // The renderer loads lazily: settled first, never raced against findBy's deadline (WEB-U-014).
    await screen.findByRole("button", { name: /^Show the / });
    await settleLazy();
    expect(screen.getByRole("heading", { name: "Findings" })).toBeVisible();
  });

  it("F3: shows a CSV as a table, with its size, and as text on request", async () => {
    answers["clean.csv"] = content(
      "clean.csv",
      "region,total\nNorth,1204\nSouth,98\nEast,7\n",
    );
    show("clean.csv");
    const table = await screen.findByRole("table", {
      name: "Table of clean.csv",
    });
    expect(
      within(table)
        .getAllByRole("columnheader")
        .map((cell) => cell.textContent),
    ).toEqual(["region", "total"]);
    expect(within(table).getAllByRole("row")).toHaveLength(4);
    expect(screen.getByText("3 rows")).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "Show as text" }));
    expect(
      screen.getByRole("region", { name: "Contents of clean.csv" }),
    ).toHaveTextContent("North,1204");
  });

  it("F3: shows the first 200 rows of a long table and says how many there are", async () => {
    const rows = Array.from(
      { length: 250 },
      (_, index) => `r${index},${index}`,
    );
    answers["long.csv"] = content(
      "long.csv",
      `name,value\n${rows.join("\n")}\n`,
    );
    show("long.csv");
    const table = await screen.findByRole("table", {
      name: "Table of long.csv",
    });
    expect(within(table).getAllByRole("row")).toHaveLength(201);
    expect(screen.getByText("Showing 200 of 250 rows")).toBeVisible();
  });

  it("F3: falls back to text when the file isn't a table", async () => {
    answers["broken.csv"] = content("broken.csv", 'a,b\n1,"never closed\n');
    show("broken.csv");
    expect(
      await screen.findByText("Couldn't read this as a table."),
    ).toBeVisible();
    expect(
      screen.getByRole("region", { name: "Contents of broken.csv" }),
    ).toBeVisible();
  });

  it("F3: keeps the cells of a row wider than its header", async () => {
    answers["wide.csv"] = content("wide.csv", "a,b\n1,2,3\n");
    show("wide.csv");
    const table = await screen.findByRole("table", {
      name: "Table of wide.csv",
    });
    expect(within(table).getByText("3")).toBeVisible();
  });

  // A remount (closing and reopening the panel) gets a fresh object URL, never a revoked one.
  it("F1: makes a fresh image URL when it shows again, and lets go of the old one", async () => {
    answers["out/chart.png"] = content("out/chart.png", null);
    const first = show("out/chart.png");
    await waitFor(() => expect(createObjectURL).toHaveBeenCalledTimes(1));
    first.unmount();
    expect(revokeObjectURL).toHaveBeenCalledWith("blob:preview-1");
    createObjectURL.mockReturnValueOnce("blob:preview-2");
    show("out/chart.png");
    const image = await screen.findByRole("img", { name: "chart.png" });
    await waitFor(() => expect(image).toHaveAttribute("src", "blob:preview-2"));
  });
});

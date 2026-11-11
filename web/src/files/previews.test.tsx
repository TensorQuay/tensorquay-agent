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

import type { FileContent, SheetPreview } from "../protocol/generated/api";
import { conversationId } from "../test/frames";
import { choose } from "../test/picker";
import { LIMITS } from "../test/user";

import { FilePreview } from "./FilePreview";

const content = (path: string, size: number, text: string | null) =>
  ({
    path,
    size,
    modified_at: "2026-01-01T00:00:00Z",
    etag: "e1",
    binary: text === null,
    truncated: false,
    text,
  }) satisfies FileContent;

const sheet = (overrides: Partial<SheetPreview> = {}): SheetPreview => ({
  sheets: ["Totals"],
  sheet: "Totals",
  rows: [["Region", "Sales"]],
  total_rows: 1,
  more_rows: false,
  more_columns: false,
  truncated_cells: 0,
  ...overrides,
});

type Answer = { status: number; body: unknown };
let files: Record<string, FileContent>;
/** The sheet answers by the requested sheet name ("" for none). */
let sheets: Record<string, Answer>;
/** A file's whole bytes, as the download answers them. */
let downloads: Record<string, string>;
const fetch = vi.fn();
beforeEach(() => {
  files = {};
  sheets = {};
  downloads = {};
  fetch.mockReset().mockImplementation((input) => {
    const url = new URL(input as string, "http://localhost");
    if (url.pathname.endsWith("/files/download"))
      return Promise.resolve(
        new Response(downloads[url.searchParams.get("path") ?? ""] ?? "", {
          status: 200,
          headers: { "Content-Type": "application/octet-stream" },
        }),
      );
    if (url.pathname.endsWith("/files/sheet")) {
      const answer = sheets[url.searchParams.get("sheet") ?? ""] ?? {
        status: 404,
        body: {
          error: { code: "not_found", message: "not found", retryable: false },
        },
      };
      return Promise.resolve(
        Response.json(answer.body, { status: answer.status }),
      );
    }
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
const sheetCalls = () =>
  fetch.mock.calls
    .map(([input]) => new URL(input as string, "http://localhost"))
    .filter((url) => url.pathname.endsWith("/files/sheet"));
const error = (status: number, code: string): Answer => ({
  status,
  body: { error: { code, message: code, retryable: false } },
});

// Scripts run, in a sandbox that keeps them from the app.
describe("WEB-U-087 a web page in the Files panel", () => {
  it("frames the page from the sandboxed view, with only allow-scripts, the note, Reload and Download", async () => {
    files["out/report.html"] = content("out/report.html", 900, "<p>hi</p>");
    show("out/report.html");
    const frame = await screen.findByTitle("Preview of report.html");
    const src = new URL(frame.getAttribute("src") ?? "", "http://localhost");
    expect(src.pathname).toBe(
      `/api/v1/conversations/${conversationId}/files/view`,
    );
    expect(src.searchParams.get("path")).toBe("out/report.html");
    expect(frame.getAttribute("sandbox")).toBe("allow-scripts");
    for (const grant of [
      "allow-same-origin",
      "allow-popups",
      "allow-top-navigation",
      "allow-forms",
      "allow-modals",
      "allow-downloads",
    ])
      expect(frame.getAttribute("sandbox")).not.toContain(grant);
    expect(frame.getAttribute("referrerpolicy")).toBe("no-referrer");
    expect(screen.getByText("Scripts are running in a sandbox")).toBeTruthy();
    expect(
      screen.getByRole("link", { name: "Download" }).getAttribute("href"),
    ).toContain("/files/download");
    // Never the page's text as source: it is shown, not read.
    expect(screen.queryByText("<p>hi</p>")).toBeNull();
  });

  it("Reload starts the page again in a new frame", async () => {
    files["page.htm"] = content("page.htm", 10, "<p>x</p>");
    show("page.htm");
    const first = await screen.findByTitle("Preview of page.htm");
    fireEvent.click(screen.getByRole("button", { name: "Reload" }));
    const second = screen.getByTitle("Preview of page.htm");
    expect(second).not.toBe(first);
    expect(second.getAttribute("sandbox")).toBe("allow-scripts");
  });

  // WEB-U-254 (a pass over the pages before 17.0): a link followed inside the page leaves the frame blank (the sandbox
  // blocks it); its second load says why, and Reload brings the page back without the note.
  it("says why the frame went blank after a link, until Reload", async () => {
    files["page.htm"] = content("page.htm", 10, '<a href="/">x</a>');
    show("page.htm");
    const note =
      "Links in a preview don't open here. Reload to see the page again.";
    const first = await screen.findByTitle("Preview of page.htm");
    fireEvent.load(first);
    expect(screen.queryByText(note)).toBeNull();
    fireEvent.load(first);
    expect(screen.getByRole("status")).toHaveTextContent(note);
    fireEvent.click(screen.getByRole("button", { name: "Reload" }));
    const second = screen.getByTitle("Preview of page.htm");
    fireEvent.load(second);
    expect(screen.queryByText(note)).toBeNull();
  });

  it("Expand opens the page wide in a dialog, one frame at a time, and closing brings it back", async () => {
    files["out/report.html"] = content("out/report.html", 900, "<p>hi</p>");
    show("out/report.html");
    await screen.findByTitle("Preview of report.html");
    fireEvent.click(screen.getByRole("button", { name: "Expand" }));
    const dialog = await screen.findByRole("dialog", { name: "report.html" });
    const wide = within(dialog).getByTitle("Preview of report.html");
    expect(wide.getAttribute("sandbox")).toBe("allow-scripts");
    expect(screen.getAllByTitle("Preview of report.html")).toHaveLength(1);
    expect(
      within(dialog).getByText("Scripts are running in a sandbox"),
    ).toBeTruthy();
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Close dialog" }),
    );
    expect(screen.queryByRole("dialog", { name: "report.html" })).toBeNull();
    expect(screen.getAllByTitle("Preview of report.html")).toHaveLength(1);
  });

  it("doesn't frame a page over the preview limit, and says so with both sizes", async () => {
    const size = LIMITS.preview_html_bytes + 1;
    files["big.html"] = content("big.html", size, null);
    show("big.html");
    expect(
      await screen.findByText(
        "This page is too large to preview (2 MB of 2 MB). Download it to open it.",
      ),
    ).toBeTruthy();
    expect(screen.queryByTitle("Preview of big.html")).toBeNull();
    expect(screen.getByRole("link", { name: "Download" })).toBeTruthy();
  });
});

// WEB-U-467: a preview runs offline, so a page that loads from the internet says so on its own row under the bar,
// naming the hosts as text; the bar keeps the sandbox note and Reload, and a self-contained page has no row.
describe("WEB-U-467 a page that loads from the internet", () => {
  const sandboxed = "Scripts are running in a sandbox";
  const offline = (hosts: string) =>
    `Previews run offline: what this page loads from ${hosts} won't show here. Download it to see it all.`;
  const weather = [
    '<script src="https://cdn.example.org/three.min.js"></script>',
    '<p id="now">Loading…</p><a href="https://example.com/about">About</a>',
    "<script>fetch('https://api.example.org/forecast')</script>",
  ].join("");

  it("names the hosts on a row of its own under the bar, as text, never as links", async () => {
    files["weather.html"] = content("weather.html", weather.length, weather);
    show("weather.html");
    const line = await screen.findByText(
      offline("cdn.example.org and api.example.org"),
    );
    expect(line.tagName).toBe("P");
    expect(line.closest("a")).toBeNull();
    expect(line.querySelector("a")).toBeNull();
    // The bar is as before: the sandbox note and Reload; the line follows it.
    const note = screen.getByText(sandboxed);
    const bar = note.parentElement;
    expect(bar?.className).toBe("file-kind-bar");
    expect(line.closest(".file-kind-bar")).toBeNull();
    expect(bar?.nextElementSibling).toBe(line);
    expect(screen.getAllByRole("link").map((link) => link.textContent)).toEqual(
      ["Download"],
    );
    expect(screen.getByTitle("Preview of weather.html")).toBeTruthy();
  });

  it("names three hosts at most, then counts the rest", async () => {
    const html = ["a", "b", "c", "d", "e"]
      .map((name) => `<script src="https://${name}.example.org/x.js"></script>`)
      .join("");
    files["many.html"] = content("many.html", html.length, html);
    show("many.html");
    expect(
      await screen.findByText(
        offline("a.example.org, b.example.org, c.example.org, and 2 more"),
      ),
    ).toBeTruthy();
  });

  it("shows the bar alone for a self-contained page", async () => {
    const html = '<p>hi</p><a href="https://example.org/">More</a>';
    files["report.html"] = content("report.html", html.length, html);
    show("report.html");
    await screen.findByTitle("Preview of report.html");
    // Let the page's text be read before looking for a line that must not come.
    await new Promise((settle) => setTimeout(settle, 20));
    expect(screen.getByText(sandboxed)).toBeTruthy();
    expect(screen.queryByText(/Previews run offline/)).toBeNull();
  });

  it("reads a page past the preview's text from its download", async () => {
    const html = `${"<p>row</p>".repeat(60_000)}<script src="https://cdn.example.org/late.js"></script>`;
    files["long.html"] = {
      ...content("long.html", html.length, html.slice(0, 512 * 1024)),
      truncated: true,
    };
    downloads["long.html"] = html;
    show("long.html");
    expect(await screen.findByText(offline("cdn.example.org"))).toBeTruthy();
  });

  /** The page's download fails `failures` times (a dropped connection), then answers. */
  function failingDownload(failures: number) {
    const answer = fetch.getMockImplementation() as
      ((input: unknown, init: unknown) => Promise<Response>) | undefined;
    let failed = 0;
    fetch.mockImplementation((input: unknown, init: unknown) => {
      const url = new URL(input as string, "http://localhost");
      if (url.pathname.endsWith("/files/download") && failed < failures) {
        failed += 1;
        return Promise.reject(new TypeError("Failed to fetch"));
      }
      return answer?.(input, init);
    });
    return () =>
      fetch.mock.calls.filter(([input]) =>
        new URL(input as string, "http://localhost").pathname.endsWith(
          "/files/download",
        ),
      ).length;
  }
  const longPage = () => {
    const html = `${"<p>row</p>".repeat(60_000)}<script src="https://cdn.example.org/late.js"></script>`;
    files["long.html"] = {
      ...content("long.html", html.length, html.slice(0, 512 * 1024)),
      truncated: true,
    };
    downloads["long.html"] = html;
  };

  it("reads the page again once when its first read fails", async () => {
    longPage();
    const reads = failingDownload(1);
    show("long.html");
    expect(
      await screen.findByText(offline("cdn.example.org"), undefined, {
        timeout: 4000,
      }),
    ).toBeTruthy();
    expect(reads()).toBe(2);
  });

  it("reads it twice at most, and shows the bar alone when both reads fail", async () => {
    longPage();
    const reads = failingDownload(5);
    show("long.html");
    await screen.findByTitle("Preview of long.html");
    await waitFor(() => expect(reads()).toBe(2), { timeout: 4000 });
    // A third read would come 2 s after the second (the query's backoff): watch past it.
    await new Promise((settle) => setTimeout(settle, 3000));
    expect(reads()).toBe(2);
    expect(screen.queryByText(/Previews run offline/)).toBeNull();
  });
});

describe("WEB-U-088 a spreadsheet in the Files panel", () => {
  it("shows the sheet under column letters with numbered rows, cells as plain text", async () => {
    files["sales.xlsx"] = content("sales.xlsx", 5000, null);
    sheets[""] = {
      status: 200,
      body: sheet({
        rows: [
          ["Region", "https://example.invalid", "<b>bold</b>"],
          ["North", "12 000", '=HYPERLINK("x")'],
        ],
        total_rows: 2,
      }),
    };
    show("sales.xlsx");
    const table = await screen.findByRole("table", {
      name: "Sheet Totals of sales.xlsx",
    });
    const headers = within(table)
      .getAllByRole("columnheader")
      .map((cell) => cell.textContent);
    expect(headers).toEqual(["A", "B", "C"]);
    expect(
      within(table)
        .getAllByRole("rowheader")
        .map((cell) => cell.textContent),
    ).toEqual(["1", "2"]);
    expect(
      within(table).getByText("https://example.invalid").closest("td"),
    ).toBeTruthy();
    expect(within(table).queryByRole("link")).toBeNull();
    expect(within(table).getByText("<b>bold</b>")).toBeTruthy();

    expect(screen.getByText("2 rows · 3 columns")).toBeTruthy();
    // One sheet: no picker.
    expect(screen.queryByRole("group", { name: "Sheets" })).toBeNull();
  });

  // A screen review before 17.0: the sheet's first fact was its file size ("6 B"), and columns past the panel
  // were cut with no sign. It leads with its rows and columns, and the cut edge shows there is more.
  it("WEB-U-182 leads with rows and columns, not the size, and marks a table wider than its box", async () => {
    files["sales.xlsx"] = content("sales.xlsx", 5000, null);
    sheets[""] = {
      status: 200,
      body: sheet({
        rows: [
          ["Region", "Q1", "Q2", "Q3"],
          ["North", "10", "11", "12"],
        ],
        total_rows: 2,
      }),
    };
    const wide = vi
      .spyOn(HTMLElement.prototype, "scrollWidth", "get")
      .mockImplementation(function (this: HTMLElement) {
        return this.classList.contains("file-sheet") ? 900 : 0;
      });
    const room = vi
      .spyOn(HTMLElement.prototype, "clientWidth", "get")
      .mockImplementation(function (this: HTMLElement) {
        return this.classList.contains("file-sheet") ? 300 : 0;
      });
    show("sales.xlsx");
    await screen.findByRole("table", { name: "Sheet Totals of sales.xlsx" });
    expect(screen.getByText("2 rows · 4 columns")).toBeTruthy();
    // The size isn't the preview's headline.
    expect(screen.queryByText("4.9 kB")).toBeNull();
    expect(
      screen.getByRole("region", { name: "Sheet Totals of sales.xlsx" }),
    ).toHaveAttribute("data-more", "true");
    wide.mockRestore();
    room.mockRestore();
  });

  it("caps a long cell with its full text on hover, and opens the sheet wide on Expand", async () => {
    const long = "https://example.invalid/a/very/long/address/for/the/sales";
    files["sales.xlsx"] = content("sales.xlsx", 5000, null);
    sheets[""] = {
      status: 200,
      body: sheet({ rows: [["Source", long, "short"]] }),
    };
    show("sales.xlsx");
    const table = await screen.findByRole("table", {
      name: "Sheet Totals of sales.xlsx",
    });
    const cell = (text: string) => within(table).getByText(text).closest("td");
    expect(cell(long)?.getAttribute("title")).toBe(long);
    expect(cell("short")?.getAttribute("title")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Expand" }));
    const dialog = await screen.findByRole("dialog", { name: "sales.xlsx" });
    expect(
      within(dialog).getByRole("table", { name: "Sheet Totals of sales.xlsx" }),
    ).toBeTruthy();
  });

  it("aligns a column right when more than half its cells are numbers, its heading too", async () => {
    files["sales.xlsx"] = content("sales.xlsx", 5000, null);
    sheets[""] = {
      status: 200,
      body: sheet({
        rows: [
          ["Region", "Q1", "Note"],
          ["North", "12 000", "late"],
          ["South", "-3.5", "12%"],
        ],
      }),
    };
    show("sales.xlsx");
    const table = await screen.findByRole("table", {
      name: "Sheet Totals of sales.xlsx",
    });
    const column = (text: string) =>
      within(table).getByText(text).closest("td")?.className ?? "";
    // B: two numbers under a text heading, so all of it aligns right.
    for (const text of ["Q1", "12 000", "-3.5"])
      expect(column(text)).toBe("file-cell-number");
    expect(
      within(table).getByRole("columnheader", { name: "B" }).className,
    ).toBe("file-cell-number");
    // A (no numbers) and C (one number of three) stay left.
    for (const text of ["Region", "North", "Note", "late", "12%"])
      expect(column(text)).toBe("");
  });

  it("says how much of a large sheet it shows", async () => {
    files["big.ods"] = content("big.ods", 5000, null);
    const rows = Array.from({ length: 500 }, () =>
      Array.from({ length: 50 }, () => "7"),
    );
    sheets[""] = {
      status: 200,
      body: sheet({
        rows,
        total_rows: 12000,
        more_rows: true,
        more_columns: true,
        truncated_cells: 3,
      }),
    };
    show("big.ods");
    expect(
      await screen.findByText(
        "Showing 500 of 12 000 rows · the first 50 columns · 3 long cells cut short",
      ),
    ).toBeTruthy();
  });

  it("says 'the first' rows when the file doesn't declare how many there are", async () => {
    files["big.xlsx"] = content("big.xlsx", 5000, null);
    sheets[""] = {
      status: 200,
      body: sheet({
        rows: [["1"], ["2"]],
        total_rows: null,
        more_rows: true,
      }),
    };
    show("big.xlsx");
    expect(
      await screen.findByText("Showing the first 2 rows · 1 column"),
    ).toBeTruthy();
  });

  it("switches sheets with a row of buttons, and with a list from 8 sheets", async () => {
    files["book.xlsx"] = content("book.xlsx", 5000, null);
    sheets[""] = {
      status: 200,
      body: sheet({ sheets: ["Totals", "Notes"] }),
    };
    sheets["Notes"] = {
      status: 200,
      body: sheet({
        sheets: ["Totals", "Notes"],
        sheet: "Notes",
        rows: [["n"]],
      }),
    };
    const { unmount } = show("book.xlsx");
    const picker = await screen.findByRole("group", { name: "Sheets" });
    expect(
      within(picker)
        .getByRole("button", { name: "Totals" })
        .getAttribute("aria-pressed"),
    ).toBe("true");
    fireEvent.click(within(picker).getByRole("button", { name: "Notes" }));
    expect(
      await screen.findByRole("table", { name: "Sheet Notes of book.xlsx" }),
    ).toBeTruthy();
    expect(sheetCalls().at(-1)?.searchParams.get("sheet")).toBe("Notes");
    unmount();
    const many = Array.from({ length: 8 }, (_sheet, index) => `S${index + 1}`);
    sheets[""] = {
      status: 200,
      body: sheet({ sheets: many, sheet: "S1" }),
    };
    sheets["S8"] = {
      status: 200,
      body: sheet({ sheets: many, sheet: "S8", rows: [["eight"]] }),
    };
    show("book.xlsx");
    // The standard picker from 8 sheets (WEB-U-134).
    await screen.findByRole("button", { name: /^Sheets: / });
    choose("Sheets", /^S8$/);
    expect(await screen.findByText("eight")).toBeTruthy();
  });

  it("goes back to the first sheet when the chosen one is gone", async () => {
    files["book.xlsx"] = content("book.xlsx", 5000, null);
    sheets[""] = {
      status: 200,
      body: sheet({ sheets: ["Totals", "Old"] }),
    };
    show("book.xlsx");
    fireEvent.click(await screen.findByRole("button", { name: "Old" }));
    // "Old" answers 404 (the file changed): the first sheet shows again.
    expect(
      await screen.findByRole("table", { name: "Sheet Totals of book.xlsx" }),
    ).toBeTruthy();
    expect(sheetCalls().map((url) => url.searchParams.get("sheet"))).toEqual([
      null,
      "Old",
      null,
    ]);
  });

  it("says an empty sheet is empty", async () => {
    files["empty.xlsx"] = content("empty.xlsx", 5000, null);
    sheets[""] = { status: 200, body: sheet({ rows: [], total_rows: 0 }) };
    show("empty.xlsx");
    expect(await screen.findByText("This sheet is empty.")).toBeTruthy();
  });

  it("doesn't fetch a file over the preview limit", async () => {
    const size = LIMITS.preview_sheet_bytes + 1;
    files["huge.xlsx"] = content("huge.xlsx", size, null);
    show("huge.xlsx");
    expect(
      await screen.findByText(
        "This spreadsheet is too large to preview (10 MB of 10 MB). Download it to open it.",
      ),
    ).toBeTruthy();
    expect(sheetCalls()).toHaveLength(0);
  });

  it.each([
    [
      error(413, "preview_too_large"),
      "This spreadsheet is too large to preview. Download it to open it.",
    ],
    [
      error(422, "preview_damaged"),
      "This spreadsheet can't be shown: it's damaged.",
    ],
    [
      error(422, "preview_protected"),
      "This spreadsheet can't be shown: it's protected with a password.",
    ],
    [error(404, "not_found"), "This file is no longer there."],
    [
      error(500, "internal"),
      "This spreadsheet couldn't be loaded. Download it to open it.",
    ],
  ])("says what went wrong: %j", async (answer, words) => {
    files["x.xlsx"] = content("x.xlsx", 5000, null);
    sheets[""] = answer;
    show("x.xlsx");
    expect(await screen.findByText(words)).toBeTruthy();
    expect(screen.getByRole("link", { name: "Download" })).toBeTruthy();
  });

  it("offers Expand only for pages and sheets", async () => {
    files["notes.txt"] = content("notes.txt", 5, "hello");
    show("notes.txt");
    await screen.findByText("hello");
    expect(screen.queryByRole("button", { name: "Expand" })).toBeNull();
  });

  // WEB-U-255 (a pass over the pages before 17.0): a file too large to preview shows only a note, so it offers no
  // Expand.
  it.each([
    ["big.html", LIMITS.preview_html_bytes, "This page is too large"],
    ["huge.xlsx", LIMITS.preview_sheet_bytes, "This spreadsheet is too large"],
    ["long.pdf", LIMITS.preview_pdf_bytes, "This PDF is too large"],
  ])("offers no Expand for %s over its limit", async (path, limit, words) => {
    files[path] = content(path, limit + 1, null);
    show(path);
    expect(await screen.findByText(new RegExp(`^${words}`))).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Expand" })).toBeNull();
    expect(screen.getByRole("link", { name: "Download" })).toBeTruthy();
  });

  it("gives legacy .xls no table, only the download", async () => {
    files["old.xls"] = content("old.xls", 5000, null);
    show("old.xls");
    expect(
      await screen.findByText(
        "This file can't be previewed. Download it to open it.",
      ),
    ).toBeTruthy();
    expect(sheetCalls()).toHaveLength(0);
  });
});

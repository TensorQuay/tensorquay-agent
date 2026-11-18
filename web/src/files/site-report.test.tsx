import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import en from "../i18n/locales/en.json";
import zh from "../i18n/locales/zh-Hans.json";
import type { FileContent, SitePass } from "../protocol/generated/api";
import { conversationId } from "../test/frames";
import { LIMITS } from "../test/user";

import { FilePreview } from "./FilePreview";
import { siteReportLine, siteReportPart } from "./site-report";

const base = `/api/v1/conversations/${conversationId}/site/p4ss/`;
let refused = false;
const fetch = vi.fn();

beforeEach(() => {
  refused = false;
  fetch.mockReset().mockImplementation((input, init?: RequestInit) => {
    const url = new URL(input as string, "http://localhost");
    if (url.pathname.endsWith("/site") && init?.method === "POST")
      return Promise.resolve(
        refused
          ? Response.json(
              { error: { code: "not_found", message: "no", retryable: false } },
              { status: 404 },
            )
          : Response.json(
              {
                pass: "p4ss",
                folder: "dist",
                url: `${base}dist/`,
                expires_in_seconds: 900,
                network: { any: false, hosts: [] },
              } satisfies SitePass,
              { status: 201 },
            ),
      );
    return Promise.resolve(
      Response.json(
        {
          path: "dist/index.html",
          size: 40,
          modified_at: "2026-01-01T00:00:00Z",
          etag: "e1",
          binary: false,
          truncated: false,
          text: '<div id="root"></div><script src="./a.js"></script>',
        } satisfies FileContent,
        { status: 200 },
      ),
    );
  });
  vi.stubGlobal("fetch", fetch);
});
afterEach(() => vi.unstubAllGlobals());

function show() {
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
      path="dist/index.html"
      onBack={vi.fn()}
    />,
    {
      wrapper,
    },
  );
}

async function framed() {
  const frame = await screen.findByTitle<HTMLIFrameElement>(
    "Preview of index.html",
  );
  await waitFor(() =>
    expect(frame.getAttribute("src")).toBe(`${base}dist/index.html`),
  );
  return frame;
}

function post(
  data: unknown,
  source: MessageEventSource | null,
  origin = "null",
) {
  act(() => {
    window.dispatchEvent(new MessageEvent("message", { data, origin, source }));
  });
}

const report = (kind: string, paths?: unknown, more?: unknown) => ({
  type: "tqa-site-report",
  kind,
  paths,
  more,
});

// 18.1, built web apps preview as they are: the page's reporter (GW-U-222) tells the panel what the page couldn't
// load, and the panel says so on a muted line under its bar instead of leaving a blank frame unexplained.
describe("WEB-U-500 the panel says what a previewed page couldn't load", () => {
  it("names three root paths and counts the rest, then says what to ask", async () => {
    show();
    const frame = await framed();
    post(
      report("root", ["/a.png", "/b.png", "/c.png", "/d.png", "/e.png"], 3),
      frame.contentWindow,
    );
    const line = await screen.findByRole("status");
    expect(line.textContent).toBe(
      "This page asks for files at the server's root (/a.png, /b.png, /c.png, and 5 more), which a preview can't " +
        "serve. Ask the agent to build it with relative paths.",
    );
  });

  it("shows one line: root paths before missing files before a page that drew nothing", async () => {
    show();
    const frame = await framed();
    post(report("empty"), frame.contentWindow);
    expect((await screen.findByRole("status")).textContent).toMatch(
      /^This page drew nothing\./,
    );
    post(report("missing", ["assets/chart.js"]), frame.contentWindow);
    expect((await screen.findByRole("status")).textContent).toBe(
      "This page asks for files its folder doesn't have: assets/chart.js.",
    );
    post(report("root", []), frame.contentWindow);
    expect(screen.getAllByRole("status")).toHaveLength(1);
    expect(screen.getByRole("status").textContent).toBe(
      "This page asks for files at the server's root, which a preview can't serve. Ask the agent to build it with " +
        "relative paths.",
    );
  });

  it("ignores a message from anything but this frame, from a real origin, of another type, or naming nothing", async () => {
    show();
    const frame = await framed();
    post(report("root", ["/a.png"]), window);
    post(
      report("root", ["/a.png"]),
      frame.contentWindow,
      "https://example.org",
    );
    post({ ...report("root", ["/a.png"]), type: "other" }, frame.contentWindow);
    post(report("missing", []), frame.contentWindow);
    post(report("unknown", ["/a.png"]), frame.contentWindow);
    expect(screen.queryByRole("status")).toBeNull();
  });

  it("starts again with nothing said after Reload and after Stop scripts", async () => {
    show();
    let frame = await framed();
    post(report("empty"), frame.contentWindow);
    await screen.findByRole("status");
    fireEvent.click(screen.getByRole("button", { name: "Reload" }));
    expect(screen.queryByRole("status")).toBeNull();
    frame = screen.getByTitle<HTMLIFrameElement>("Preview of index.html");
    post(report("empty"), frame.contentWindow);
    await screen.findByRole("status");
    fireEvent.click(screen.getByRole("button", { name: "Stop scripts" }));
    expect(screen.queryByRole("status")).toBeNull();
  });

  it("says nothing in the single-page view, where no reporter runs", async () => {
    refused = true;
    show();
    const frame = await screen.findByTitle<HTMLIFrameElement>(
      "Preview of index.html",
    );
    post(report("empty"), frame.contentWindow);
    expect(screen.queryByRole("status")).toBeNull();
  });

  it("reads a report as bounded text: ten paths of 120 characters, the count of the rest", () => {
    const long = "/" + "x".repeat(300);
    expect(
      siteReportPart(
        report(
          "root",
          [long, 7, null, ...Array.from({ length: 12 }, (_, i) => `/p${i}`)],
          2.5,
        ),
      ),
    ).toEqual({
      root: {
        paths: [
          long.slice(0, 120),
          ...Array.from({ length: 9 }, (_, i) => `/p${i}`),
        ],
        more: 0,
      },
    });
    expect(siteReportPart(report("missing", ["a.js"], 4))).toEqual({
      missing: { paths: ["a.js"], more: 4 },
    });
    expect(siteReportPart(null)).toBeNull();
    expect(siteReportPart("tqa-site-report")).toBeNull();
    expect(siteReportLine({})).toBeNull();
  });

  it("has its words in English and Chinese", () => {
    for (const catalogue of [en, zh] as Record<string, unknown>[]) {
      const words = (catalogue.messages ?? catalogue) as Record<string, string>;
      for (const key of [
        "files.siteRootPaths",
        "files.siteRootPathsUnnamed",
        "files.siteMissing",
        "files.siteEmpty",
      ])
        expect(typeof words[key], key).toBe("string");
    }
  });
});

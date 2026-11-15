import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { FileContent, SitePass } from "../protocol/generated/api";
import { conversationId } from "../test/frames";
import { LIMITS } from "../test/user";

import { FilePreview } from "./FilePreview";

const page = (path: string, text: string) =>
  ({
    path,
    size: text.length,
    modified_at: "2026-01-01T00:00:00Z",
    etag: "e1",
    binary: false,
    truncated: false,
    text,
  }) satisfies FileContent;

const base = `/api/v1/conversations/${conversationId}/site/p4ss/`;
let pass: { status: number; body: unknown };
let files: Record<string, FileContent>;
const fetch = vi.fn();
const mints = () =>
  fetch.mock.calls.filter(([input]) =>
    new URL(input as string, "http://localhost").pathname.endsWith("/site"),
  );

beforeEach(() => {
  files = {};
  pass = {
    status: 201,
    body: {
      pass: "p4ss",
      folder: "sg-weather-map",
      url: `${base}sg-weather-map/`,
      expires_in_seconds: 900,
      network: { any: false, hosts: ["unpkg.com"] },
    } satisfies SitePass,
  };
  fetch.mockReset().mockImplementation((input, init?: RequestInit) => {
    const url = new URL(input as string, "http://localhost");
    if (url.pathname.endsWith("/site") && init?.method === "POST")
      return Promise.resolve(Response.json(pass.body, { status: pass.status }));
    return Promise.resolve(
      Response.json(files[url.searchParams.get("path") ?? ""], { status: 200 }),
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

const weather =
  '<link rel="stylesheet" href="style.css"><script src="https://unpkg.com/three@0.170.0/build/three.module.js"></script>' +
  "<script>fetch('https://api.data.gov.sg/v1/environment/air-temperature')</script>";

// The owner (12 Nov 2026): "most of sites has many files, not single page". A page is framed from its folder served
// as a site, so its own CSS, scripts, pictures and links load; outside hosts follow the install's network setting.
describe("WEB-U-470 a web page is previewed with its folder's files", () => {
  it("frames the page from its folder's site address, with scripts on and only allow-scripts", async () => {
    files["sg-weather-map/index.html"] = page(
      "sg-weather-map/index.html",
      weather,
    );
    show("sg-weather-map/index.html");
    const frame = await screen.findByTitle("Preview of index.html");
    await waitFor(() =>
      expect(frame.getAttribute("src")).toBe(
        `${base}sg-weather-map/index.html`,
      ),
    );
    expect(frame.getAttribute("sandbox")).toBe("allow-scripts");
    expect(frame.getAttribute("referrerpolicy")).toBe("no-referrer");
    const [[, init]] = mints() as [[string, RequestInit]];
    expect(JSON.parse(init.body as string)).toEqual({
      folder: "sg-weather-map",
    });
  });

  it("opens a page that isn't the folder's index by its own name, and a page at the top by the root", async () => {
    pass.body = { ...(pass.body as SitePass), folder: "", url: base };
    files["My report.html"] = page("My report.html", "<p>hi</p>");
    show("My report.html");
    const frame = await screen.findByTitle("Preview of My report.html");
    await waitFor(() =>
      expect(frame.getAttribute("src")).toBe(`${base}My%20report.html`),
    );
    const [[, init]] = mints() as [[string, RequestInit]];
    expect(JSON.parse(init.body as string)).toEqual({ folder: "" });
  });

  it("Stop scripts reloads the page without them, and Run scripts brings them back", async () => {
    files["site/index.html"] = page("site/index.html", "<p>hi</p>");
    show("site/index.html");
    await screen.findByTitle("Preview of index.html");
    fireEvent.click(screen.getByRole("button", { name: "Stop scripts" }));
    const stopped = screen.getByTitle("Preview of index.html");
    expect(stopped.getAttribute("sandbox")).toBe("");
    expect(screen.getByText("Scripts are stopped")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Run scripts" }));
    expect(
      screen.getByTitle("Preview of index.html").getAttribute("sandbox"),
    ).toBe("allow-scripts");
  });

  it("names the hosts the network setting blocks, and none that it allows", async () => {
    files["sg-weather-map/index.html"] = page(
      "sg-weather-map/index.html",
      weather,
    );
    show("sg-weather-map/index.html");
    const line = await screen.findByText(/^Blocked by the network setting:/);
    expect(line.textContent).toBe(
      "Blocked by the network setting: api.data.gov.sg",
    );
    expect(line.textContent).not.toContain("unpkg.com");
    expect(line.querySelector("a")).toBeNull();
  });

  it("says nothing of hosts when the network setting reaches any host", async () => {
    pass.body = {
      ...(pass.body as SitePass),
      network: { any: true, hosts: [] },
    };
    files["sg-weather-map/index.html"] = page(
      "sg-weather-map/index.html",
      weather,
    );
    show("sg-weather-map/index.html");
    const frame = await screen.findByTitle("Preview of index.html");
    await waitFor(() => expect(frame.getAttribute("src")).toContain("/site/"));
    expect(screen.queryByText(/Blocked by the network setting/)).toBeNull();
    expect(screen.queryByText(/Previews run offline/)).toBeNull();
  });

  it("falls back to the single-page view when the site can't be opened", async () => {
    pass = {
      status: 409,
      body: {
        error: {
          code: "site_passes_limit",
          message: "limit",
          retryable: false,
        },
      },
    };
    files["out/report.html"] = page("out/report.html", "<p>hi</p>");
    show("out/report.html");
    const frame = await screen.findByTitle("Preview of report.html");
    await waitFor(() =>
      expect(
        new URL(frame.getAttribute("src") ?? "", "http://localhost").pathname,
      ).toBe(`/api/v1/conversations/${conversationId}/files/view`),
    );
  });
});

import { afterEach, describe, expect, it, vi } from "vitest";

import { openEventStream, openScreenSocket } from "./stream";

afterEach(() => {
  vi.unstubAllGlobals();
});

// GW-U-027: the two connections the page opens outside the transport, each on the page's own origin.
describe("GW-U-027 the page's own connections", () => {
  it("opens the screen as a WebSocket on the same origin, ws or wss as the page is", () => {
    const opened: string[] = [];
    vi.stubGlobal(
      "WebSocket",
      class {
        constructor(url: URL) {
          opened.push(url.toString());
        }
      },
    );
    openScreenSocket("/api/v1/conversations/task-1/screen");
    expect(opened).toEqual([
      `ws://${window.location.host}/api/v1/conversations/task-1/screen`,
    ]);
  });

  it("opens the event stream at its path", () => {
    const opened: string[] = [];
    vi.stubGlobal(
      "EventSource",
      class {
        constructor(url: string) {
          opened.push(url);
        }
      },
    );
    openEventStream("/api/v1/conversations/task-1/events?stream=1");
    expect(opened).toEqual(["/api/v1/conversations/task-1/events?stream=1"]);
  });
});

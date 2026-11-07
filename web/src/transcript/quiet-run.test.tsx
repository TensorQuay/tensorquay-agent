import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { api } from "../api/client";
import type { Conversation } from "../protocol/generated/api";
import { conversationId, frame, ready, runId } from "../test/frames";
import { templates } from "../test/templates";
import { shellData } from "../test/workspaces";

import { LiveConversation } from "./LiveConversation";

// The page's other reads, none of them the stream under test.
vi.mock("./use-held", () => ({
  useHeldMessages: () => ({
    held: [],
    waiting: [],
    listed: null,
    dropped: [],
    notice: null,
    remove: vi.fn(),
    refresh: vi.fn(),
  }),
}));
vi.mock("../auth/use-session", () => ({
  useSession: () => ({ data: undefined }),
}));

/** The browser's event stream, as the page opens it: what the gateway sends, and nothing for its keep-alives. */
class Stream extends EventTarget {
  static opened: Stream[] = [];
  readyState = 0;
  constructor(readonly url: string) {
    super();
    Stream.opened.push(this);
  }
  close() {
    this.readyState = 2;
  }
  open() {
    this.readyState = 1;
    this.dispatchEvent(new Event("open"));
  }
  send(value: unknown) {
    this.dispatchEvent(
      new MessageEvent("message", { data: JSON.stringify(value) }),
    );
  }
}

const conversation: Conversation = {
  id: conversationId,
  owner_id: conversationId,
  title: "Long build",
  model_route: "local",
  template: "workstation",
  pinned: false,
  title_source: "person",
  backend: "docker",
  state: "active",
  created_at: "2026-01-01T00:00:00Z",
  updated_at: "2026-01-01T00:00:00Z",
  last_activity_at: "2026-01-01T00:00:00Z",
  workspace_id: "0195d8c0-0000-7000-8000-0000000000ff",
  mcp_servers: [],
  connectors: [],
  team: null,
};

beforeEach(() => {
  Stream.opened = [];
  vi.useFakeTimers();
  vi.stubGlobal("EventSource", Stream);
  vi.spyOn(api, "files").mockResolvedValue({
    path: "",
    items: [],
    next_after: null,
  });
  vi.spyOn(api, "keyUses").mockResolvedValue({ items: [], next: null });
});
afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

// The gateway keeps a quiet run's stream open with keep-alive comments (a long tool call sends no frames for minutes);
// the browser never passes a comment to the page. So the page sees nothing at all, and must stay live: not reconnect,
// not say "Reconnecting…", not open a second stream.
describe("WEB-U-217 a task page stays live through a quiet run", () => {
  it("keeps one stream and its live state through two minutes without a frame", async () => {
    const cache = new QueryClient();
    cache.setQueryData(["templates"], templates);
    shellData(cache);
    cache.setQueryData(["models"], { items: [] });
    render(<LiveConversation conversation={conversation} />, {
      wrapper: ({ children }) => (
        <QueryClientProvider client={cache}>{children}</QueryClientProvider>
      ),
    });
    const stream = Stream.opened[0];
    expect(stream).toBeDefined();
    await act(async () => {
      stream?.open();
      stream?.send(ready);
      stream?.send(frame(1, { type: "run.started", run_id: runId }));
      stream?.send(frame(2, { type: "step.started", step: 1, attempt: 1 }));
      await vi.advanceTimersByTimeAsync(100);
    });
    expect(screen.getByLabelText("Run status")).toHaveTextContent(/^Working…/);
    // Two quiet minutes: only keep-alives, which reach no listener.
    await act(() => vi.advanceTimersByTimeAsync(120_000));
    expect(Stream.opened).toHaveLength(1);
    expect(stream?.readyState).toBe(1);
    expect(screen.queryByText("Reconnecting to the task…")).toBeNull();
    expect(screen.getByLabelText("Run status")).toHaveTextContent(/^Working…/);
  });
});

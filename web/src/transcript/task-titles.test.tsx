import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { api } from "../api/client";
import type { Conversation } from "../protocol/generated/api";
import { parseFrame } from "../protocol/validate";
import { conversationId } from "../test/frames";
import { templates } from "../test/templates";
import { shellData } from "../test/workspaces";

import { LiveConversation } from "./LiveConversation";
import { apply, emptyTranscript } from "./reducer";
import type { Transcript } from "./types";
import { useTranscript } from "./use-transcript";

vi.mock("./use-transcript", () => ({ useTranscript: vi.fn() }));
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

const conversation: Conversation = {
  id: conversationId,
  owner_id: conversationId,
  title: "Clean up sales.csv: remove exact duplicate rows, make every…",
  title_source: "message",
  model_route: "local",
  template: "workstation",
  pinned: false,
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
const titled = () =>
  apply(
    { ...emptyTranscript(), sandbox: "ready" },
    parseFrame(
      JSON.stringify({ type: "task.titled", title: "Sales CSV cleanup" }),
    ),
  );

let fetch: ReturnType<typeof vi.fn>;
beforeEach(() => {
  fetch = vi.fn(() =>
    Promise.resolve(
      Response.json({
        ...conversation,
        title: "Sales CSV cleanup",
        title_source: "model",
      }),
    ),
  );
  vi.stubGlobal("fetch", fetch);
  // The task's stored key uses, read at load (WEB-U-193): not a read of the task these tests count.
  vi.spyOn(api, "keyUses").mockResolvedValue({ items: [], next: null });
});
afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

function page(
  transcript: Transcript,
  task: Conversation = conversation,
  connection: "live" | "reconnecting" = "live",
  prepare?: (cache: QueryClient) => void,
) {
  vi.mocked(useTranscript).mockReturnValue({ transcript, connection });
  const cache = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  cache.setQueryData(["templates"], templates);
  shellData(cache);
  cache.setQueryData(["models"], { items: [] });
  cache.setQueryData(["conversation", task.id], task);
  prepare?.(cache);
  const view = render(<LiveConversation conversation={task} />, {
    wrapper: ({ children }) => (
      <QueryClientProvider client={cache}>{children}</QueryClientProvider>
    ),
  });
  return { cache, view };
}

// A live check before 17.0: the gateway names a task in 3 to 6 words after its first completed run; the page shows
// the name at once, and a person's rename is never replaced.
describe("WEB-U-081 short task titles", () => {
  it("records the name from task.titled without moving the sequence or the transcript", () => {
    const before = { ...emptyTranscript(), sandbox: "ready" as const };
    const after = titled();
    expect(after.title).toBe("Sales CSV cleanup");
    expect(after.lastSeq).toBe(before.lastSeq);
    expect(after.items).toEqual(before.items);
  });

  it("shows the name in the header and the tab, and marks it the model's", async () => {
    const { cache } = page(titled());
    expect(
      await screen.findByRole("heading", {
        level: 1,
        name: "Sales CSV cleanup",
      }),
    ).toBeVisible();
    await waitFor(() =>
      expect(document.title).toBe("Sales CSV cleanup · TensorQuay Agent"),
    );
    expect(
      cache.getQueryData<Conversation>(["conversation", conversationId]),
    ).toMatchObject({ title: "Sales CSV cleanup", title_source: "model" });
  });

  it("refreshes the task list so the sidebar row reads the name too", async () => {
    const spy: { invalidate?: unknown } = {};
    page(titled(), conversation, "live", (cache) => {
      spy.invalidate = vi.spyOn(cache, "invalidateQueries");
    });
    await waitFor(() =>
      expect(spy.invalidate).toHaveBeenCalledWith({
        queryKey: ["conversations"],
      }),
    );
  });

  it("never replaces a title the person chose", async () => {
    const renamed: Conversation = {
      ...conversation,
      title: "Q3 numbers",
      title_source: "person",
    };
    const { cache } = page(titled(), renamed);
    expect(
      await screen.findByRole("heading", { level: 1, name: "Q3 numbers" }),
    ).toBeVisible();
    expect(
      cache.getQueryData<Conversation>(["conversation", conversationId]),
    ).toMatchObject({ title: "Q3 numbers", title_source: "person" });
  });

  it("rereads the task when the stream comes back, since a name sent meanwhile isn't replayed", async () => {
    const { view } = page(
      { ...emptyTranscript(), sandbox: "ready" },
      conversation,
      "reconnecting",
    );
    expect(fetch).not.toHaveBeenCalled();
    vi.mocked(useTranscript).mockReturnValue({
      transcript: { ...emptyTranscript(), sandbox: "ready" },
      connection: "live",
    });
    view.rerender(<LiveConversation conversation={conversation} />);
    await waitFor(() =>
      expect(
        fetch.mock.calls.some(([url]) =>
          String(url).endsWith(`/api/v1/conversations/${conversationId}`),
        ),
      ).toBe(true),
    );
    expect(
      await screen.findByRole("heading", {
        level: 1,
        name: "Sales CSV cleanup",
      }),
    ).toBeVisible();
  });
});

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { api } from "../api/client";
import { ApiError } from "../api/error";
import type { Conversation, FilePage } from "../protocol/generated/api";
import { conversationId } from "../test/frames";
import { templates } from "../test/templates";
import { shellData } from "../test/workspaces";

import { LiveConversation } from "./LiveConversation";
import { emptyTranscript } from "./reducer";
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

const conversation = {
  id: conversationId,
  owner_id: conversationId,
  title: "Workspace review",
  model_route: "local",
  template: "workstation",
  pinned: false,
  title_source: "message",
  backend: "docker",
  state: "active",
  created_at: "2026-01-01T00:00:00Z",
  updated_at: "2026-01-01T00:00:00Z",
  last_activity_at: "2026-01-01T00:00:00Z",
  workspace_id: "0195d8c0-0000-7000-8000-0000000000ff",
  mcp_servers: [],
  connectors: [],
} as unknown as Conversation;

const root = (names: string[]): FilePage => ({
  path: "",
  items: names.map((name) => ({
    name,
    kind: "file",
    size: 57,
    modified_at: "2026-01-01T00:00:00Z",
  })),
  next_after: null,
});
function page() {
  const cache = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  cache.setQueryData(["templates"], templates);
  shellData(cache);
  return render(
    <QueryClientProvider client={cache}>
      <LiveConversation conversation={conversation} />
    </QueryClientProvider>,
  );
}
beforeEach(() => {
  vi.mocked(useTranscript).mockReturnValue({
    transcript: { ...emptyTranscript(), sandbox: "ready" },
    connection: "live",
  });
});
afterEach(() => vi.restoreAllMocks());

// The 13f screen review, #1: a task opened on a workspace with files said "Your workspace starts empty." beside a Files
// panel listing them. The line says so only when the workspace's top folder is empty.
describe("WEB-U-152 a new task's first line is true to its workspace", () => {
  it("says the workspace starts empty when it is", async () => {
    vi.spyOn(api, "files").mockResolvedValue(root([]));
    page();
    expect(
      await screen.findByText(
        "Send a message to begin. Your workspace starts empty.",
      ),
    ).toBeVisible();
  });

  it("says only how to begin when the workspace has files", async () => {
    vi.spyOn(api, "files").mockResolvedValue(root(["README.md", ".git"]));
    page();
    expect(await screen.findByText("Send a message to begin.")).toBeVisible();
    expect(screen.queryByText(/starts empty/)).toBeNull();
  });

  it("says only how to begin when the files can't be listed", async () => {
    vi.spyOn(api, "files").mockRejectedValue(
      new ApiError(503, "sandbox_unavailable", "Not ready."),
    );
    page();
    expect(await screen.findByText("Send a message to begin.")).toBeVisible();
    expect(screen.queryByText(/starts empty/)).toBeNull();
  });
});

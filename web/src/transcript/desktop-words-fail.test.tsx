import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { api } from "../api/client";
import type { Conversation } from "../protocol/generated/api";
import { conversationId, frame, runId } from "../test/frames";
import { templates } from "../test/templates";
import { shellData } from "../test/workspaces";

import { LiveConversation } from "./LiveConversation";
import { apply, emptyTranscript } from "./reducer";
import { onDesktopWords } from "./tool-summary";
import { useTranscript } from "./use-transcript";

vi.mock("./use-transcript", () => ({ useTranscript: vi.fn() }));
const ME = "0195d8c0-0000-7000-8000-0000000000a1";
vi.mock("../auth/use-session", () => ({
  useSession: () => ({
    data: {
      id: ME,
      limits: { upload_bytes: 1024 },
      features: { agent_teams: false },
    },
  }),
}));
// The words' chunk fails once (a tab open across a deploy: its chunk 404s), then loads.
let attempts = 0;
vi.mock("../screen/desktop-words", async (importOriginal) => {
  attempts += 1;
  if (attempts === 1)
    throw new Error("Failed to fetch dynamically imported module");
  return importOriginal();
});

const desktop = {
  name: "desktop",
  display_name: "Desktop",
  capabilities: ["shell", "desktop"],
  is_default: false,
  description: null,
  cpus: 2,
  memory_mib: 4096,
};
const task: Conversation = {
  id: conversationId,
  owner_id: ME,
  title: "Fill in the form",
  model_route: "local",
  template: "desktop",
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
  team: null,
};
const clicked = [
  frame(1, { type: "message.user", text: "Fill in the form", attachments: [] }),
  frame(2, { type: "run.started", run_id: runId }),
  frame(3, { type: "step.started", step: 1, attempt: 1 }),
  frame(4, {
    type: "tool.call",
    call_id: "call-click",
    name: "computer",
    arguments: { action: "click", ref: "e3" },
  }),
  frame(5, { type: "tool.start", call_id: "call-click" }),
  frame(6, { type: "tool.end", call_id: "call-click", status: "ok" }),
  frame(7, {
    type: "tool.result",
    call_id: "call-click",
    status: "ok",
    content: "Clicked e3.",
    target: { name: "Save", role: "button" },
  }),
].reduce(apply, emptyTranscript());

beforeEach(() => {
  vi.spyOn(api, "files").mockResolvedValue({
    path: "",
    items: [],
    next_after: null,
  });
  vi.spyOn(api, "queuedMessages").mockResolvedValue({ items: [] });
});
afterEach(() => vi.restoreAllMocks());

// WEB-U-422: a Desktop task's steps wait for the Desktop tool's words; if their chunk fails to load, the steps show with
// the general label rather than staying hidden, and the words are asked for again (as the failure copy, WEB-U-392).
describe("WEB-U-422 a Desktop task whose words fail to load", () => {
  it("shows its steps with the general label, then their words once a retry loads them", async () => {
    vi.mocked(useTranscript).mockReturnValue({
      transcript: clicked,
      connection: "live",
    });
    const cache = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    cache.setQueryData(["templates"], {
      items: [...templates.items, desktop],
    });
    shellData(cache);
    cache.setQueryData(["models"], { items: [] });
    // The page hears both: the failure (its steps show) and the retry's arrival (their words), even with nothing else
    // to render it again.
    const heard = vi.fn();
    const unsubscribe = onDesktopWords(heard);
    const seen: string[] = [];
    const watch = new MutationObserver(() => {
      for (const label of document.querySelectorAll(".activity-label"))
        seen.push(label.textContent ?? "");
    });
    watch.observe(document.body, {
      childList: true,
      subtree: true,
      characterData: true,
    });
    render(
      <QueryClientProvider client={cache}>
        <LiveConversation conversation={task} />
      </QueryClientProvider>,
    );
    await waitFor(() =>
      expect(screen.getByText("Clicked “Save”")).toBeVisible(),
    );
    watch.disconnect();
    unsubscribe();
    expect(attempts).toBeGreaterThanOrEqual(2);
    expect(heard).toHaveBeenCalledTimes(2);
    // Never empty, raw or undefined: the general label, then the words.
    expect(
      seen.filter(
        (label) => label !== "Used the desktop" && label !== "Clicked “Save”",
      ),
    ).toEqual([]);
  });
});

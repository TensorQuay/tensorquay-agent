import { readFileSync } from "node:fs";
import { join } from "node:path";

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

const DESKTOP_KEYS =
  /^tool\.computer(?!Other$)|^screen\.(named|someElement|role)/;
const catalogue = (area: string) =>
  (
    JSON.parse(
      readFileSync(join("src", "i18n", "generated", `en.${area}.json`), "utf8"),
    ) as { messages: Record<string, string> }
  ).messages;

const desktop = {
  name: "desktop",
  display_name: "Desktop",
  capabilities: ["shell", "desktop"],
  is_default: false,
  description: null,
  cpus: 2,
  memory_mib: 4096,
};
const task = (template: string): Conversation => ({
  id: conversationId,
  owner_id: ME,
  title: "Fill in the form",
  model_route: "local",
  template,
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
});
// A run that clicked a named button on the desktop, as the stream replays it to a page opened cold.
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

function open(template: string) {
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
  // Every label the step shows, from its first render on.
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
      <LiveConversation conversation={task(template)} />
    </QueryClientProvider>,
  );
  return { seen, stop: () => watch.disconnect() };
}

beforeEach(() => {
  vi.spyOn(api, "files").mockResolvedValue({
    path: "",
    items: [],
    next_after: null,
  });
  vi.spyOn(api, "queuedMessages").mockResolvedValue({ items: [] });
});
afterEach(() => vi.restoreAllMocks());

// WEB-U-422 (17.0's release candidate): the Desktop tool's step words (a click, a key, a scroll, an element's
// role) are only ever shown on a Desktop task, so they load with the screen area, off the conversation page's first
// load; a Desktop task asks for them as the page opens, so its steps never show an empty, raw or stand-in label.
describe("WEB-U-422 the Desktop tool's step words leave the first load", () => {
  it("keeps the Desktop tool's words out of the core catalogue, in the screen area's", () => {
    const core = Object.keys(catalogue("core"));
    expect(core.filter((key) => DESKTOP_KEYS.test(key))).toEqual([]);
    const area = catalogue("screen");
    expect(area["tool.computerClickOn"]).toBeDefined();
    expect(area["screen.roleButton"]).toBe("a button");
    // The one stand-in a step can need while the words load stays core.
    expect(catalogue("core")["tool.computerOther"]).toBe("Used the desktop");
  });

  it("labels a cold Desktop task's step with its words from its first render", async () => {
    const { seen, stop } = open("desktop");
    await waitFor(() =>
      expect(screen.getByText("Clicked “Save”")).toBeVisible(),
    );
    stop();
    expect(seen.length).toBeGreaterThan(0);
    expect(seen.filter((label) => label !== "Clicked “Save”")).toEqual([]);
  });

  it("labels a computer step in a task without a declared desktop once the words arrive", async () => {
    const { stop } = open("workstation");
    await waitFor(() =>
      expect(screen.getByText("Clicked “Save”")).toBeVisible(),
    );
    stop();
    expect(document.body).not.toHaveTextContent(/tool\.computer|undefined/);
  });
});

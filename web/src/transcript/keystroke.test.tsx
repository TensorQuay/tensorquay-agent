import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { createElement, type ComponentType } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { Conversation } from "../protocol/generated/api";
import { conversationId, transcriptFrames } from "../test/frames";
import { templates } from "../test/templates";
import { shellData } from "../test/workspaces";

import { LiveConversation } from "./LiveConversation";
import { apply, emptyTranscript } from "./reducer";
import type { Transcript } from "./types";
import { useTranscript } from "./use-transcript";

/** How often each watched part rendered since the counts were last cleared. */
const renders: Record<string, number> = {};
function counted<P extends object>(name: string, real: ComponentType<P>) {
  function Counted(props: P) {
    renders[name] = (renders[name] ?? 0) + 1;
    return createElement(real, props);
  }
  Counted.displayName = `Counted(${name})`;
  return Counted;
}
// A turn of the transcript, and the header's pickers (the task's model and environment).
vi.mock("./ActivityGroup", async (load) => {
  const real = await load<typeof import("./ActivityGroup")>();
  return { ...real, ActivityGroup: counted("turn", real.ActivityGroup) };
});
vi.mock("../ui/MenuButton", async (load) => {
  const real = await load<typeof import("../ui/MenuButton")>();
  return { ...real, MenuButton: counted("picker", real.MenuButton) };
});
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

function showing(transcript: Transcript) {
  vi.mocked(useTranscript).mockReturnValue({ transcript, connection: "live" });
}
function page() {
  const cache = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  cache.setQueryData(["templates"], templates);
  shellData(cache);
  cache.setQueryData(["models"], {
    items: [
      {
        name: "local",
        model: "Local model",
        protocol: "openai_chat",
        context_window: null,
        runs_on: { where: "this_computer", provider: null },
        vision: false,
        reasoning: null,
      },
    ],
  });
  const view = () => (
    <QueryClientProvider client={cache}>
      <LiveConversation conversation={conversation} />
    </QueryClientProvider>
  );
  const { rerender } = render(view());
  return { again: () => rerender(view()) };
}
const clear = () => {
  for (const name of Object.keys(renders)) renders[name] = 0;
};
/** The page settled: its turns rendered and its model picker named from its loaded list (no waiting on a clock). */
async function settled() {
  await waitFor(() => expect(renders.turn ?? 0).toBeGreaterThan(0));
  await screen.findByRole("button", { name: /^Model: (?!Loading)/ });
}

let finished: Transcript;
beforeEach(() => {
  finished = {
    ...transcriptFrames.reduce(apply, emptyTranscript()),
    sandbox: "ready",
  };
  showing(finished);
});

// The keystroke profile of dc017a1f: every keystroke re-rendered the whole transcript (95% of its time, 21-51 ms for
// 100 turns in a development build) and the header's pickers, because the message box's text lives on the task's page.
describe("WEB-U-151 typing re-renders only the message box", () => {
  it("re-renders neither the transcript's turns nor the header's pickers", async () => {
    page();
    const input = await screen.findByRole("textbox", { name: "Message" });
    await settled();
    for (const text of ["h", "he", "hel"]) {
      clear();
      fireEvent.change(input, { target: { value: text } });
      await act(() => Promise.resolve());
      expect(input).toHaveValue(text);
      expect(renders.turn ?? 0).toBe(0);
      expect(renders.picker ?? 0).toBe(0);
    }
  });

  it("still re-renders the transcript when it changes", async () => {
    const { again } = page();
    await screen.findByRole("textbox", { name: "Message" });
    await settled();
    clear();
    showing({ ...finished, title: "Renamed" });
    again();
    await act(() => Promise.resolve());
    expect(renders.turn).toBeGreaterThan(0);
  });
});

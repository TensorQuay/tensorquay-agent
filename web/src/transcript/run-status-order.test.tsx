import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { Conversation } from "../protocol/generated/api";
import type { ServerFrame } from "../protocol/generated/protocol";
import { conversationId, transcriptFrames } from "../test/frames";
import { templates } from "../test/templates";
import { shellData } from "../test/workspaces";

import { LiveConversation } from "./LiveConversation";
import { apply, emptyTranscript } from "./reducer";
import { useTranscript } from "./use-transcript";

vi.mock("./use-transcript", () => ({ useTranscript: vi.fn() }));
// These tests count the page's requests for a message; the held list and the session are read elsewhere (WEB-U-079).
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
  team: null,
};
function page() {
  const cache = new QueryClient();
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
  return render(<LiveConversation conversation={conversation} />, {
    wrapper: ({ children }) => (
      <QueryClientProvider client={cache}>{children}</QueryClientProvider>
    ),
  });
}
beforeEach(() => {
  vi.mocked(useTranscript).mockReturnValue({
    transcript: { ...emptyTranscript(), sandbox: "ready" },
    connection: "live",
  });
});
afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

const accepted = () =>
  Response.json(
    {
      accepted: true,
      conversation_id: conversationId,
      message_id: "1",
      held: false,
    },
    { status: 202 },
  );
/** The run as the stream gives it, ending as the fake front's ends, or failed. */
function streamed(failed = false) {
  const frames = transcriptFrames.map((value): ServerFrame =>
    failed && value.type === "event" && value.event.type === "run.completed"
      ? {
          ...value,
          event: {
            type: "run.failed" as const,
            run_id: value.event.run_id,
            code: "internal",
            reason: "diag.internal",
          },
        }
      : value,
  );
  return {
    transcript: frames.reduce(apply, emptyTranscript()),
    connection: "live" as const,
  };
}
/** Sends the fixture's prompt; the message's answer waits until `answer` is called, each other request answers. */
function send() {
  let answer: (() => void) | undefined;
  vi.stubGlobal(
    "fetch",
    vi.fn((input: RequestInfo | URL) => {
      const url = input instanceof Request ? input.url : input.toString();
      if (!url.endsWith("/messages"))
        return Promise.resolve(Response.json({ live: false, files: [] }));
      return new Promise<Response>((resolve) => {
        answer = () => resolve(accepted());
      });
    }),
  );
  const view = page();
  fireEvent.change(screen.getByRole("textbox", { name: "Message" }), {
    target: { value: "Summarize the workspace." },
  });
  fireEvent.click(screen.getByRole("button", { name: "Send" }));
  return {
    answer: async () => {
      await waitFor(() => expect(answer).toBeDefined());
      await act(async () => {
        answer?.();
        await Promise.resolve();
      });
    },
    stream: (failed = false) => {
      vi.mocked(useTranscript).mockReturnValue(streamed(failed));
      view.rerender(<LiveConversation conversation={conversation} />);
    },
  };
}
const status = () => screen.getByRole("group", { name: "Run status" });

// WEB-U-129 (the c13b integration): the header's run status reached its end whatever the order the stream and the
// send's answer arrive in; a slow browser only delays it. It never stays on "Sending…" or "Working…".
describe("WEB-U-129 the run status takes the run's end in either order", () => {
  it("the whole run streams in before the send is answered", async () => {
    const sent = send();
    sent.stream();
    await sent.answer();
    await waitFor(() => expect(status()).toHaveTextContent(/^Done/));
  });

  it("the send is answered first, then the run streams in", async () => {
    const sent = send();
    await sent.answer();
    expect(status()).toHaveTextContent("Sending…");
    sent.stream();
    await waitFor(() => expect(status()).toHaveTextContent(/^Done/));
  });

  it("a run that fails before the send is answered says so", async () => {
    const sent = send();
    sent.stream(true);
    await sent.answer();
    await waitFor(() =>
      expect(status()).toHaveTextContent(
        "Failed: Something went wrong inside the agent",
      ),
    );
  });
});

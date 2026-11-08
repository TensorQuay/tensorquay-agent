import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { api } from "../api/client";
import type { Conversation } from "../protocol/generated/api";
import { conversationId, frame, ready, runId } from "../test/frames";
import { openTaskChoices } from "../test/start-choices";
import { templates } from "../test/templates";
import { shellData } from "../test/workspaces";

import { LiveConversation } from "./LiveConversation";
import { apply, emptyTranscript } from "./reducer";
import type { Transcript } from "./types";
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
const fetch = vi.fn<typeof globalThis.fetch>();
beforeEach(() => {
  fetch.mockReset().mockImplementation((url) =>
    Promise.resolve(
      Response.json(
        // A message's acceptance names it (GW-I-129); a cancel's doesn't.
        (url instanceof Request ? url.url : url.toString()).endsWith(
          "/messages",
        )
          ? {
              accepted: true,
              conversation_id: conversationId,
              message_id: "1",
              held: false,
            }
          : { accepted: true, conversation_id: conversationId },
        { status: 202 },
      ),
    ),
  );
  vi.stubGlobal("fetch", fetch);
  // The task's stored key uses, read at load (WEB-U-193): not one of the actions these tests count.
  vi.spyOn(api, "keyUses").mockResolvedValue({ items: [], next: null });
  // The workspace's top folder, which the empty page reads (WEB-U-152): not one of the actions these tests count.
  vi.spyOn(api, "files").mockResolvedValue({
    path: "",
    items: [],
    next_after: null,
  });
});
afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

function page(initialMessage: string | null = null) {
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
  let transcript: Transcript = { ...emptyTranscript(), sandbox: "ready" };
  vi.mocked(useTranscript).mockImplementation(() => ({
    transcript,
    connection: "live",
  }));
  const element = () => (
    <QueryClientProvider client={cache}>
      <LiveConversation
        conversation={conversation}
        initialMessage={initialMessage}
      />
    </QueryClientProvider>
  );
  const view = render(element());
  return (next: Transcript) => {
    transcript = next;
    view.rerender(element());
  };
}

describe("WEB-U-001 accepted action failure recovery", () => {
  it("does not invite another message when a reload finds queued work", () => {
    const update = page();
    update(
      apply(emptyTranscript(), {
        ...ready,
        sandbox: { ...ready.sandbox, state: "queued" },
        queue: { ahead: 0 },
      }),
    );
    expect(screen.queryByText(/Send a message to begin/)).toBeNull();
    expect(screen.getByRole("textbox", { name: "Message" })).toBeDisabled();
    expect(screen.getByLabelText("Run status")).toHaveTextContent(
      "Waiting for a free slot · next",
    );
  });

  // WEB-U-121 (F26): waiting only for the person's own idle sandbox to make room is starting, never a queue.
  it("WEB-U-121 says Starting… while the person's own idle sandbox makes room", async () => {
    const update = page("Start it");
    update(
      apply(emptyTranscript(), {
        ...ready,
        sandbox: { ...ready.sandbox, state: "queued" },
        queue: { ahead: 0, reason: "own_sandbox" },
      }),
    );
    expect(screen.getByLabelText("Run status")).toHaveTextContent("Starting…");
    expect(screen.queryByText(/free slot/)).toBeNull();
    await openTaskChoices();
    expect(
      screen.getByRole("button", { name: /^Model:/ }),
    ).toHaveAccessibleDescription(
      "Wait for the environment to start before changing the model.",
    );
  });

  it("shows accepted queue and wake progress before a user event is written", async () => {
    const update = page("Wait for a slot");
    const queued = apply(emptyTranscript(), {
      ...ready,
      sandbox: { ...ready.sandbox, state: "queued" },
      queue: { ahead: 2 },
    });
    update(queued);
    expect(screen.getByLabelText("Run status")).toHaveTextContent(
      "Waiting for a free slot · 2 ahead",
    );
    expect(screen.queryByRole("button", { name: "Sending" })).toBeNull();
    // review-050: the composer carries one queued message, the helper line.
    expect(screen.getByRole("textbox", { name: "Message" })).toHaveAttribute(
      "placeholder",
      "Reply…",
    );
    // WEB-U-161 (a screen review before 17.0): the composer says what to do, not the state again.
    expect(screen.getByText("You can reply once it starts.")).toBeVisible();
    // The model and environment wait behind the header's Change.
    await openTaskChoices();
    const model = screen.getByRole("button", { name: /^Model:/ });
    expect(model).toHaveAttribute("aria-disabled", "true");
    expect(model).toHaveAccessibleDescription(
      "Wait for a free slot before changing the model.",
    );
    fireEvent.click(model);
    expect(screen.queryByRole("menu")).toBeNull();
    expect(
      screen.getByRole("button", { name: /^Environment:/ }),
    ).toHaveAttribute("aria-disabled", "true");
    // WEB-U-161: the waiting is said once, in the header; the message itself carries no "Queued" line.
    expect(
      screen.getByRole("article", { name: "Your message" }),
    ).not.toHaveTextContent("Queued");
    expect(screen.getAllByText(/Waiting for a free slot/)).toHaveLength(1);
    expect(screen.queryByText(/queued/i)).toBeNull();
    const creating = apply(queued, {
      ...ready,
      sandbox: { ...ready.sandbox, state: "creating" },
    });
    update(creating);
    expect(screen.getByLabelText("Run status")).toHaveTextContent(
      /^Starting…$/,
    );
    expect(screen.queryAllByText("Sending…")).toHaveLength(0);
    const running = [
      frame(1, {
        type: "message.user",
        text: "Wait for a slot",
        attachments: [],
      }),
      frame(2, { type: "run.started", run_id: runId }),
      { ...ready, sandbox: { ...ready.sandbox, state: "busy" as const } },
    ].reduce(apply, creating);
    update(running);
    expect(screen.getByLabelText("Run status")).toHaveTextContent(/^Working…/);
    expect(
      screen.getAllByRole("article", { name: "Your message" }),
    ).toHaveLength(1);
    expect(screen.queryByText("Queued")).toBeNull();
  });

  it("keeps the same retry key after uncertain network delivery", async () => {
    fetch.mockRejectedValueOnce(new Error("Lost response"));
    page();
    fireEvent.change(screen.getByRole("textbox", { name: "Message" }), {
      target: { value: "Keep this message" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Send" }));
    await within(
      screen.getByRole("article", { name: "Your message" }),
    ).findByText(/Not sent\. The connection was lost/);
    fireEvent.click(screen.getByRole("button", { name: "Retry message" }));
    await waitFor(() => expect(fetch).toHaveBeenCalledTimes(2));
    await waitFor(() =>
      expect(screen.getByRole("textbox", { name: "Message" })).toHaveValue(""),
    );
    expect(
      new Headers(fetch.mock.calls[0]?.[1]?.headers).get("idempotency-key"),
    ).toBe(
      new Headers(fetch.mock.calls[1]?.[1]?.headers).get("idempotency-key"),
    );
  });
  it("retains a failure that arrives before the accepted HTTP response", async () => {
    let accept: ((response: Response) => void) | undefined;
    fetch.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          accept = resolve;
        }),
    );
    const update = page();
    fireEvent.change(screen.getByRole("textbox", { name: "Message" }), {
      target: { value: "Keep the failed prompt" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Send" }));
    await waitFor(() => expect(fetch).toHaveBeenCalledOnce());
    update(
      apply(emptyTranscript(), {
        type: "error",
        code: "sandbox_failed",
        message: "Private diagnostic",
        retryable: true,
      }),
    );
    await act(() => {
      accept?.(
        Response.json(
          {
            accepted: true,
            conversation_id: conversationId,
            message_id: "1",
            held: false,
          },
          { status: 202 },
        ),
      );
      return Promise.resolve();
    });
    // E1's failed start, the page the owner wanted seen (a pass over the pages before 17.0): what happened and the way
    // on.
    expect(
      within(screen.getByRole("article", { name: "Your message" })).getByText(
        "Not sent. The task's environment couldn't be started. Send the message again; if it fails again, choose another environment or ask an administrator.",
      ),
    ).toBeVisible();
    expect(screen.queryByRole("button", { name: "Sending" })).toBeNull();
    expect(screen.getByRole("textbox", { name: "Message" })).toHaveValue(
      "Keep the failed prompt",
    );
  });
  it.each([
    [
      "sandbox_failed",
      "The task's environment couldn't be started. Send the message again; if it fails again, choose another environment or ask an administrator.",
    ],
    [
      "runtime_start_failed",
      "The agent stopped while starting. Send a message to try again; if it happens again, ask an administrator to rebuild the environment.",
    ],
    [
      "action_failed",
      "An action couldn't be completed. Send the message again in a moment.",
    ],
  ] as const)(
    "leaves Sending on %s, restores the draft and gives a new retry key",
    async (code, copy) => {
      const update = page();
      fireEvent.change(screen.getByRole("textbox", { name: "Message" }), {
        target: { value: "Keep this message" },
      });
      fireEvent.click(screen.getByRole("button", { name: "Send" }));
      await waitFor(() => expect(fetch).toHaveBeenCalledOnce());
      await waitFor(() =>
        expect(screen.getByRole("button", { name: "Sending" })).toBeDisabled(),
      );
      const failure = apply(
        { ...emptyTranscript(), sandbox: "failed" },
        {
          type: "error",
          code,
          message: "Internal sandbox startup detail.",
          retryable: true,
        },
      );
      update(failure);
      const notices = screen.getAllByText(`Not sent. ${copy}`);
      expect(notices).toHaveLength(2);
      for (const notice of notices) {
        expect(notice).toBeVisible();
        expect(notice.closest(".type-label")).toBeNull();
      }
      expect(screen.getByLabelText("Run status")).toHaveTextContent(
        `Not sent. ${copy}`,
      );
      expect(screen.queryByText(/The environment could not start/)).toBeNull();
      expect(screen.queryByText(/Internal sandbox/)).toBeNull();
      expect(screen.queryByRole("button", { name: "Sending" })).toBeNull();
      expect(screen.getByRole("textbox", { name: "Message" })).toHaveValue(
        "Keep this message",
      );
      fireEvent.click(screen.getByRole("button", { name: "Retry message" }));
      await waitFor(() => expect(fetch).toHaveBeenCalledTimes(2));
      await waitFor(() =>
        expect(screen.getByRole("textbox", { name: "Message" })).toHaveValue(
          "",
        ),
      );
      const keys = fetch.mock.calls.map(([, options]) =>
        new Headers(options?.headers).get("idempotency-key"),
      );
      expect(keys[0]).toMatch(/^[a-f0-9-]{36}$/);
      expect(keys[1]).not.toBe(keys[0]);
      update(
        apply(
          failure,
          frame(1, {
            type: "message.user",
            text: "Keep this message",
            attachments: [],
          }),
        ),
      );
      expect(screen.queryByText(/Not sent:/)).toBeNull();
      expect(
        screen.getAllByRole("article", { name: "Your message" }),
      ).toHaveLength(1);
    },
  );

  it("recovers the first prompt after navigation even when no event was written", () => {
    const update = page("A first message");
    expect(
      screen.getByRole("article", { name: "Your message" }),
    ).toHaveTextContent("Sending…");
    update(
      apply(emptyTranscript(), {
        type: "error",
        code: "action_failed",
        message: "The environment stopped before it was ready.",
        retryable: true,
      }),
    );
    expect(screen.queryAllByText("Sending…")).toHaveLength(0);
    expect(screen.getByRole("button", { name: "Retry message" })).toBeEnabled();
    expect(screen.getByRole("textbox", { name: "Message" })).toHaveValue(
      "A first message",
    );
  });
});

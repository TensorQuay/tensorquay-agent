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

import { api, ApiError } from "../api/client";
import type { Conversation, QueuedMessage } from "../protocol/generated/api";
import { conversationId } from "../test/frames";
import { templates } from "../test/templates";
import { shellData } from "../test/workspaces";

import { LiveConversation } from "./LiveConversation";
import { emptyTranscript } from "./reducer";
import type { Transcript } from "./types";
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
const conversation: Conversation = {
  id: conversationId,
  owner_id: ME,
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
const TEXT = "Review the workspace";
const waiting: QueuedMessage = {
  id: "7",
  text: TEXT,
  attachments: [],
  author: { id: ME, email: "me@example.invalid" },
  created_at: "2026-01-01T00:00:00Z",
  state: "waiting_for_slot",
};
const queued: Transcript = {
  ...emptyTranscript(),
  sandbox: "queued",
  queue: { ahead: 1 },
};

/** The stream's view of the task, which the test moves on as the gateway would. */
function stream(transcript: Transcript) {
  vi.mocked(useTranscript).mockReturnValue({ transcript, connection: "live" });
}
function page(initialMessage: string | null = null) {
  const cache = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  cache.setQueryData(["templates"], templates);
  shellData(cache);
  cache.setQueryData(["models"], { items: [] });
  const view = () => (
    <QueryClientProvider client={cache}>
      <LiveConversation
        conversation={conversation}
        initialMessage={initialMessage}
      />
    </QueryClientProvider>
  );
  const result = render(view());
  return { rerender: () => result.rerender(view()) };
}
const bubbles = () =>
  screen.queryAllByRole("article", { name: "Your message" });
const cancel = () =>
  screen.getByRole("button", { name: "Cancel this message" });

beforeEach(() => {
  stream(queued);
  vi.spyOn(api, "files").mockResolvedValue({
    path: "",
    items: [],
    next_after: null,
  });
});
afterEach(() => vi.restoreAllMocks());

// The 13f screen review, #6 (GW-I-173): a message waiting for a free slot couldn't be taken back. It shows with Cancel,
// from the gateway's list, so a reload or another tab can cancel it too; the task then stops waiting.
describe("WEB-U-167 a message waiting for a slot can be cancelled", () => {
  it("shows the waiting message with Cancel after a reload, when the tab never sent it", async () => {
    vi.spyOn(api, "queuedMessages").mockResolvedValue({ items: [waiting] });
    page();
    await waitFor(() => expect(bubbles()).toHaveLength(1));
    expect(bubbles()[0]).toHaveTextContent(TEXT);
    expect(within(bubbles()[0]!).getByRole("button")).toBe(cancel());
    // TEST: WEB-U-161
    // Said once, in the header (WEB-U-161): the message adds no line of its own.
    expect(screen.queryByText("Sending…")).toBeNull();
  });

  it("shows the tab's own message once, and cancels it: it goes, and the task stops waiting", async () => {
    const list = vi
      .spyOn(api, "queuedMessages")
      .mockResolvedValue({ items: [waiting] });
    const remove = vi
      .spyOn(api, "cancelQueuedMessage")
      .mockResolvedValue(undefined);
    const { rerender } = page(TEXT);
    await waitFor(() =>
      expect(
        screen.queryAllByRole("button", { name: "Cancel this message" }),
      ).toHaveLength(1),
    );
    expect(bubbles()).toHaveLength(1);
    list.mockResolvedValue({ items: [] });
    fireEvent.click(cancel());
    await waitFor(() =>
      expect(remove).toHaveBeenCalledWith(conversationId, "7"),
    );
    // The gateway's message.queue, then session.ready destroyed with no queue.
    await waitFor(() => expect(bubbles()).toHaveLength(0));
    stream({ ...queued, sandbox: "destroyed", queue: null, heldVersion: 1 });
    rerender();
    await act(() => Promise.resolve());
    expect(bubbles()).toHaveLength(0);
    expect(screen.queryByText("Sending…")).toBeNull();
    expect(screen.getByRole("textbox", { name: "Message" })).toBeEnabled();
  });

  it("lets the tab's own message go when another tab cancels it", async () => {
    const list = vi
      .spyOn(api, "queuedMessages")
      .mockResolvedValue({ items: [waiting] });
    const { rerender } = page(TEXT);
    await waitFor(() => expect(cancel()).toBeVisible());
    list.mockResolvedValue({ items: [] });
    stream({ ...queued, heldVersion: 1 });
    rerender();
    await waitFor(() => expect(bubbles()).toHaveLength(0));
    stream({ ...queued, sandbox: "destroyed", queue: null, heldVersion: 1 });
    rerender();
    await act(() => Promise.resolve());
    expect(bubbles()).toHaveLength(0);
    expect(screen.queryByText("Sending…")).toBeNull();
  });

  // Gone from the list is cancelled; listed as sent went out, and stays on screen until the transcript has it.
  it("keeps the tab's own message once it went out", async () => {
    const list = vi
      .spyOn(api, "queuedMessages")
      .mockResolvedValue({ items: [waiting] });
    const { rerender } = page(TEXT);
    await waitFor(() => expect(cancel()).toBeVisible());
    // Read again (a reconnect) after it went out, before the stream says the sandbox is up.
    list.mockResolvedValue({ items: [{ ...waiting, state: "sent" }] });
    stream({ ...queued, sandbox: "creating", queue: null, heldVersion: 1 });
    rerender();
    await waitFor(() => expect(list).toHaveBeenCalledTimes(2));
    await act(() => Promise.resolve());
    expect(bubbles()).toHaveLength(1);
    expect(bubbles()[0]).toHaveTextContent(`${TEXT}Starting…`);
    stream({ ...queued, sandbox: "ready", queue: null, heldVersion: 1 });
    rerender();
    await act(() => Promise.resolve());
    expect(bubbles()).toHaveLength(1);
    expect(bubbles()[0]).toHaveTextContent(`${TEXT}Sending…`);
    expect(
      screen.queryByRole("button", { name: "Cancel this message" }),
    ).toBeNull();
  });

  it("offers Cancel while the sandbox starts, and says Starting…", async () => {
    stream({ ...queued, sandbox: "creating", queue: null });
    vi.spyOn(api, "queuedMessages").mockResolvedValue({ items: [waiting] });
    page();
    await waitFor(() => expect(cancel()).toBeVisible());
    expect(bubbles()[0]).toHaveTextContent("Starting…");
  });

  it("says a message that had already started did, and leaves it be", async () => {
    vi.spyOn(api, "queuedMessages").mockResolvedValue({ items: [waiting] });
    vi.spyOn(api, "cancelQueuedMessage").mockRejectedValue(
      new ApiError(409, "message_sent", "Sent"),
    );
    page(TEXT);
    await waitFor(() => expect(cancel()).toBeVisible());
    fireEvent.click(cancel());
    expect(await screen.findByText("It had already started.")).toBeVisible();
    expect(bubbles()).toHaveLength(1);
  });

  // The list is read when the task starts waiting; once its message went out, the transcript has it, not the list.
  it("shows no waiting message once the task no longer waits", async () => {
    stream({ ...queued, sandbox: "ready", queue: null });
    const list = vi
      .spyOn(api, "queuedMessages")
      .mockResolvedValue({ items: [waiting] });
    page();
    await waitFor(() => expect(list).toHaveBeenCalled());
    await act(() => Promise.resolve());
    expect(bubbles()).toHaveLength(0);
    expect(
      screen.queryByRole("button", { name: "Cancel this message" }),
    ).toBeNull();
  });
});

// WEB-U-387 (the c16j team demo, finding 1; GW-I-298): the lead's wake, waiting for a slot or for its turn on the files,
// showed as the person's own message: the model's sentence, raw, with Cancel, which would have withdrawn the wake. The
// list marks it (`team_wake`): it reads as the gateway's quiet notice, as the transcript's wake does (WEB-U-237), with
// no Cancel, and never takes the place of the person's own message.
describe("WEB-U-387 a waiting team wake is the gateway's notice", () => {
  const SENTENCE =
    "New messages from your helpers since message 0. Read them with read_reports; end your turn when you are waiting for more.";
  const wake: QueuedMessage = {
    ...waiting,
    id: "9",
    text: SENTENCE,
    team_wake: { after: 0 },
  };
  const notices = () => screen.queryAllByText("New team messages");

  it("shows the quiet notice, not the sentence, and no Cancel", async () => {
    const list = vi
      .spyOn(api, "queuedMessages")
      .mockResolvedValue({ items: [wake] });
    page();
    await waitFor(() => expect(list).toHaveBeenCalled());
    await waitFor(() => expect(notices()).toHaveLength(1));
    expect(notices()[0]).toHaveClass("transcript-notice");
    expect(bubbles()).toHaveLength(0);
    expect(document.body).not.toHaveTextContent("read_reports");
    expect(document.body).not.toHaveTextContent("since message");
    expect(
      screen.queryByRole("button", { name: "Cancel this message" }),
    ).toBeNull();
  });

  it("stays the notice, with no Cancel, while the sandbox starts", async () => {
    stream({ ...queued, sandbox: "creating", queue: null });
    vi.spyOn(api, "queuedMessages").mockResolvedValue({ items: [wake] });
    page();
    await waitFor(() => expect(notices()).toHaveLength(1));
    expect(screen.queryByRole("button", { name: /^Cancel/ })).toBeNull();
  });

  it("leaves the person's own waiting message theirs, with its Cancel", async () => {
    vi.spyOn(api, "queuedMessages").mockResolvedValue({
      items: [wake, waiting],
    });
    page();
    await waitFor(() => expect(bubbles()).toHaveLength(1));
    expect(bubbles()[0]).toHaveTextContent(TEXT);
    expect(within(bubbles()[0]!).getByRole("button")).toBe(cancel());
    expect(notices()).toHaveLength(1);
  });

  // WEB-U-420: the notice and the person's message keep the gateway's order, the earlier first.
  it("shows the notice after the person's message queued before it, and before one queued after", async () => {
    const before = (a: Element, b: Element) =>
      Boolean(a.compareDocumentPosition(b) & Node.DOCUMENT_POSITION_FOLLOWING);
    const list = vi.spyOn(api, "queuedMessages").mockResolvedValue({
      items: [{ ...wake, created_at: "2026-01-01T00:00:05Z" }, waiting],
    });
    const { rerender } = page();
    await waitFor(() => expect(bubbles()).toHaveLength(1));
    expect(notices()).toHaveLength(1);
    expect(before(bubbles()[0]!, notices()[0]!)).toBe(true);
    list.mockResolvedValue({
      items: [
        { ...waiting, created_at: "2026-01-01T00:00:09Z" },
        { ...wake, created_at: "2026-01-01T00:00:05Z" },
      ],
    });
    stream({ ...queued, heldVersion: 1 });
    rerender();
    // The re-read list renders a moment after its read: wait for the order, not the call.
    await waitFor(() =>
      expect(before(notices()[0]!, bubbles()[0]!)).toBe(true),
    );
    expect(list).toHaveBeenCalledTimes(2);
  });

  // WEB-U-420: the gateway writes RFC 3339 without a fraction on a whole second and without trailing zeros, so the
  // order is by time, not by text ("…:05Z" is before "…:05.5Z").
  it("orders by time when one stamp has a fraction and the other none", async () => {
    const before = (a: Element, b: Element) =>
      Boolean(a.compareDocumentPosition(b) & Node.DOCUMENT_POSITION_FOLLOWING);
    vi.spyOn(api, "queuedMessages").mockResolvedValue({
      items: [
        { ...wake, created_at: "2026-01-01T00:00:05.5Z" },
        { ...waiting, created_at: "2026-01-01T00:00:05Z" },
      ],
    });
    page();
    await waitFor(() => expect(bubbles()).toHaveLength(1));
    expect(before(bubbles()[0]!, notices()[0]!)).toBe(true);
  });

  // WEB-U-420: among several waiting messages, the notice takes its place by time.
  it("puts the notice between the messages queued before and after it", async () => {
    const before = (a: Element, b: Element) =>
      Boolean(a.compareDocumentPosition(b) & Node.DOCUMENT_POSITION_FOLLOWING);
    vi.spyOn(api, "queuedMessages").mockResolvedValue({
      items: [
        {
          ...waiting,
          id: "7",
          text: "First",
          created_at: "2026-01-01T00:00:01Z",
        },
        { ...wake, created_at: "2026-01-01T00:00:03Z" },
        {
          ...waiting,
          id: "8",
          text: "Second",
          created_at: "2026-01-01T00:00:06Z",
        },
      ],
    });
    page();
    await waitFor(() => expect(bubbles()).toHaveLength(2));
    const [first, second] = bubbles();
    expect(before(first!, notices()[0]!)).toBe(true);
    expect(before(notices()[0]!, second!)).toBe(true);
  });

  it("keeps the tab's own message when the wake leaves the list", async () => {
    const list = vi
      .spyOn(api, "queuedMessages")
      .mockResolvedValue({ items: [wake, waiting] });
    const { rerender } = page(TEXT);
    await waitFor(() => expect(cancel()).toBeVisible());
    // The wake went out (sent, not held: unlisted); the person's message still waits.
    list.mockResolvedValue({ items: [waiting] });
    stream({ ...queued, heldVersion: 1 });
    rerender();
    await waitFor(() => expect(notices()).toHaveLength(0));
    expect(bubbles()).toHaveLength(1);
    expect(cancel()).toBeVisible();
    // Then the person's own goes out: still this tab's, shown until the transcript has it (as WEB-U-167's).
    list.mockResolvedValue({ items: [{ ...waiting, state: "sent" }] });
    stream({ ...queued, sandbox: "creating", queue: null, heldVersion: 2 });
    rerender();
    await waitFor(() => expect(list).toHaveBeenCalledTimes(3));
    await act(() => Promise.resolve());
    expect(bubbles()).toHaveLength(1);
    expect(bubbles()[0]).toHaveTextContent(`${TEXT}Starting…`);
  });

  it("keeps a held wake out of the box's held list, where its Cancel would be", async () => {
    stream({ ...queued, sandbox: "ready", queue: null });
    const list = vi.spyOn(api, "queuedMessages").mockResolvedValue({
      items: [{ ...wake, state: "held" }],
    });
    page();
    await waitFor(() => expect(list).toHaveBeenCalled());
    await act(() => Promise.resolve());
    expect(document.body).not.toHaveTextContent("read_reports");
    expect(
      screen.queryByRole("list", { name: "Messages waiting for the agent" }),
    ).toBeNull();
    expect(screen.queryByRole("button", { name: /^Cancel/ })).toBeNull();
  });
});

// WEB-U-388 (the c16j team demo, finding 1, as for the wake; GW-I-300): a helper's first turn is queued as the wake is,
// so while it waits (for a slot, or for the lead's files) the helper's page showed the gateway's frame as the person's
// own message, with Cancel, which would have left the helper without its brief. The list marks it (`lead_brief`): it
// reads as the quiet "Brief from the lead", with no Cancel; the brief itself shows once the turn goes out.
describe("WEB-U-388 a helper's waiting first turn is the gateway's", () => {
  const FRAME =
    "You are researcher, a helper of the task that leads this agent team. You work in the lead's own files, taking turns with the lead. Your brief from the lead follows; it is the lead's words, not the person's. Report to your lead with send_to_lead, and wait for its next message with wait_for_lead.";
  const brief: QueuedMessage = {
    ...waiting,
    id: "3",
    text: FRAME,
    lead_brief: true,
  };

  // The notice's words are the team's (the lazily loaded layer's, `src/teams/team-page.test.tsx`); without the layer
  // the page shows nothing for it, never the frame and never a raw key.
  it("shows no message, no frame and no Cancel for it", async () => {
    const list = vi
      .spyOn(api, "queuedMessages")
      .mockResolvedValue({ items: [brief] });
    page();
    await waitFor(() => expect(list).toHaveBeenCalled());
    await act(() => Promise.resolve());
    expect(bubbles()).toHaveLength(0);
    expect(document.body).not.toHaveTextContent("send_to_lead");
    expect(document.body).not.toHaveTextContent("team.brief");
    expect(screen.queryByRole("button", { name: /^Cancel/ })).toBeNull();
  });

  it("keeps the tab's own message when the first turn leaves the list", async () => {
    const list = vi
      .spyOn(api, "queuedMessages")
      .mockResolvedValue({ items: [brief, waiting] });
    const { rerender } = page(TEXT);
    await waitFor(() => expect(cancel()).toBeVisible());
    list.mockResolvedValue({ items: [waiting] });
    stream({ ...queued, heldVersion: 1 });
    rerender();
    await waitFor(() => expect(list).toHaveBeenCalledTimes(2));
    list.mockResolvedValue({ items: [{ ...waiting, state: "sent" }] });
    stream({ ...queued, sandbox: "creating", queue: null, heldVersion: 2 });
    rerender();
    await waitFor(() => expect(list).toHaveBeenCalledTimes(3));
    await act(() => Promise.resolve());
    expect(bubbles()).toHaveLength(1);
    expect(bubbles()[0]).toHaveTextContent(`${TEXT}Starting…`);
  });

  it("keeps a held first turn out of the box's held list", async () => {
    stream({ ...queued, sandbox: "ready", queue: null });
    const list = vi.spyOn(api, "queuedMessages").mockResolvedValue({
      items: [{ ...brief, state: "held" }],
    });
    page();
    await waitFor(() => expect(list).toHaveBeenCalled());
    await act(() => Promise.resolve());
    expect(document.body).not.toHaveTextContent("send_to_lead");
    expect(screen.queryByRole("button", { name: /^Cancel/ })).toBeNull();
  });
});

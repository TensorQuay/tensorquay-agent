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
import type { Conversation, User } from "../protocol/generated/api";
import { parseFrame } from "../protocol/validate";
import { conversationId, frame, runId, transcriptFrames } from "../test/frames";
import { openTaskChoices } from "../test/start-choices";
import { templates } from "../test/templates";
import { LIMITS } from "../test/user";
import { shellData } from "../test/workspaces";

import { LiveConversation } from "./LiveConversation";
import { apply, emptyTranscript } from "./reducer";
import { useTranscript } from "./use-transcript";

vi.mock("./use-transcript", () => ({ useTranscript: vi.fn() }));
// These tests count the page's requests for a message; the held list and the session are read elsewhere (WEB-U-079),
// and the workspace's files are stubbed below.
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
const session = vi.hoisted(() => ({
  current: undefined as User | undefined,
}));
vi.mock("../auth/use-session", () => ({
  useSession: () => ({ data: session.current }),
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
// Each test's client, cleared and unmounted after it (WEB-U-195).
let cache: QueryClient | undefined;
function page() {
  const client = new QueryClient();
  cache = client;
  client.setQueryData(["templates"], templates);
  shellData(client);
  client.setQueryData(["models"], {
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
      <QueryClientProvider client={client}>{children}</QueryClientProvider>
    ),
  });
}
beforeEach(() => {
  vi.mocked(useTranscript).mockReturnValue({
    transcript: { ...emptyTranscript(), sandbox: "ready" },
    connection: "live",
  });
  // The task's stored key uses (WEB-U-193), none unless a test says: not a request for a message.
  vi.spyOn(api, "keyUses").mockResolvedValue({ items: [], next: null });
  // The workspace's top folder, which the empty page reads (WEB-U-152): not a request for a message.
  vi.spyOn(api, "files").mockResolvedValue({
    path: "",
    items: [],
    next_after: null,
  });
});
afterEach(() => {
  session.current = undefined;
  cache?.clear();
  cache?.unmount();
  cache = undefined;
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

/** Opens the task's model and environment panel from its header, then finds a picker in it by name. */
async function inHeader(name: string) {
  await openTaskChoices();
  return screen.findByRole("button", { name });
}

// The correction of step 2 B (WEB-U-113, which replaces WEB-U-096's composer row): the model and environment are
// two small pickers in the task header, right of the title and status; no ⚙ and no "Model and environment" panel.
describe("WEB-U-113 the task's choices sit in its header", () => {
  // WEB-U-113: the pickers are back in the header, right of the title and status.
  it("offers the model and environment in the header, right of the status, and none in the composer", async () => {
    page();
    const header = screen.getByRole("region", { name: "Task header" });
    expect(
      within(header).queryByRole("button", {
        name: "Change model or environment",
      }),
    ).toBeNull();
    const choices = within(header).getByRole("group", {
      name: "Model and environment",
    });
    expect(
      await within(choices).findByRole("button", {
        name: "Model: Local model",
      }),
    ).toBeVisible();
    expect(
      within(choices).getByRole("button", {
        name: "Environment: Workstation",
      }),
    ).toBeVisible();
    // After the title and the status, before the Files icon and the ⋯.
    const status = within(header).getByRole("group", { name: "Run status" });
    const title = within(header).getByRole("heading", { level: 1 });
    const follows = (a: Element, b: Element) =>
      Boolean(a.compareDocumentPosition(b) & Node.DOCUMENT_POSITION_FOLLOWING);
    expect(follows(title, choices)).toBe(true);
    expect(follows(status, choices)).toBe(true);
    expect(
      follows(choices, within(header).getByRole("button", { name: "Files" })),
    ).toBe(true);
    expect(
      follows(
        choices,
        within(header).getByRole("button", { name: /^More actions for / }),
      ),
    ).toBe(true);
    const composer = screen.getByRole("form", { name: "Message composer" });
    expect(
      within(composer).queryByRole("group", { name: "Model and environment" }),
    ).toBeNull();
    expect(
      within(composer).queryByRole("button", { name: /^Model:/ }),
    ).toBeNull();
  });
});

describe("WEB-U-001 inline action failures", () => {
  it("WEB-U-018 preserves a draft and prevents sending while an environment switch is pending", async () => {
    let finish: ((response: Response) => void) | undefined;
    const fetch = vi.fn().mockImplementation(
      () =>
        new Promise((resolve) => {
          finish = resolve;
        }),
    );
    vi.stubGlobal("fetch", fetch);
    page();
    const input = screen.getByRole("textbox", { name: "Message" });
    fireEvent.change(input, { target: { value: "Use the new environment" } });
    fireEvent.click(await inHeader("Environment: Workstation"));
    fireEvent.click(screen.getByRole("menuitemradio", { name: /^Minimal/ }));
    fireEvent.click(
      await screen.findByRole("button", { name: "Switch to Minimal" }),
    );
    expect(input).toBeDisabled();
    expect(input).toHaveValue("Use the new environment");
    expect(screen.getByRole("button", { name: "Send" })).toBeDisabled();
    await act(() => {
      finish?.(Response.json({ ...conversation, template: "minimal" }));
      return Promise.resolve();
    });
    await waitFor(() => expect(input).toBeEnabled());
    expect(input).toHaveValue("Use the new environment");
    expect(fetch).toHaveBeenCalledOnce();
  });
  it("keeps the draft and shows a key generation exception inline", async () => {
    vi.stubGlobal("crypto", {
      randomUUID: () => {
        throw new Error("Secure random source failed.");
      },
    });
    const fetch = vi.fn();
    vi.stubGlobal("fetch", fetch);
    page();
    const input = screen.getByRole("textbox", { name: "Message" });
    fireEvent.change(input, { target: { value: "Keep this draft" } });
    fireEvent.click(screen.getByRole("button", { name: "Send" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Secure random source failed.",
    );
    expect(input).toHaveValue("Keep this draft");
    expect(screen.getByRole("button", { name: "Send" })).toBeEnabled();
    expect(fetch).not.toHaveBeenCalled();
  });

  it("shows a cancel preparation failure inline and leaves Stop available", async () => {
    vi.mocked(useTranscript).mockReturnValue({
      transcript: {
        ...emptyTranscript(),
        sandbox: "ready",
        run: { id: runId, status: "running" },
      },
      connection: "live",
    });
    vi.spyOn(api, "cancel").mockImplementation(() => {
      throw new Error("Could not prepare Stop.");
    });
    page();
    fireEvent.click(screen.getByRole("button", { name: "Stop" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Could not prepare Stop.",
    );
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Stop" })).toBeEnabled(),
    );
  });
});

describe("WEB-U-015 composer resumes following", () => {
  it("keeps a follow-up after replay below the old answer and follows its pending layout", async () => {
    const replay = transcriptFrames.reduce(apply, emptyTranscript());
    vi.mocked(useTranscript).mockReturnValue({
      transcript: replay,
      connection: "live",
    });
    vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(200);
    vi.spyOn(HTMLElement.prototype, "scrollHeight", "get").mockImplementation(
      function (this: HTMLElement) {
        return 3000 + this.querySelectorAll(".user-message").length * 100;
      },
    );
    vi.stubGlobal(
      "fetch",
      vi
        .fn()
        .mockResolvedValue(
          Response.json(
            { accepted: true, conversation_id: conversationId },
            { status: 202 },
          ),
        ),
    );
    const view = page();
    const region = screen.getByRole("region", { name: "Transcript" });
    const input = screen.getByRole("textbox", { name: "Message" });
    fireEvent.change(input, { target: { value: "Continue after replay" } });
    fireEvent.keyDown(input, { key: "Enter" });
    await waitFor(() => expect(input).toHaveValue(""));
    expect(
      screen.getAllByRole("article", { name: "Your message" }).at(-1),
    ).toHaveTextContent("Continue after replay");
    expect(region.scrollTop).toBe(region.scrollHeight - region.clientHeight);
    fireEvent.scroll(region);
    const next = apply(
      replay,
      frame(20, {
        type: "message.user",
        text: "Continue after replay",
        attachments: [],
      }),
    );
    vi.mocked(useTranscript).mockReturnValue({
      transcript: next,
      connection: "live",
    });
    view.rerender(<LiveConversation conversation={conversation} />);
    expect(region.scrollTop).toBe(region.scrollHeight - region.clientHeight);
    expect(screen.queryByRole("button", { name: /Jump to latest/ })).toBeNull();
    expect(
      screen.getAllByRole("article", { name: "Your message" }),
    ).toHaveLength(2);
  });

  // WEB-U-274: Jump to latest goes once the view is at the end, so the focus of someone using it with the keyboard fell
  // to the page; the transcript, showing the latest, keeps it.
  it("leaves the focus on the transcript after Jump to latest", () => {
    vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(200);
    vi.spyOn(HTMLElement.prototype, "scrollHeight", "get").mockReturnValue(
      3000,
    );
    page();
    const region = screen.getByRole("region", { name: "Transcript" });
    region.scrollTop = 0;
    fireEvent.scroll(region);
    const jump = screen.getByRole("button", { name: /Jump to latest/ });
    jump.focus();
    fireEvent.click(jump);
    expect(screen.queryByRole("button", { name: /Jump to latest/ })).toBeNull();
    expect(region).toHaveFocus();
  });

  it("jumps from earlier content when Send is used", async () => {
    vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(200);
    vi.spyOn(HTMLElement.prototype, "scrollHeight", "get").mockReturnValue(
      3000,
    );
    const fetch = vi
      .fn()
      .mockResolvedValue(
        Response.json(
          { accepted: true, conversation_id: conversationId },
          { status: 202 },
        ),
      );
    vi.stubGlobal("fetch", fetch);
    page();
    const region = screen.getByRole("region", {
      name: "Transcript",
    });
    region.scrollTop = 0;
    fireEvent.scroll(region);
    expect(
      screen.getByRole("button", { name: /Jump to latest/ }),
    ).toBeVisible();
    const input = screen.getByRole("textbox", { name: "Message" });
    fireEvent.change(input, { target: { value: "Continue the review" } });
    fireEvent.click(screen.getByRole("button", { name: "Send" }));
    await waitFor(() => expect(input).toHaveValue(""));
    expect(fetch).toHaveBeenCalledOnce();
    expect(region.scrollTop).toBe(2800);
    expect(screen.queryByRole("button", { name: /Jump to latest/ })).toBeNull();
  });
});

// UI-GUIDELINES §7: the one-line bar has no divider until the transcript scrolls under it.
describe("WEB-U-007 bar divider", () => {
  it("marks the page scrolled only while the transcript is scrolled away from its top", () => {
    const view = page();
    const section = screen.getByRole("region", { name: "Task" });
    const scroller = view.container.querySelector(".transcript-scroll");
    if (!(scroller instanceof HTMLElement)) throw new Error("No transcript");
    expect(section).toHaveAttribute("data-scrolled", "false");
    // WEB-U-267: its icon buttons' tooltips are placed inside it, never under the Files panel beside it.
    expect(section).toHaveAttribute("data-tip-bounds");
    scroller.scrollTop = 40;
    fireEvent.scroll(scroller);
    expect(section).toHaveAttribute("data-scrolled", "true");
    scroller.scrollTop = 0;
    fireEvent.scroll(scroller);
    expect(section).toHaveAttribute("data-scrolled", "false");
  });
});

// A host key's name shows in a task's steps only to the run's sender, whose key it is (ADR-0037 §2); in a shared task
// that may not be the task's owner.
describe("WEB-U-148 a keyed call's key named to the run's sender only", () => {
  const person = (id: string): User => ({
    id,
    email: `${id}@example.invalid`,
    role: "member",
    internet: "none",
    limits: LIMITS,
    features: { agent_teams: false },
  });
  function keyed(name: string | null) {
    const state = [
      frame(1, { type: "run.started", run_id: runId }),
      frame(2, { type: "step.started", step: 1, attempt: 1 }),
      frame(3, {
        type: "tool.call",
        call_id: "call-1",
        name: "shell",
        arguments: { command: "curl https://api.github.com/user" },
      }),
    ].reduce(apply, emptyTranscript());
    const used = parseFrame(
      JSON.stringify({
        type: "host_key.used",
        run_id: runId,
        step: 1,
        sender_id: "5a4b3c2d-1e0f-4a9b-8c7d-6e5f4a3b2c1d",
        name,
        host: "api.github.com",
        port: 443,
      }),
    );
    vi.mocked(useTranscript).mockReturnValue({
      transcript: { ...apply(state, used), sandbox: "ready" },
      connection: "live",
    });
  }

  it("names the key to the person whose message started the run", async () => {
    session.current = person("5a4b3c2d-1e0f-4a9b-8c7d-6e5f4a3b2c1d");
    keyed("GITHUB_WORK");
    page();
    expect(
      await screen.findByText("Called api.github.com with GITHUB_WORK"),
    ).toBeInTheDocument();
  });

  it("names no key to the task's owner when someone else started the run", async () => {
    session.current = person(conversation.owner_id);
    keyed("GITHUB_WORK");
    page();
    expect(
      await screen.findByText("Called api.github.com with a key"),
    ).toBeInTheDocument();
    expect(screen.queryByText(/GITHUB_WORK/)).toBeNull();
  });

  it("names no key while the viewer isn't known", async () => {
    keyed(null);
    page();
    expect(
      await screen.findByText("Called api.github.com with a key"),
    ).toBeInTheDocument();
  });
});

describe("WEB-U-193 a page opened later shows the task's stored key uses", () => {
  const SENDER = "5a4b3c2d-1e0f-4a9b-8c7d-6e5f4a3b2c1d";
  const stored = {
    id: "0195d8c0-0000-7000-8000-0000000000aa",
    run_id: runId,
    sender_id: SENDER,
    name: "GITHUB_WORK",
    host: "api.github.com",
    port: 443,
    outcome: "foreign_sandbox" as const,
    count: 2,
    first_at: "2026-01-01T00:00:00.300Z",
    last_at: "2026-01-01T00:00:00.300Z",
  };

  it("says a stored refusal on the run's only step, read again after a dropped stream", async () => {
    session.current = {
      id: SENDER,
      email: "sender@example.invalid",
      role: "member",
      internet: "none",
      limits: LIMITS,
      features: { agent_teams: false },
    };
    const read = vi
      .spyOn(api, "keyUses")
      .mockResolvedValue({ items: [stored], next: null });
    // Coming back from a dropped stream, the task is read again too.
    vi.spyOn(api, "conversation").mockResolvedValue(conversation);
    vi.mocked(useTranscript).mockReturnValue({
      transcript: {
        ...[
          frame(1, { type: "run.started", run_id: runId }),
          frame(2, { type: "step.started", step: 1, attempt: 1 }),
          frame(3, { type: "run.completed", run_id: runId }),
        ].reduce(apply, emptyTranscript()),
        sandbox: "ready",
      },
      connection: "live",
    });
    const view = page();
    const words =
      "GITHUB_WORK wasn't sent to api.github.com: it came from another task.";
    expect(await screen.findByText(words)).toBeInTheDocument();
    expect(read).toHaveBeenCalledTimes(1);
    expect(read.mock.calls[0]?.slice(0, 2)).toEqual([conversationId, null]);
    vi.mocked(useTranscript).mockReturnValue({
      transcript: { ...emptyTranscript(), sandbox: "ready" },
      connection: "reconnecting",
    });
    view.rerender(<LiveConversation conversation={conversation} />);
    vi.mocked(useTranscript).mockReturnValue({
      transcript: { ...emptyTranscript(), sandbox: "ready" },
      connection: "live",
    });
    view.rerender(<LiveConversation conversation={conversation} />);
    await waitFor(() => expect(read).toHaveBeenCalledTimes(2));
  });
});

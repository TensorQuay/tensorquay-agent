import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { api } from "../api/client";
import type { Conversation } from "../protocol/generated/api";
import { frame, ready, runId } from "../test/frames";
import {
  helper,
  helperId,
  leadId,
  team,
  teamId,
  teamMessage,
} from "../test/teams";
import { templates } from "../test/templates";
import { shellData, workspace } from "../test/workspaces";
import { LiveConversation } from "../transcript/LiveConversation";

import { readTeam, readTeamMessages } from "./requests";

vi.mock("./requests", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./requests")>();
  return {
    readTeam: vi.fn(actual.readTeam),
    readTeamMessages: vi.fn(actual.readTeamMessages),
    answerTeam: vi.fn(actual.answerTeam),
    stopTeam: vi.fn(actual.stopTeam),
  };
});
vi.mock("../transcript/use-held", async (importOriginal) => ({
  gatewayTurn: (await importOriginal<typeof import("../transcript/use-held")>())
    .gatewayTurn,
  useHeldMessages: () => ({
    held: [],
    waiting: [],
    listed: null,
    dropped: [],
    wakeWaiting: false,
    briefWaiting: false,
    gatewayAt: 0,
    notice: null,
    remove: vi.fn(),
    refresh: vi.fn(),
  }),
}));
vi.mock("../auth/use-session", () => ({
  useSession: () => ({
    data: {
      id: "0195d8c0-0000-7000-8000-000000000009",
      email: "me@example.invalid",
      role: "member",
      internet: "none",
      limits: {
        upload_bytes: 1,
        uploads_bytes: 1,
        preview_html_bytes: 1,
        preview_pdf_bytes: 1,
        preview_sheet_bytes: 1,
      },
      features: { agent_teams: true },
    },
  }),
}));

/** The browser's event stream: opened by the page; each test sends what it replays. */
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

const record = (extra: Partial<Conversation>): Conversation => ({
  id: leadId,
  owner_id: "0195d8c0-0000-7000-8000-000000000009",
  title: "Lead a team of three helpers",
  model_route: "local",
  template: "workstation",
  pinned: false,
  title_source: "person",
  backend: "docker",
  state: "active",
  created_at: "2026-01-01T00:00:00Z",
  updated_at: "2026-01-01T00:00:00Z",
  last_activity_at: "2026-01-01T00:00:00Z",
  workspace_id: leadId,
  mcp_servers: [],
  connectors: [],
  run_status: "idle",
  team: null,
  ...extra,
});
// A helper of the lead's on its files: its own record, as its page gets it.
const helperRecord = (
  team: Partial<NonNullable<Conversation["team"]>>,
): Conversation =>
  record({
    id: helperId(3),
    title: "helper-c",
    workspace_id: leadId,
    team: {
      role: "member",
      id: teamId,
      state: "running",
      lead: { id: leadId, title: "Lead a team of three helpers" },
      name: "helper-c",
      helper_state: "queued",
      ...team,
    },
  });

async function show(conversation: Conversation, frames: unknown[]) {
  const cache = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  cache.setQueryData(["templates"], templates);
  shellData(cache, []);
  cache.setQueryData(["models"], { items: [] });
  cache.setQueryData(["conversation", conversation.id], conversation);
  render(<LiveConversation conversation={conversation} />, {
    wrapper: ({ children }) => (
      <QueryClientProvider client={cache}>{children}</QueryClientProvider>
    ),
  });
  const stream = Stream.opened[0];
  await act(async () => {
    stream?.open();
    for (const value of frames) stream?.send(value);
    await Promise.resolve();
  });
}

beforeEach(() => {
  Stream.opened = [];
  vi.stubGlobal("EventSource", Stream);
  vi.spyOn(api, "files").mockResolvedValue({
    path: "",
    items: [],
    next_after: null,
  });
  vi.spyOn(api, "keyUses").mockResolvedValue({ items: [], next: null });
  vi.mocked(readTeam).mockReset();
  vi.mocked(readTeamMessages).mockReset();
});
afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

// WEB-U-410 (c17 U5) on the page: a helper waiting for its turn on the lead's files says so where a phone shows it.
describe("WEB-U-410 a waiting helper's page says what it waits for", () => {
  it("says the turn on the lead's files in the composer", async () => {
    await show(helperRecord({}), [
      {
        ...ready,
        conversation_id: helperId(3),
        sandbox: { backend: "docker", state: "queued" },
        queue: { ahead: 0, reason: "workspace" },
      },
    ]);
    await waitFor(() =>
      expect(
        screen.getByText(
          "Its turn on the lead's files comes when the one using them finishes. You can reply once it starts.",
        ),
      ).toBeVisible(),
    );
  });
});

// WEB-U-419 (c17) on the page: the waiting helper names the task using the lead's files,
// as the files' lease gives it.
describe("WEB-U-419 a waiting helper's page names the task using the lead's files", () => {
  it("names the lease's task in the composer", async () => {
    const read = vi.spyOn(api, "workspace").mockResolvedValue(
      workspace({
        id: leadId,
        lease: {
          conversation_id: helperId(1),
          title: "helper-a",
          running: true,
          pinned: false,
        },
      }),
    );
    await show(helperRecord({}), [
      {
        ...ready,
        conversation_id: helperId(3),
        sandbox: { backend: "docker", state: "queued" },
        queue: { ahead: 0, reason: "workspace" },
      },
    ]);
    await waitFor(() =>
      expect(
        screen.getByText(
          "Its turn on the lead's files comes when “helper-a” is done with them. You can reply once it starts.",
        ),
      ).toBeVisible(),
    );
    expect(read).toHaveBeenCalledWith(leadId, expect.anything());
  });
  // WEB-U-419: a lease the task itself holds names no one; the general words stay.
  it("keeps the general words when the lease is the task's own", async () => {
    vi.spyOn(api, "workspace").mockResolvedValue(
      workspace({
        id: leadId,
        lease: {
          conversation_id: helperId(3),
          title: "helper-c",
          running: false,
          pinned: false,
        },
      }),
    );
    await show(helperRecord({}), [
      {
        ...ready,
        conversation_id: helperId(3),
        sandbox: { backend: "docker", state: "queued" },
        queue: { ahead: 0, reason: "workspace" },
      },
    ]);
    await waitFor(() =>
      expect(
        screen.getByText(
          "Its turn on the lead's files comes when the one using them finishes. You can reply once it starts.",
        ),
      ).toBeVisible(),
    );
    expect(screen.queryByText(/“helper-c”/)).toBeNull();
  });
});

// WEB-U-412 (c17 U7; 16u live L3, pages-stop-10s-helper-helper-c): a helper the team's stop stopped before it ever ran
// read "Send a message to begin. Your workspace starts empty.": wrong for a helper on the lead's files, and silent
// about the stop. It says it was stopped before it started and made no changes.
describe("WEB-U-412 a helper stopped before it started says so", () => {
  it("says it was stopped, not that its workspace starts empty", async () => {
    await show(helperRecord({ state: "ended", helper_state: "stopped" }), [
      { ...ready, conversation_id: helperId(3) },
    ]);
    await waitFor(() =>
      expect(
        screen.getByText(
          "This helper was stopped before it started, when its team stopped. It made no changes.",
        ),
      ).toBeVisible(),
    );
    expect(screen.queryByText(/Send a message to begin/)).toBeNull();
  });
});

// WEB-U-413 (c17 U8; 16u live L3): after Stop team on a lead whose own turn had already ended, its header read Done
// above "You stopped the team." A lead whose team its person stopped reads Stopped; a run the person starts after the
// stop reads Done again when it completes.
describe("WEB-U-413 a lead whose team a person stopped reads Stopped", () => {
  const lead = record({
    run_status: "completed",
    last_activity_at: "2026-01-01T00:10:00Z",
    team: {
      role: "lead",
      id: teamId,
      state: "ended",
      helpers: 3,
      attention: { stuck: 0, failed: 0 },
      waiting_for_you: false,
    },
  });
  const ran = [
    ready,
    frame(1, { type: "message.user", text: "Lead a team", attachments: [] }),
    frame(2, { type: "run.started", run_id: runId }),
    frame(3, { type: "run.completed", run_id: runId }),
  ];
  const stopped = (at: string) => {
    vi.mocked(readTeam).mockResolvedValue(
      team({
        state: "ended",
        stop_reason: "stopped",
        helpers: [helper(1, "helper-a", { state: "stopped", activity: null })],
      }),
    );
    vi.mocked(readTeamMessages).mockResolvedValue({
      items: [
        teamMessage(9, "ended", {
          from: { kind: "gateway" },
          to: { kind: "team" },
          at,
          reason: "stopped",
          tokens: { total: 0, members: [] },
        }),
      ],
      last_seq: 9,
      more: false,
    });
  };

  it("reads Stopped after the person's Stop team", async () => {
    stopped("2026-01-01T00:05:00Z");
    await show(lead, ran);
    await waitFor(() => expect(readTeamMessages).toHaveBeenCalled());
    const status = () => screen.getByLabelText("Run status");
    await waitFor(() => expect(status()).toHaveTextContent("Stopped"));
    expect(status()).not.toHaveTextContent("Done");
  });

  it("reads Done for a run the person started after the stop", async () => {
    stopped("2025-12-31T00:00:00Z");
    await show(lead, ran);
    await waitFor(() => expect(readTeamMessages).toHaveBeenCalled());
    await act(async () => {
      await Promise.resolve();
    });
    await waitFor(() =>
      expect(screen.getByLabelText("Run status")).toHaveTextContent("Done"),
    );
  });
});

// WEB-U-424 (c17's candidate shots, S4): the end card said "1 report from late-files" under the gateway's line that
// late-files "stopped before it reported". The gateway's hand-back of a helper's work (GW-I-311) isn't that helper's
// report: the card counts only the reports the helpers wrote; what the gateway handed back stays listed as files.
describe("WEB-U-424 the team's end card counts only the helpers' own reports", () => {
  it("leaves the gateway's hand-back out of the count", async () => {
    vi.mocked(readTeam).mockResolvedValue(
      team({
        state: "ended",
        stop_reason: "stopped",
        helpers: [
          helper(1, "helper-a", { state: "done", activity: null }),
          helper(3, "helper-c", { state: "stopped", activity: null }),
        ],
      }),
    );
    vi.mocked(readTeamMessages).mockResolvedValue({
      items: [
        teamMessage(7, "report", {
          from: { kind: "member", name: "helper-a" },
          to: { kind: "lead" },
          text: "Here is my part.",
        }),
        teamMessage(8, "report", {
          from: { kind: "gateway" },
          to: { kind: "lead" },
          member: "helper-c",
          text: "The gateway hands back helper-c's work: helper-c stopped before it reported (the team was stopped).",
        }),
        teamMessage(9, "ended", {
          from: { kind: "gateway" },
          to: { kind: "team" },
          reason: "stopped",
          tokens: { total: 0, members: [] },
          reports: [
            { member: "helper-a", seq: 7 },
            { member: "helper-c", seq: 8 },
          ],
          files: [{ member: "helper-c", seq: 8, name: "notes.md", size: 12 }],
        }),
      ],
      last_seq: 9,
      more: false,
    });
    await show(
      record({
        run_status: "completed",
        team: {
          role: "lead",
          id: teamId,
          state: "ended",
          helpers: 2,
          attention: { stuck: 0, failed: 0 },
          waiting_for_you: false,
        },
      }),
      [
        ready,
        frame(1, {
          type: "message.user",
          text: "Lead a team",
          attachments: [],
        }),
        frame(2, { type: "run.started", run_id: runId }),
        frame(3, { type: "run.completed", run_id: runId }),
      ],
    );
    const card = await screen.findByRole("region", {
      name: "You stopped the team.",
    });
    expect(card).toHaveTextContent("1 report from helper-a");
    expect(card).not.toHaveTextContent("report from helper-c");
    // What the gateway handed back is still listed.
    expect(card).toHaveTextContent(/Handed back: notes\.md \(.*helper-c\)/);
  });
});

// WEB-U-425 (c17's candidate shots, S5): a helper stopped before it started said so in its page, but its header read
// Ready, as if it could begin. Its header reads Stopped. A helper stopped after it started reads as it did before:
// its own run's end.
describe("WEB-U-425 a helper stopped before it started reads Stopped in its header", () => {
  it("reads Stopped, never Ready", async () => {
    await show(helperRecord({ state: "ended", helper_state: "stopped" }), [
      { ...ready, conversation_id: helperId(3) },
    ]);
    await screen.findByText(
      "This helper was stopped before it started, when its team stopped. It made no changes.",
    );
    const status = () => screen.getByLabelText("Run status");
    await waitFor(() => expect(status()).toHaveTextContent("Stopped"));
    expect(status()).not.toHaveTextContent("Ready");
  });

  it("leaves a helper stopped after it started reading its run's end", async () => {
    await show(helperRecord({ state: "ended", helper_state: "stopped" }), [
      { ...ready, conversation_id: helperId(3) },
      frame(1, { type: "message.user", text: "Your part", attachments: [] }),
      frame(2, { type: "run.started", run_id: runId }),
      frame(3, { type: "run.completed", run_id: runId }),
    ]);
    await waitFor(() =>
      expect(screen.getByLabelText("Run status")).toHaveTextContent("Done"),
    );
    expect(screen.queryByText(/stopped before it started/)).toBeNull();
  });
});

import {
  QueryClient,
  QueryClientProvider,
  useQuery,
} from "@tanstack/react-query";
import { act, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { api } from "../api/client";
import type { Conversation, TeamMessage } from "../protocol/generated/api";
import { frame, ready, runId } from "../test/frames";
import { helper, leadId, team, teamId, teamMessage } from "../test/teams";
import { templates } from "../test/templates";
import { shellData } from "../test/workspaces";
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

/** The browser's event stream: opened by the page, but it replays nothing in these tests. */
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

// The lead's record as the page gets it: its last run completed, its team runs.
const lead: Conversation = {
  id: leadId,
  owner_id: "0195d8c0-0000-7000-8000-000000000009",
  title: "EDGE-L end a turn without done",
  model_route: "local",
  template: "workstation",
  pinned: false,
  title_source: "person",
  backend: "docker",
  state: "active",
  created_at: "2026-10-18T07:00:00Z",
  updated_at: "2026-10-18T07:00:00Z",
  last_activity_at: "2026-10-18T07:00:00Z",
  workspace_id: leadId,
  mcp_servers: [],
  connectors: [],
  run_status: "completed",
  team: {
    role: "lead",
    id: teamId,
    state: "running",
    helpers: 1,
    attention: { stuck: 0, failed: 0 },
    waiting_for_you: false,
  },
};
const reminder = (seq: number): TeamMessage =>
  teamMessage(seq, "notice", {
    from: { kind: "gateway" },
    to: { kind: "lead" },
    kind: "lead_unfinished",
    text: "",
  });

function show() {
  const cache = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  cache.setQueryData(["templates"], templates);
  shellData(cache, []);
  cache.setQueryData(["models"], { items: [] });
  cache.setQueryData(["conversation", lead.id], lead);
  render(<LiveConversation conversation={lead} />, {
    wrapper: ({ children }) => (
      <QueryClientProvider client={cache}>{children}</QueryClientProvider>
    ),
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

// WEB-U-405 (16r int, E2E-AGENTS-032 after WEB-U-404): a lead's page opened after its turns had ended read Done in its
// first moments. The team layer answered before the stream replayed a frame, so the layer knew the team runs but not
// that the lead's run was over, and the header fell back to the stored status: Done. Before the replay the stored
// status says how the run ended, for the layer's words as for the header's.
describe("WEB-U-405 a running team's lead never reads Done on its page's first render", () => {
  it("says the lead stopped without finishing, from the stored status, before the stream replays", async () => {
    vi.mocked(readTeam).mockResolvedValue(
      team({
        helpers: [helper(1, "researcher", { state: "done", activity: null })],
      }),
    );
    vi.mocked(readTeamMessages).mockResolvedValue({
      items: [
        teamMessage(1, "assign", { text: "Collect the figures." }),
        reminder(2),
        reminder(3),
      ],
      last_seq: 3,
      more: false,
    });
    show();
    const status = () => screen.getByLabelText("Run status");
    expect(status()).not.toHaveTextContent("Done");
    await waitFor(() => expect(readTeamMessages).toHaveBeenCalled());
    await act(async () => {
      await Promise.resolve();
    });
    await waitFor(() =>
      expect(status()).toHaveTextContent(
        "The lead stopped without finishing the team. Stop the team, or send a message.",
      ),
    );
    expect(status()).not.toHaveTextContent("Done");
  });

  it("says the team waits for the lead before any reminder, never Done", async () => {
    vi.mocked(readTeam).mockResolvedValue(
      team({
        helpers: [helper(1, "researcher", { state: "done", activity: null })],
      }),
    );
    vi.mocked(readTeamMessages).mockResolvedValue({
      items: [teamMessage(1, "assign", { text: "Collect the figures." })],
      last_seq: 1,
      more: false,
    });
    show();
    await waitFor(() => expect(readTeamMessages).toHaveBeenCalled());
    await waitFor(() =>
      expect(screen.getByLabelText("Run status")).toHaveTextContent(
        "Waiting for the lead to finish the team",
      ),
    );
    expect(screen.getByLabelText("Run status")).not.toHaveTextContent("Done");
  });
});

/** The task's page as the app has it: the task's own record read, and read again when the team changes. */
function TaskPage({ id }: { id: string }) {
  const current = useQuery({
    queryKey: ["conversation", id],
    queryFn: ({ signal }) => api.conversation(id, signal),
    staleTime: 30_000,
  });
  return current.data ? <LiveConversation conversation={current.data} /> : null;
}

// WEB-U-405 (b): a page opened before its task led a team. The first team frame reads the task again (WEB-U-326); a
// read that comes back before the task's place is in, and a team whose later frames only add messages (the gateway's
// reminders), must not leave the record without its team, so the header read Done once the lead's turn ended.
describe("WEB-U-405 a page opened before its task led a team never reads Done once the lead's turn ends", () => {
  it("reads the task again on the team's later messages until its place arrives, and never says Done", async () => {
    vi.mocked(readTeam).mockResolvedValue(
      team({
        helpers: [helper(1, "researcher", { state: "done", activity: null })],
      }),
    );
    vi.mocked(readTeamMessages).mockResolvedValue({
      items: [reminder(2), reminder(3)],
      last_seq: 3,
      more: false,
    });
    const before: Conversation = { ...lead, team: null, run_status: "idle" };
    const reads = vi
      .spyOn(api, "conversation")
      .mockResolvedValueOnce(before)
      .mockResolvedValue({ ...lead, run_status: "completed" });
    const cache = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    cache.setQueryData(["templates"], templates);
    shellData(cache, []);
    cache.setQueryData(["models"], { items: [] });
    cache.setQueryData(["conversation", lead.id], before);
    render(<TaskPage id={lead.id} />, {
      wrapper: ({ children }) => (
        <QueryClientProvider client={cache}>{children}</QueryClientProvider>
      ),
    });
    const stream = Stream.opened[0];
    const send = async (value: unknown) =>
      act(async () => {
        stream?.send(value);
        await Promise.resolve();
      });
    await act(async () => {
      stream?.open();
      stream?.send(ready);
      await Promise.resolve();
    });
    await send(frame(1, { type: "run.started", run_id: runId }));
    await send({ type: "team.changed", team: teamId, version: 9, last_seq: 1 });
    // The first read comes back before the task's place is in.
    await waitFor(() => expect(reads).toHaveBeenCalledTimes(1));
    await send(frame(2, { type: "run.completed", run_id: runId }));
    // The gateway's reminders: the team's log grows, its version doesn't.
    await send({ type: "team.changed", team: teamId, version: 9, last_seq: 3 });
    const status = () => screen.getByLabelText("Run status");
    await waitFor(() =>
      expect(status()).toHaveTextContent(
        "The lead stopped without finishing the team. Stop the team, or send a message.",
      ),
    );
    expect(reads.mock.calls.length).toBeGreaterThanOrEqual(2);
  });
});

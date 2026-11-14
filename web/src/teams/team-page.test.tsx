import {
  QueryClient,
  QueryClientProvider,
  useQuery,
} from "@tanstack/react-query";
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import {
  afterEach,
  beforeEach,
  describe,
  expect,
  it,
  type MockInstance,
  vi,
} from "vitest";

import type { Action } from "../api/action";
import { api, ApiError } from "../api/client";
import type {
  Conversation,
  TeamSeq,
  TeamStopped,
  Workspace,
} from "../protocol/generated/api";
import { frame, ready } from "../test/frames";
import { team, teamMessage, leadId, helperId, teamId } from "../test/teams";
import { templates } from "../test/templates";
import { shellData, workspace } from "../test/workspaces";
import { LiveConversation } from "../transcript/LiveConversation";

import { answerTeam, readTeam, readTeamMessages, stopTeam } from "./requests";

vi.mock("./requests", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./requests")>();
  // Spies over the real requests: a test overrides one; the rest go through the gateway as the page's would.
  return {
    readTeam: vi.fn(actual.readTeam),
    readTeamMessages: vi.fn(actual.readTeamMessages),
    answerTeam: vi.fn(actual.answerTeam),
    stopTeam: vi.fn(actual.stopTeam),
  };
});

// Each test starts from the real requests: no calls counted and no answer left over from the one before.
beforeEach(() => {
  vi.mocked(readTeam).mockReset();
  vi.mocked(readTeamMessages).mockReset();
  vi.mocked(answerTeam).mockReset();
  vi.mocked(stopTeam).mockReset();
});

// ADR-0034 S6 on a task's page: the lead's strip, log, question and Stop; a helper's place; nothing while teams are off.
let teamsOn = true;
/** Whether the gateway's list has the helper's first turn waiting (WEB-U-388). */
let briefWaiting = false;
vi.mock("../transcript/use-held", async (importOriginal) => ({
  gatewayTurn: (await importOriginal<typeof import("../transcript/use-held")>())
    .gatewayTurn,
  useHeldMessages: () => ({
    held: [],
    waiting: [],
    listed: null,
    dropped: [],
    wakeWaiting: false,
    briefWaiting,
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
      features: { agent_teams: teamsOn },
    },
  }),
}));

/** The browser's event stream, as the page opens it. */
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

const task = (id: string, extra: Partial<Conversation>): Conversation => ({
  id,
  owner_id: "0195d8c0-0000-7000-8000-000000000009",
  title: "Report on Q3",
  model_route: "local",
  template: "workstation",
  pinned: false,
  title_source: "person",
  backend: "docker",
  state: "active",
  created_at: "2026-10-02T20:00:00Z",
  updated_at: "2026-10-02T20:00:00Z",
  last_activity_at: "2026-10-02T20:00:00Z",
  workspace_id: id,
  mcp_servers: [],
  connectors: [],
  team: null,
  ...extra,
});
const leadTask = task(leadId, {
  team: {
    role: "lead",
    id: teamId,
    state: "running",
    helpers: 2,
    attention: { stuck: 0, failed: 0 },
    waiting_for_you: true,
  },
});
const asking = team({
  waiting_for_you: {
    seq: 4,
    text: "Which quarter counts as Q3?",
    at: "2026-10-02T20:30:00Z",
  },
});
const log = {
  items: [
    teamMessage(1, "assign", { text: "Collect the Q3 figures." }),
    teamMessage(4, "person_question", {
      from: { kind: "lead" },
      to: { kind: "person" },
      text: "Which quarter counts as Q3?",
      at: "2026-10-02T20:30:00Z",
    }),
  ],
  last_seq: 4,
  more: false,
};
// The answer an action gives, made when it's executed: a refusal made earlier would be a rejection nobody handled yet.
const action = <T,>(result: () => Promise<T>) =>
  ({ execute: result }) as unknown as Action<T>;

function show(conversation: Conversation, workspaces: Workspace[] = []) {
  const cache = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  cache.setQueryData(["templates"], templates);
  shellData(cache, workspaces);
  cache.setQueryData(["models"], { items: [] });
  cache.setQueryData(["conversation", conversation.id], conversation);
  render(<LiveConversation conversation={conversation} />, {
    wrapper: ({ children }) => (
      <QueryClientProvider client={cache}>{children}</QueryClientProvider>
    ),
  });
  return cache;
}
async function live() {
  const stream = Stream.opened[0];
  await act(async () => {
    stream?.open();
    stream?.send(ready);
    await Promise.resolve();
  });
  return stream;
}

let teamReads: MockInstance<typeof readTeam>;
let readLog: MockInstance<typeof readTeamMessages>;
beforeEach(() => {
  teamsOn = true;
  briefWaiting = false;
  Stream.opened = [];
  vi.stubGlobal("EventSource", Stream);
  vi.spyOn(api, "files").mockResolvedValue({
    path: "",
    items: [],
    next_after: null,
  });
  vi.spyOn(api, "keyUses").mockResolvedValue({ items: [], next: null });
  teamReads = vi.mocked(readTeam).mockResolvedValue(asking);
  readLog = vi.mocked(readTeamMessages).mockResolvedValue(log);
});
afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

describe("WEB-U-221/222/224 a lead task's page", () => {
  it("shows the strip, the log among its turns, and the question with Waiting for you as its status", async () => {
    show(leadTask);
    await live();
    const strip = await screen.findByRole("region", { name: "Helpers" });
    expect(
      within(strip).getByRole("link", { name: "researcher: Working…" }),
    ).toHaveAttribute("href", `/c/${helperId(1)}`);
    expect(
      await screen.findByText("To researcher: Collect the Q3 figures."),
    ).toBeVisible();
    expect(screen.getByText("Which quarter counts as Q3?")).toBeVisible();
    expect(screen.getByText("Answer in the message box below.")).toBeVisible();
    expect(screen.getByLabelText("Run status")).toHaveTextContent(
      "Waiting for you",
    );
  });

  // WEB-U-411 (17.0's release candidate): with why it is no longer needed.
  it.each([
    ["has ended", "ended", null, "the team has ended"],
    ["is stopping", "stopping", null, "the team is stopping"],
    [
      "asked again",
      "running",
      { seq: 6, text: "And which year?", at: "2026-10-02T20:40:00Z" },
      "the lead asked something else",
    ],
  ] as const)(
    "says an unanswered question is no longer needed once the team %s, and why",
    async (_, state, waiting, why) => {
      teamReads.mockResolvedValue(team({ state, waiting_for_you: waiting }));
      show(
        task(leadId, {
          team: {
            role: "lead",
            id: teamId,
            state,
            helpers: 2,
            attention: { stuck: 0, failed: 0 },
            waiting_for_you: waiting !== null,
          },
        }),
      );
      await live();
      const card = await screen.findByRole("region", {
        name: `No longer needed: ${why}`,
      });
      expect(card).toHaveTextContent("Which quarter counts as Q3?");
      expect(
        screen.queryByText("Answer in the message box below."),
      ).not.toBeInTheDocument();
      if (state !== "running")
        expect(screen.getByLabelText("Run status")).not.toHaveTextContent(
          "Waiting for you",
        );
    },
  );

  it("sends the message box as the answer, then reads the team and its log again", async () => {
    const answer = vi
      .mocked(answerTeam)
      .mockReturnValue(action<TeamSeq>(() => Promise.resolve({ seq: 5 })));
    show(leadTask);
    await live();
    await screen.findByText("Which quarter counts as Q3?");
    const box = screen.getByRole("textbox", { name: "Message" });
    fireEvent.change(box, { target: { value: "July to September" } });
    fireEvent.submit(box.closest("form") as HTMLFormElement);
    await waitFor(() =>
      expect(answer).toHaveBeenCalledWith(expect.anything(), leadId, {
        question_seq: 4,
        text: "July to September",
      }),
    );
    await waitFor(() => expect(teamReads).toHaveBeenCalledTimes(2));
  });

  it.each([
    ["already_answered", 409, "This question was already answered."],
    [
      "team_stopped",
      409,
      "The team has stopped, so the lead won't get this answer.",
    ],
    ["unknown_reference", 422, "This question is no longer open."],
  ])(
    "says a refused answer (%s) in words and keeps the text",
    async (code, status, words) => {
      vi.mocked(answerTeam).mockReturnValue(
        action<TeamSeq>(() =>
          Promise.reject(new ApiError(status, code, "refused")),
        ),
      );
      show(leadTask);
      await live();
      await screen.findByText("Which quarter counts as Q3?");
      const box = screen.getByRole("textbox", { name: "Message" });
      fireEvent.change(box, { target: { value: "July to September" } });
      fireEvent.submit(box.closest("form") as HTMLFormElement);
      expect(await screen.findByRole("alert")).toHaveTextContent(words);
      expect(box).toHaveValue("July to September");
    },
  );

  it("stops the team from its ⋯ after asking", async () => {
    const stop = vi
      .mocked(stopTeam)
      .mockReturnValue(
        action<TeamStopped>(() =>
          Promise.resolve({ team: { state: "stopping" } }),
        ),
      );
    show(leadTask);
    await live();
    await screen.findByRole("region", { name: "Helpers" });
    fireEvent.click(
      screen.getByRole("button", { name: "More actions for Report on Q3" }),
    );
    fireEvent.click(screen.getByRole("menuitem", { name: "Stop team" }));
    const dialog = await screen.findByRole("dialog", {
      name: "Stop the team?",
    });
    expect(dialog).toHaveTextContent(
      "The lead and 2 helpers stop now. Their tasks and files stay.",
    );
    fireEvent.click(within(dialog).getByRole("button", { name: "Stop team" }));
    await waitFor(() =>
      expect(stop).toHaveBeenCalledWith(expect.anything(), leadId),
    );
  });
});

/** The task's page as ConversationPage gives it: the task as its query holds it, read again when that query is. */
function TaskPage({ id }: { id: string }) {
  const current = useQuery({
    queryKey: ["conversation", id],
    queryFn: ({ signal }) => api.conversation(id, signal),
    staleTime: 30_000,
  });
  return current.data ? <LiveConversation conversation={current.data} /> : null;
}

describe("WEB-U-326 the page that started the team", () => {
  function showPage(conversation: Conversation) {
    const cache = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    cache.setQueryData(["templates"], templates);
    shellData(cache, []);
    cache.setQueryData(["models"], { items: [] });
    cache.setQueryData(["conversation", conversation.id], conversation);
    render(<TaskPage id={conversation.id} />, {
      wrapper: ({ children }) => (
        <QueryClientProvider client={cache}>{children}</QueryClientProvider>
      ),
    });
  }
  const started = {
    type: "team.changed",
    team: teamId,
    version: 9,
    last_seq: 1,
  };

  it("shows the strip as soon as the team starts, without a reload", async () => {
    const reads = vi.spyOn(api, "conversation").mockResolvedValue(leadTask);
    showPage(task(leadId, { team: null }));
    const stream = await live();
    expect(
      screen.queryByRole("region", { name: "Helpers" }),
    ).not.toBeInTheDocument();
    await act(async () => {
      stream?.send(started);
      await Promise.resolve();
    });
    expect(
      await screen.findByRole("region", { name: "Helpers" }),
    ).toBeVisible();
    expect(reads).toHaveBeenCalledWith(leadId, expect.anything());
  });

  it("reads the task again on the next change while its place hasn't arrived", async () => {
    const reads = vi
      .spyOn(api, "conversation")
      .mockResolvedValueOnce(task(leadId, { team: null }))
      .mockResolvedValue(leadTask);
    showPage(task(leadId, { team: null }));
    const stream = await live();
    await act(async () => {
      stream?.send(started);
      await Promise.resolve();
    });
    await waitFor(() => expect(reads).toHaveBeenCalledTimes(1));
    await act(async () => {
      stream?.send({ ...started, version: 10 });
      await Promise.resolve();
    });
    expect(
      await screen.findByRole("region", { name: "Helpers" }),
    ).toBeVisible();
    // The second change read it again; the layer, once there, reads it on the changes it sees as before.
    expect(reads.mock.calls.length).toBeGreaterThanOrEqual(2);
  });

  it("reads nothing again while teams are off", async () => {
    teamsOn = false;
    const reads = vi.spyOn(api, "conversation").mockResolvedValue(leadTask);
    showPage(task(leadId, { team: null }));
    const stream = await live();
    await act(async () => {
      stream?.send(started);
      await Promise.resolve();
    });
    expect(reads).not.toHaveBeenCalled();
    expect(
      screen.queryByRole("region", { name: "Helpers" }),
    ).not.toBeInTheDocument();
  });
});

describe("WEB-U-229 team.changed", () => {
  it("reads the team when its version rose, and only the newer messages when its last seq did", async () => {
    show(leadTask);
    const stream = await live();
    await screen.findByRole("region", { name: "Helpers" });
    expect(teamReads).toHaveBeenCalledTimes(1);
    expect(readLog).toHaveBeenLastCalledWith(
      expect.anything(),
      leadId,
      0,
      expect.anything(),
    );
    // The same version: the team isn't read again.
    await act(async () => {
      stream?.send({
        type: "team.changed",
        team: teamId,
        version: 9,
        last_seq: 4,
      });
      await Promise.resolve();
    });
    expect(teamReads).toHaveBeenCalledTimes(1);
    await act(async () => {
      stream?.send({
        type: "team.changed",
        team: teamId,
        version: 10,
        last_seq: 6,
      });
      await Promise.resolve();
    });
    await waitFor(() => expect(teamReads).toHaveBeenCalledTimes(2));
    await waitFor(() =>
      expect(readLog).toHaveBeenLastCalledWith(
        expect.anything(),
        leadId,
        4,
        expect.anything(),
      ),
    );
  });
});

// A log that can't be read is said in its place, never left out silently, and read again: on Try again, and on the
// team's next change (the run where a contract refusal emptied the lead's page).
describe("WEB-U-229 a team log that can't be read", () => {
  it("says so with Try again, which reads it again", async () => {
    readLog.mockRejectedValueOnce(new Error("contract"));
    show(leadTask);
    await live();
    const problem = await screen.findByText(
      "The team's messages couldn't be read.",
    );
    expect(
      screen.queryByText("To researcher: Collect the Q3 figures."),
    ).not.toBeInTheDocument();
    fireEvent.click(
      within(problem.closest("[role=alert]") as HTMLElement).getByRole(
        "button",
        { name: "Try again" },
      ),
    );
    expect(
      await screen.findByText("To researcher: Collect the Q3 figures."),
    ).toBeVisible();
    expect(
      screen.queryByText("The team's messages couldn't be read."),
    ).not.toBeInTheDocument();
  });

  it("reads it again when the team changes", async () => {
    readLog.mockRejectedValueOnce(new Error("contract"));
    show(leadTask);
    const stream = await live();
    await screen.findByText("The team's messages couldn't be read.");
    await act(async () => {
      stream?.send({
        type: "team.changed",
        team: teamId,
        version: 9,
        last_seq: 5,
      });
      await Promise.resolve();
    });
    expect(
      await screen.findByText("To researcher: Collect the Q3 figures."),
    ).toBeVisible();
    // Read twice (Try again's read and the change's) and shown once: the second read's repeat is dropped. A loaded CI
    // runner showed it twice (14 Nov 2026).
    await new Promise((resolve) => setTimeout(resolve, 100));
    expect(
      screen.getAllByText("To researcher: Collect the Q3 figures."),
    ).toHaveLength(1);
  });
});

describe("WEB-U-223 a helper's page", () => {
  it("says whose helper it is, with the lead linked, and that the lead sees what is sent", async () => {
    show(
      task(helperId(1), {
        title: "researcher",
        team: {
          role: "member",
          id: teamId,
          state: "running",
          lead: { id: leadId, title: "Report on Q3" },
          name: "researcher",
          helper_state: "working",
        },
      }),
    );
    await live();
    const leadLink = await screen.findByRole("link", { name: "Report on Q3" });
    const place = leadLink.closest("p");
    expect(place).toHaveTextContent("Helper of Report on Q3 · researcher");
    expect(leadLink).toHaveAttribute("href", `/c/${leadId}`);
    expect(
      screen.getByRole("textbox", { name: "Message" }),
    ).toHaveAccessibleDescription(/The lead task sees what you send here\./);
    // A helper's page reads no team log: its own turns hold what concerns it.
    expect(readLog).not.toHaveBeenCalled();
  });

  // WEB-U-310 (a UX review before 17.0): the gateway copies a person's message to the lead only while the team runs.
  it("drops the notice that the lead sees what is sent once the team has ended", async () => {
    show(
      task(helperId(1), {
        title: "researcher",
        team: {
          role: "member",
          id: teamId,
          state: "ended",
          lead: { id: leadId, title: "Report on Q3" },
          name: "researcher",
          helper_state: "done",
        },
      }),
    );
    await live();
    await screen.findByRole("link", { name: "Report on Q3" });
    expect(
      screen.queryByText("The lead task sees what you send here."),
    ).toBeNull();
    expect(
      screen.getByRole("textbox", { name: "Message" }),
    ).not.toHaveAccessibleDescription(
      /The lead task sees what you send here\./,
    );
  });
});

// WEB-U-308: offered where the gateway's can_stop says so (GW-U-168), whatever the state, so a helper whose run failed
// can be stopped and its seat freed; a place without can_stop (not the helper's own person) offers none.
describe("WEB-U-225, WEB-U-308 Stop helper on a helper's page", () => {
  it.each([
    ["working", true, true],
    ["done", true, true],
    ["failed", true, true],
    ["failed", false, false],
    ["stopped", false, false],
    ["working", undefined, false],
  ] as const)(
    "on a helper %s, can_stop %s: offered %s",
    async (state, canStop, offered) => {
      show(
        task(helperId(1), {
          title: "researcher",
          team: {
            role: "member",
            id: teamId,
            state: "running",
            lead: { id: leadId, title: "Report on Q3" },
            name: "researcher",
            helper_state: state,
            ...(canStop === undefined ? {} : { can_stop: canStop }),
          },
        }),
      );
      await live();
      await screen.findByText(/Helper of/);
      fireEvent.click(
        screen.getByRole("button", { name: "More actions for researcher" }),
      );
      if (offered)
        expect(
          screen.getByRole("menuitem", { name: "Stop helper" }),
        ).toBeVisible();
      else
        expect(
          screen.queryByRole("menuitem", { name: "Stop helper" }),
        ).toBeNull();
    },
  );
});

describe("WEB-U-228 agent teams switched off", () => {
  it("shows no trace of a team and asks for none", async () => {
    teamsOn = false;
    show(leadTask);
    await live();
    await screen.findByRole("textbox", { name: "Message" });
    expect(screen.queryByRole("region", { name: "Helpers" })).toBeNull();
    expect(screen.queryByText(/Waiting for you|Helper of/)).toBeNull();
    expect(teamReads).not.toHaveBeenCalled();
    expect(readLog).not.toHaveBeenCalled();
    fireEvent.click(
      screen.getByRole("button", { name: "More actions for Report on Q3" }),
    );
    expect(screen.queryByRole("menuitem", { name: "Stop team" })).toBeNull();
  });
});

// WEB-U-239 (a pass over the pages before 17.0, at 390): a helper's header says each thing once. "Helper of" is one
// line whose lead title gives way with "…" (whole in its tooltip) while the helper's name stays; and the helper's own
// copy, named by its role like the helper, isn't said again as "In researcher (helper copy)".
describe("WEB-U-239 a helper's header says each thing once", () => {
  const long =
    "You lead a small team. Start two helpers with start_member: a researcher…";
  const copy = workspace({ name: "researcher", kind: "helper_copy" });
  const helper = (title: string, workspaceId: string) =>
    task(helperId(1), {
      title: "researcher",
      workspace_id: workspaceId,
      team: {
        role: "member",
        id: teamId,
        state: "running",
        lead: { id: leadId, title },
        name: "researcher",
        helper_state: "working",
      },
    });

  it("keeps the helper's name whole and the lead's whole title in its tooltip", async () => {
    show(helper(long, copy.id), [copy]);
    await live();
    const leadLink = await screen.findByRole("link", { name: long });
    expect(leadLink).toHaveAttribute("title", long);
    expect(leadLink).toHaveClass("helper-of-lead");
    expect(leadLink.closest("p")).toHaveTextContent(/· researcher$/);
  });

  it("doesn't say the helper's own copy again, but names any other workspace", async () => {
    show(helper("Report on Q3", copy.id), [copy]);
    await live();
    await screen.findByRole("link", { name: "Report on Q3" });
    expect(screen.queryByText(/^In researcher/)).toBeNull();
    cleanup();
    Stream.opened = [];
    const shared = workspace({ name: "Q3 data", kind: "standard" });
    show(helper("Report on Q3", shared.id), [shared]);
    await live();
    expect(await screen.findByText(/In Q3 data/)).toBeVisible();
  });
});

// WEB-U-388 (a team demo before 17.0, as for the wake; GW-I-300): a helper's first turn waiting to go (for a slot,
// or for its turn on the lead's files) is the gateway's, named in the team's own words once its layer is loaded, never
// a raw key before; and once the turn goes, the brief's card takes its place: the page never names the brief twice.
describe("WEB-U-388 a helper's first turn waiting to go", () => {
  const helper = task(helperId(1), {
    title: "researcher",
    team: {
      role: "member",
      id: teamId,
      state: "running",
      lead: { id: leadId, title: "Report on Q3" },
      name: "researcher",
      helper_state: "queued",
    },
  });
  const OBJECTIVE = "Find the Q3 revenue by region.";

  it("says Brief from the lead while it waits, then shows the card alone once it goes", async () => {
    briefWaiting = true;
    show(helper);
    const stream = Stream.opened[0];
    await act(async () => {
      stream?.open();
      stream?.send({
        ...ready,
        sandbox: { backend: "docker", state: "queued" },
      });
      await Promise.resolve();
    });
    expect(document.body).not.toHaveTextContent("team.brief");
    const notice = await screen.findByText("Brief from the lead");
    expect(notice).toHaveClass("transcript-notice");
    expect(
      screen.queryByRole("button", { name: "Cancel this message" }),
    ).toBeNull();
    // It goes: the sandbox is ready and the turn carries the brief. The list may still say it waits a moment longer.
    await act(async () => {
      stream?.send(ready);
      stream?.send(
        frame(1, {
          type: "message.user",
          text: "You are researcher, a helper of the task that leads this agent team.",
          attachments: [],
          lead_brief: {
            role: "researcher",
            workspace: "my_files",
            brief: {
              objective: OBJECTIVE,
              output: "A table.",
              boundaries: "Don't edit the reports.",
            },
          },
        }),
      );
      await Promise.resolve();
    });
    expect(await screen.findByText(OBJECTIVE)).toBeVisible();
    expect(screen.getAllByText("Brief from the lead")).toHaveLength(1);
    expect(
      screen.queryByText("Brief from the lead", {
        selector: ".transcript-notice",
      }),
    ).toBeNull();
  });
});

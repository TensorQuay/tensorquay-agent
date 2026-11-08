import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { api, ApiError } from "../api/client";
import { ConversationList } from "../conversations/ConversationList";
import { DeleteConversation } from "../conversations/DeleteConversation";
import { TaskRow } from "../conversations/TaskRow";
import type { Conversation } from "../protocol/generated/api";
import { helper, helperId, leadId, team, teamId } from "../test/teams";
import { fakeGateway, me, task, workspace } from "../test/workspaces";

import { readTeam } from "./requests";

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
});

let pathname = "/";
vi.mock("next/navigation", () => ({ usePathname: () => pathname }));

// ADR-0034 S6 in the task list (UI-GUIDELINES §13b): helpers are never top-level rows; they follow their lead.
const home = workspace({ name: "reports" });
const teamsOn = { ...me, features: { agent_teams: true } };
const lead = (extra: Partial<NonNullable<Conversation["team"]>> = {}) =>
  task(leadId, home.id, {
    title: "Report on Q3",
    last_activity_at: new Date().toISOString(),
    team: {
      role: "lead",
      id: teamId,
      state: "running",
      helpers: 2,
      attention: { stuck: 0, failed: 0 },
      waiting_for_you: false,
      ...extra,
    },
  });

function show(
  session: typeof me | typeof teamsOn,
  items: Conversation[],
  url: string,
) {
  const gateway = fakeGateway({
    [`GET ${url}`]: () => Response.json({ items, next_cursor: null }),
    "GET /api/v1/workspaces": () => Response.json({ items: [home] }),
    [`GET /api/v1/conversations/${leadId}/team`]: () => Response.json(team()),
  });
  vi.stubGlobal("fetch", gateway.fetch);
  const cache = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  cache.setQueryData(["session"], session);
  render(
    <QueryClientProvider client={cache}>
      <ConversationList />
    </QueryClientProvider>,
  );
  return { gateway, cache };
}
afterEach(() => {
  pathname = "/";
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

describe("WEB-U-220 helpers under their lead in the task list", () => {
  it("lists top-level tasks, a lead's count at its row's end, and its helpers under its chevron", async () => {
    const { gateway } = show(
      teamsOn,
      [lead()],
      "/api/v1/conversations?scope=top",
    );
    const row = await screen.findByRole("link", { name: /^Report on Q3/ });
    expect(row).toHaveAccessibleName(/2 helpers$/);
    expect(row).toHaveTextContent("2");
    // The count's words show on hover too, not only to a screen reader (WEB-U-310).
    expect(row.querySelector("[title='2 helpers']")).not.toBeNull();
    const toggle = screen.getByRole("button", {
      name: "Helpers of Report on Q3",
    });
    expect(toggle).toHaveAttribute("aria-expanded", "false");
    // Closed, the team isn't read.
    expect(gateway.calls.some((call) => call.url.endsWith("/team"))).toBe(
      false,
    );
    fireEvent.click(toggle);
    const helpers = await screen.findByRole("list", { name: "Helpers" });
    expect(
      within(helpers).getByRole("link", { name: "researcher: Working…" }),
    ).toHaveAttribute("href", `/c/${helperId(1)}`);
    expect(
      within(helpers).getByRole("link", {
        name: "writer: Waiting for a slot (1 ahead)",
      }),
    ).toBeVisible();
    expect(toggle).toHaveAttribute("aria-expanded", "true");
  });

  it("says failed or stuck in the count's place, failed first, and Waiting for you", async () => {
    show(
      teamsOn,
      [lead({ attention: { stuck: 1, failed: 1 }, waiting_for_you: true })],
      "/api/v1/conversations?scope=top",
    );
    const row = await screen.findByRole("link", { name: /^Report on Q3/ });
    expect(row).toHaveTextContent("1 failed");
    // §13b: Waiting for you is a filled amber dot in the row, its words in the row's name.
    expect(row.querySelector(".task-waiting")).not.toBeNull();
    expect(row).toHaveAccessibleName(
      "Report on Q3, Waiting for you, 2 helpers, 1 failed, 1 stuck",
    );
  });

  // §13b: closed by default, and only the person opens it, even on a helper's own page.
  it("leaves a lead's helpers closed until the person opens them, on its helper's page too", async () => {
    pathname = `/c/${helperId(1)}`;
    const { gateway } = show(
      teamsOn,
      [lead()],
      "/api/v1/conversations?scope=top",
    );
    const toggle = await screen.findByRole("button", {
      name: "Helpers of Report on Q3",
    });
    expect(toggle).toHaveAttribute("aria-expanded", "false");
    expect(screen.queryByRole("list", { name: "Helpers" })).toBeNull();
    expect(gateway.calls.some((call) => call.url.endsWith("/team"))).toBe(
      false,
    );
  });

  it("says when the helpers can't be read, and reads them again", async () => {
    show(teamsOn, [lead()], "/api/v1/conversations?scope=top");
    let fail = true;
    vi.mocked(readTeam).mockImplementation(() =>
      fail ? Promise.reject(new Error("offline")) : Promise.resolve(team()),
    );
    fireEvent.click(
      await screen.findByRole("button", { name: "Helpers of Report on Q3" }),
    );
    expect(await screen.findByText("Couldn't load the helpers.")).toBeVisible();
    fail = false;
    fireEvent.click(screen.getByRole("button", { name: "Try again" }));
    expect(await screen.findByRole("list", { name: "Helpers" })).toBeVisible();
  });

  // Seen in a browser, the lead's count said "1 stuck" from the list's poll while the open helper
  // row still said Working… The rows read the team again whenever the list's view of the team changes.
  it("reads the open helpers again when the list's view of the team changes, and only then", async () => {
    const { SidebarHelpers } = await import("./SidebarHelpers");
    vi.mocked(readTeam)
      .mockResolvedValueOnce(team({ helpers: [helper(1, "researcher")] }))
      .mockResolvedValue(
        team({ helpers: [helper(1, "researcher", { state: "stuck" })] }),
      );
    const cache = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    const at = (signal: string) => (
      <QueryClientProvider client={cache}>
        <SidebarHelpers leadId={leadId} pathname="/" signal={signal} />
      </QueryClientProvider>
    );
    const shown = render(at("running|2|0|0|false|t1"));
    expect(
      await screen.findByRole("link", { name: "researcher: Working…" }),
    ).toBeVisible();
    shown.rerender(at("running|2|0|0|false|t1"));
    expect(readTeam).toHaveBeenCalledOnce();
    shown.rerender(at("running|2|1|0|false|t2"));
    expect(
      await screen.findByRole("link", { name: "researcher: Stuck" }),
    ).toBeVisible();
    expect(readTeam).toHaveBeenCalledTimes(2);
  });

  it("WEB-U-228 lists every task as before while agent teams are off: no scope, no count, no chevron", async () => {
    show(me, [lead()], "/api/v1/conversations");
    const row = await screen.findByRole("link", { name: /^Report on Q3/ });
    expect(row).toHaveAccessibleName("Report on Q3");
    expect(screen.queryByRole("button", { name: /Helpers of/ })).toBeNull();
  });
});

describe("WEB-U-220 a helper found by a search", () => {
  it("says whose helper it is, in its row and its name", () => {
    const cache = new QueryClient();
    render(
      <QueryClientProvider client={cache}>
        <ul>
          <TaskRow
            conversation={task(helperId(1), home.id, {
              title: "researcher",
            })}
            current={false}
            workspace=""
            under="Helper of Report on Q3"
            withMenu={false}
            onAction={() => undefined}
          />
        </ul>
      </QueryClientProvider>,
    );
    const row = screen.getByRole("link", {
      name: /^researcher/,
    });
    expect(row).toHaveTextContent("Helper of Report on Q3");
    expect(row).toHaveAccessibleName("researcher, Helper of Report on Q3");
  });
});

describe("WEB-U-226 deleting a lead task", () => {
  function confirm(conversation: Conversation, session = teamsOn) {
    const cache = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    cache.setQueryData(["session"], session);
    render(
      <QueryClientProvider client={cache}>
        <DeleteConversation conversation={conversation} onClose={vi.fn()} />
      </QueryClientProvider>,
    );
    return screen.getByRole("dialog", { name: "Delete task" });
  }

  it("names its helpers and deletes it with them", async () => {
    vi.mocked(readTeam).mockResolvedValue(team());
    const remove = vi
      .spyOn(api, "deleteConversation")
      .mockResolvedValue(undefined);
    const dialog = confirm(lead());
    expect(
      await within(dialog).findByText(
        "Its 2 helpers, researcher and writer, are deleted with it.",
      ),
    ).toBeVisible();
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Delete task" }),
    );
    await waitFor(() =>
      expect(remove).toHaveBeenCalledWith(leadId, expect.anything(), true),
    );
  });

  it("reads the team again when a helper started since, and says so", async () => {
    const read = vi
      .mocked(readTeam)
      .mockResolvedValueOnce(team({ helpers: [] }))
      .mockResolvedValue(team({ helpers: [helper(3, "checker")] }));
    vi.spyOn(api, "deleteConversation").mockRejectedValue(
      new ApiError(409, "has_helpers", "refused"),
    );
    const dialog = confirm(lead());
    await waitFor(() => expect(read).toHaveBeenCalledOnce());
    // Delete waits for the team's read, so the confirmation can name what goes.
    const remove = within(dialog).getByRole("button", { name: "Delete task" });
    await waitFor(() => expect(remove).toBeEnabled());
    fireEvent.click(remove);
    expect(
      await within(dialog).findByText(
        "A helper started since you opened this. Check the names above and delete again.",
      ),
    ).toBeVisible();
    expect(
      await within(dialog).findByText("Its helper checker is deleted with it."),
    ).toBeVisible();
  });

  it("WEB-U-228 deletes as any task while agent teams are off", async () => {
    const read = vi.mocked(readTeam);
    const remove = vi
      .spyOn(api, "deleteConversation")
      .mockResolvedValue(undefined);
    const dialog = confirm(lead(), me);
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Delete task" }),
    );
    await waitFor(() =>
      expect(remove).toHaveBeenCalledWith(leadId, expect.anything(), false),
    );
    expect(read).not.toHaveBeenCalled();
  });
});

// WEB-U-409 (17.0's release candidate; a live check before 17.0): the sidebar showed the red failed mark on a lead
// whose last step had failed while its team ran on; its page said it waits for its helpers, who wake it. While its team
// runs, a lead's failed run is not the task's outcome: no red mark until the team has ended.
describe("WEB-U-409 no red failure mark on a lead whose team still runs", () => {
  it("shows none while the team runs", async () => {
    show(
      teamsOn,
      [{ ...lead(), run_status: "failed" }],
      "/api/v1/conversations?scope=top",
    );
    const row = await screen.findByRole("link", { name: /^Report on Q3/ });
    expect(row).toHaveAccessibleName("Report on Q3, 2 helpers");
    expect(row).not.toHaveAccessibleName(/Failed/);
  });

  it("shows it once the team has ended", async () => {
    show(
      teamsOn,
      [{ ...lead({ state: "ended" }), run_status: "failed" }],
      "/api/v1/conversations?scope=top",
    );
    const row = await screen.findByRole("link", { name: /^Report on Q3/ });
    expect(row).toHaveAccessibleName(/^Report on Q3, Failed/);
  });
});

import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { ApiError } from "../api/error";
import { member } from "../test/channels";
import { choose } from "../test/picker";

import { MemberList } from "./ChannelMembers";
import type { ChannelPost, Member, TaskState } from "./contract";
import { TaskBoard } from "./TaskBoard";

const CODER = member("conversation", "Coder", {
  owner_name: "alice",
  role: "lead",
  run_status: "failed",
});
const REVIEWER = member("conversation", "Reviewer", {
  owner_name: "bob",
  role: "member",
  run_status: "running",
  title: "Review the release",
});
const ANA = member("user", "Ana", { role: "member" });
const WATCHER = member("user", "Wu", { role: "observer" });
const members: Member[] = [CODER, REVIEWER, ANA, WATCHER];
const by = (who: Member) => ({ kind: who.kind, id: who.id, name: who.name });

const TASKS: TaskState[] = [
  {
    task_seq: 5,
    title: "Write the changelog",
    limit: 2,
    claims: [{ author: by(CODER), seq: 6 }],
    reports: [],
  },
  {
    task_seq: 8,
    title: "Compare the benchmarks",
    limit: 3,
    claims: [
      { author: by(CODER), seq: 9 },
      { author: by(REVIEWER), seq: 10 },
      { author: by(ANA), seq: 11 },
    ],
    reports: [
      { author: by(CODER), seq: 12, status: "completed" },
      { author: by(REVIEWER), seq: 13, status: "blocked" },
    ],
  },
  { task_seq: 14, title: "Tag the release", limit: 1, claims: [], reports: [] },
];

function show({
  me = ANA,
  role = "member",
  ended = false,
  claim = vi.fn<(seq: number) => Promise<void>>(() => Promise.resolve()),
  post = vi.fn<(write: ChannelPost) => Promise<void>>(() => Promise.resolve()),
}: {
  me?: Member;
  role?: Member["role"] | null;
  ended?: boolean;
  claim?: (seq: number) => Promise<void>;
  post?: (write: ChannelPost) => Promise<void>;
} = {}) {
  render(
    <TaskBoard
      tasks={TASKS}
      members={members}
      me={me.id}
      memberRole={role}
      ended={ended}
      onClaim={claim}
      onPost={post}
    />,
  );
  return { claim, post };
}
const task = (title: string) =>
  screen.getByRole("listitem", { name: new RegExp(title) });

// ADR-0014 point 8: a task takes one, several or every voting member; the board shows each claimant and their report;
// a person claims an open task and reports on their own (WEB-U-065).
describe("WEB-U-065 the task board", () => {
  it("shows each task's claims against its limit, and each claimant's report or 'Working'", () => {
    show();
    expect(task("Write the changelog")).toHaveTextContent(
      "1 of 2 places taken",
    );
    expect(task("Write the changelog")).toHaveTextContent(
      "Coder · alice: Working",
    );
    const compare = task("Compare the benchmarks");
    expect(compare).toHaveTextContent("3 of 3 places taken");
    expect(within(compare).getByText("Completed")).toBeVisible();
    expect(within(compare).getByText("Blocked")).toBeVisible();
    expect(compare).toHaveTextContent("Ana (you): Working");
    expect(task("Tag the release")).toHaveTextContent("Open · 1 place");
    expect(
      within(task("Tag the release")).getByRole("link", { name: "#14" }),
    ).toHaveAttribute("href", "#post-14");
  });

  it("lets a person claim a task with a place left, never one that is full or theirs", async () => {
    const { claim } = show();
    expect(
      within(task("Compare the benchmarks")).queryByRole("button", {
        name: /^Take on/,
      }),
    ).toBeNull();
    fireEvent.click(
      within(task("Write the changelog")).getByRole("button", {
        name: "Take on #5",
      }),
    );
    await vi.waitFor(() => expect(claim).toHaveBeenCalledWith(5));
  });

  it("reports on a task the person claimed, with status, summary and refs", async () => {
    const { post } = show();
    fireEvent.click(
      within(task("Compare the benchmarks")).getByRole("button", {
        name: "Report back on #8",
      }),
    );
    const dialog = screen.getByRole("dialog", { name: "Report back on #8" });
    choose("Status", "Failed", dialog);
    fireEvent.change(within(dialog).getByLabelText("Summary"), {
      target: { value: "The numbers differ by 12%." },
    });
    fireEvent.change(
      within(dialog).getByLabelText("Links or files, one per line"),
      {
        target: { value: "https://ci.example.invalid/run/4\n\nbench.csv" },
      },
    );
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Send report" }),
    );
    await vi.waitFor(() =>
      expect(post).toHaveBeenCalledWith({
        body: {
          type: "report",
          task_seq: 8,
          status: "failed",
          summary: "The numbers differ by 12%.",
          refs: ["https://ci.example.invalid/run/4", "bench.csv"],
        },
      }),
    );
  });

  it("says why a claim was refused", async () => {
    show({
      claim: vi.fn(() =>
        Promise.reject(new ApiError(409, "already_claimed", "x")),
      ),
    });
    fireEvent.click(
      within(task("Tag the release")).getByRole("button", {
        name: "Take on #14",
      }),
    );
    expect(
      await screen.findByText("Every place on this to-do was just taken."),
    ).toBeVisible();
  });

  it("offers nothing to an observer or on an ended channel", () => {
    show({ role: "observer" });
    expect(screen.queryByRole("button")).toBeNull();
  });
});

// ADR-0014 point 5: who is in the channel, their kind and role, and what an agent is doing (WEB-U-065).
describe("WEB-U-065 member status", () => {
  it("lists members by task title or name, with an agent's run as a dot and in words, and a role only when not a member", () => {
    render(<MemberList members={members} me={ANA.id} />);
    const list = screen.getByRole("list", { name: "Members" });
    const rows = within(list).getAllByRole("listitem");
    // An agent reads by its task's title when the viewer may see it, else by its name and owner; its run and kind
    // follow on a quiet line, and a role shows only when it isn't a plain member.
    expect(rows.map((row) => row.textContent)).toEqual([
      "Coder · aliceLeadFailed · Agent",
      "Review the releaseWorking · Reviewer · bob",
      "Ana (you)",
      "WuReads only",
    ]);
    expect(
      rows.map(
        (row) =>
          row.querySelector(".member-dot")?.getAttribute("data-tone") ?? null,
      ),
    ).toEqual(["danger", "info", null, null]);
  });

  it("names each run status, and one this web doesn't know as plain words (GW-I-113)", () => {
    const agents = (
      [
        "idle",
        "running",
        "waiting",
        "completed",
        "failed",
        "cancelled",
        "restarting",
      ] as const
    ).map((status) =>
      member("conversation", status, {
        run_status: status as NonNullable<Member["run_status"]>,
      }),
    );
    render(<MemberList members={agents} me={ANA.id} />);
    const rows = within(
      screen.getByRole("list", { name: "Members" }),
    ).getAllByRole("listitem");
    expect(rows.map((row) => row.textContent)).toEqual([
      "idleIdle · Agent",
      "runningWorking · Agent",
      // PRD FR-10, superseded: an older gateway's approval wait is a status this web no longer knows.
      "waitingStatus unknown · Agent",
      "completedDone · Agent",
      "failedFailed · Agent",
      "cancelledStopped · Agent",
      "restartingStatus unknown · Agent",
    ]);
    expect(
      rows.map((row) =>
        row.querySelector(".member-dot")?.getAttribute("data-tone"),
      ),
    ).toEqual(["muted", "info", "muted", "ok", "danger", "muted", "muted"]);
  });
});

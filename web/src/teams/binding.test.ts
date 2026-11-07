import { describe, expect, it } from "vitest";

import "../protocol/generated/validators.teams";
import { parseApi } from "../protocol/validate";
import { helper, team, teamMessage } from "../test/teams";
import { placeLines } from "../transcript/TranscriptView";

import { endView, messageView, teamView } from "./binding";

// ADR-0034 S6: the gateway's team answers as the page shows them; the wire's spelling stays in the binding.
describe("WEB-U-221/222 the team's binding", () => {
  it("maps the team: state, budget, deadline, each helper and the open question", () => {
    const view = teamView(
      team({
        waiting_for_you: {
          seq: 14,
          text: "Which quarter?",
          at: "2026-10-02T20:30:00Z",
        },
        helpers: [
          helper(1, "researcher", {
            last_message: {
              seq: 3,
              type: "report",
              text: "North: 4.1 M",
              at: "2026-10-02T20:20:00Z",
            },
          }),
          helper(2, "writer", { state: "queued", position: 2 }),
        ],
      }),
    );
    expect(view).toEqual({
      state: "running",
      stopReason: null,
      budget: { tokens: 2_000_000, spent: 412_000 },
      deadlineAt: "2026-10-02T22:00:00Z",
      helpers: [
        {
          conversation: helper(1, "researcher").conversation,
          name: "researcher",
          state: "working",
          position: undefined,
          workspace: "own_copy",
          lastMessage: {
            text: "North: 4.1 M",
            type: "report",
            at: "2026-10-02T20:20:00Z",
          },
          // What it is doing now and its share, for All helpers (WEB-U-221).
          activity: { tool: "shell", since: "2026-10-02T20:01:00Z" },
          tokens: 120_400,
          tokensEstimated: false,
          canStop: true,
        },
        {
          conversation: helper(2, "writer").conversation,
          name: "writer",
          state: "queued",
          position: 2,
          workspace: "own_copy",
          lastMessage: null,
          activity: { tool: "shell", since: "2026-10-02T20:01:00Z" },
          tokens: 120_400,
          tokensEstimated: false,
          canStop: true,
        },
      ],
      waitingForYou: { seq: 14, text: "Which quarter?" },
    });
  });

  it("says you for the reader's own answer, someone for another's, and counts a report's files", () => {
    const me = "0195d8c0-0000-7000-8000-000000000001";
    const mine = messageView(
      teamMessage(5, "person_answer", {
        from: { kind: "person" },
        to: { kind: "lead" },
        by: me,
      }),
      me,
    );
    expect(mine.from).toEqual({ kind: "person", you: true });
    const theirs = messageView(
      teamMessage(6, "person_answer", {
        from: { kind: "person" },
        to: { kind: "lead" },
        by: "someone-else",
      }),
      me,
    );
    expect(theirs.from).toEqual({ kind: "person", you: false });
    const report = messageView(
      teamMessage(7, "report", {
        from: { kind: "member", name: "researcher" },
        to: { kind: "lead" },
        files: [{ name: "q3.csv", size: 10, sha256: "a".repeat(64) }],
      }),
      me,
    );
    expect(report).toMatchObject({
      from: { kind: "member", name: "researcher" },
      files: 1,
    });
    const notice = messageView(
      teamMessage(8, "notice", {
        from: { kind: "gateway" },
        to: { kind: "lead" },
        kind: "member_queued",
        member: "writer",
        position: 2,
      }),
      me,
    );
    expect(notice.notice).toEqual({
      kind: "member_queued",
      member: "writer",
      position: 2,
    });
  });

  it("reads the end note's parts, keeping only files handed back by a report", () => {
    const note = endView(
      teamMessage(20, "ended", {
        from: { kind: "gateway" },
        to: { kind: "lead" },
        reason: "stopped",
        tokens: {
          total: 412_000,
          members: [{ name: "researcher", tokens: 120_400 }],
        },
        reports: [{ member: "researcher", seq: 3 }],
        files: [{ member: "researcher", seq: 3, name: "q3.csv", size: 2048 }],
      }),
    );
    expect(note).toEqual({
      reason: "stopped",
      // Absent unless the lead said it was done (the slice 4 body).
      summary: null,
      tokens: {
        total: 412_000,
        members: [{ name: "researcher", tokens: 120_400 }],
      },
      reports: [{ member: "researcher", seq: 3 }],
      files: [{ member: "researcher", seq: 3, name: "q3.csv", size: 2048 }],
    });
  });
});

// WEB-U-222: the team's log sits among the lead's turns in time order.
describe("WEB-U-222 team lines among the turns", () => {
  const line = (at: string) => ({ key: at, at, node: at });
  it("puts each line before the first turn that started after it, and the rest at the end", () => {
    const lines = [
      line("2026-10-02T20:00:00Z"),
      line("2026-10-02T20:05:00Z"),
      line("2026-10-02T20:20:00Z"),
    ];
    const placed = placeLines(
      ["2026-10-02T20:01:00Z", "2026-10-02T20:10:00Z"],
      lines,
    );
    expect(placed.before.map((each) => each.map((l) => l.at))).toEqual([
      ["2026-10-02T20:00:00Z"],
      ["2026-10-02T20:05:00Z"],
    ]);
    expect(placed.rest.map((l) => l.at)).toEqual(["2026-10-02T20:20:00Z"]);
  });

  it("lets a turn without a run yet take nothing, so earlier lines wait for the next timed turn", () => {
    const placed = placeLines(
      [null, "2026-10-02T20:10:00Z"],
      [line("2026-10-02T20:05:00Z")],
    );
    expect(placed.before).toEqual([[], [line("2026-10-02T20:05:00Z")]]);
    expect(placed.rest).toEqual([]);
  });
});

// The run where the lead's page lost its whole log: the gateway addresses the lead's question to its person
// ({kind: "person"}, tqa-types ToKind), and a page of the log with it must parse, with every addressee the gateway writes.
describe("WEB-U-222 a page of the team's log as the gateway sends it", () => {
  it("accepts every addressee the gateway writes, its person's included", () => {
    const page = {
      items: [
        teamMessage(1, "assign", { to: { kind: "member", name: "analyst" } }),
        teamMessage(2, "question", { to: { kind: "members" } }),
        teamMessage(3, "answer", {
          from: { kind: "member", name: "analyst" },
          to: { kind: "lead" },
          question_seq: 2,
        }),
        teamMessage(4, "done", { to: { kind: "team" } }),
        { ...teamMessage(5, "person_question"), to: { kind: "person" } },
      ],
      last_seq: 5,
      more: false,
    };
    expect(
      parseApi(page, "TeamMessagePage").items.map((m) => m.to.kind),
    ).toEqual(["member", "members", "lead", "team", "person"]);
  });
});

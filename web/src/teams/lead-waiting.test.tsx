import { readFileSync } from "node:fs";
import { join } from "node:path";

import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { catalogue, english, translator } from "../i18n/translate";
import type { TeamMessage } from "../protocol/generated/api";
import { frame, runId } from "../test/frames";
import { apply, emptyTranscript } from "../transcript/reducer";
import { RunStatus } from "../transcript/RunStatus";

import { leadReminders, leadWaiting, teamWaitWords } from "./lead-waiting";

const message = (
  seq: number,
  from: TeamMessage["from"]["kind"],
  type: TeamMessage["type"],
  kind?: string,
): TeamMessage =>
  ({
    seq,
    type,
    from: { kind: from },
    to: { kind: "lead" },
    text: "",
    at: "2026-10-17T12:00:00Z",
    ...(kind ? { kind } : {}),
  }) as unknown as TeamMessage;

// WEB-U-398 (c16k real-world B2, 17 Oct 2026): the lead ended its turn without done, and its header said Done while
// the team ran on. While a lead's run is over and its team still runs, the header says the team waits for the lead;
// after two unanswered reminders (GW-I-306) it says the lead stopped without finishing, with what to do.
describe("WEB-U-398 a lead whose turn ended while its team runs isn't Done", () => {
  it("counts the gateway's reminders since the lead's own last post", () => {
    expect(
      leadReminders([
        message(1, "lead", "assign"),
        message(2, "gateway", "notice", "lead_unfinished"),
        message(3, "member", "report"),
        message(4, "gateway", "notice", "lead_unfinished"),
      ]),
    ).toBe(2);
    expect(
      leadReminders([
        message(1, "gateway", "notice", "lead_unfinished"),
        message(2, "lead", "assign"),
        message(3, "gateway", "notice", "lead_unfinished"),
      ]),
    ).toBe(1);
    expect(leadReminders([message(1, "gateway", "notice", "budget_80")])).toBe(
      0,
    );
  });

  it("words the header for a lead whose team runs, whatever its run: the header shows them once the run is over (WEB-U-415)", () => {
    const waiting = (over: Partial<Parameters<typeof leadWaiting>[0]>) =>
      leadWaiting(
        { lead: true, state: "running", reminders: 0, ...over },
        english,
      );
    expect(waiting({})).toBe("Waiting for the lead to finish the team");
    expect(waiting({ reminders: 1 })).toBe(
      "Waiting for the lead to finish the team",
    );
    expect(waiting({ reminders: 2 })).toBe(
      "The lead stopped without finishing the team. Stop the team, or send a message.",
    );
    expect(waiting({ state: "ended" })).toBeNull();
    expect(waiting({ lead: false })).toBeNull();
  });

  it("shows it in the header in Done's place, and never over work", () => {
    const done = [
      frame(1, { type: "run.started", run_id: runId }),
      frame(2, { type: "run.completed", run_id: runId }),
    ].reduce(apply, emptyTranscript());
    const { rerender } = render(
      <RunStatus
        transcript={done}
        leadWaiting="Waiting for the lead to finish the team"
      />,
    );
    expect(screen.getByLabelText("Run status")).toHaveTextContent(
      "Waiting for the lead to finish the team",
    );
    expect(screen.getByLabelText("Run status")).not.toHaveTextContent("Done");
    const working = apply(
      emptyTranscript(),
      frame(1, { type: "run.started", run_id: runId }),
    );
    rerender(
      <RunStatus
        transcript={working}
        leadWaiting="Waiting for the lead to finish the team"
      />,
    );
    expect(screen.getByLabelText("Run status")).toHaveTextContent(/^Working…/);
  });
});

// WEB-U-410 (c17 U5): the team's words for a wait for the workspace, a turn on the lead's files, as a helper and a lead.
describe("WEB-U-410 a team's words for a wait for the workspace", () => {
  const place = (role: "lead" | "member", state: "running" | "ended") => ({
    role,
    id: "team",
    state,
  });
  it("says a helper's turn and a lead's wait for a helper, only while the team runs", () => {
    expect(teamWaitWords(place("member", "running"), english)).toBe(
      "Its turn on the lead's files comes when the one using them finishes. You can reply once it starts.",
    );
    expect(teamWaitWords(place("lead", "running"), english)).toBe(
      "A helper is using the files; this task carries on when the helper's turn ends. You can reply once it starts.",
    );
    expect(teamWaitWords(place("member", "ended"), english)).toBeNull();
    expect(teamWaitWords(null, english)).toBeNull();
  });
});

// WEB-U-419 (c17): "the one using them" left the person to guess which task holds the
// lead's files. A helper names it, by the title the files' lease gives; with no title, the general words stay.
describe("WEB-U-419 a waiting helper names the task using the lead's files", () => {
  const member = {
    role: "member" as const,
    id: "team",
    state: "running" as const,
  };
  it("names the task in English and Chinese", () => {
    expect(teamWaitWords(member, english, "Draft the launch notes")).toBe(
      "Its turn on the lead's files comes when “Draft the launch notes” is done with them. You can reply once it starts.",
    );
    const zh = translator(
      "zh-Hans",
      catalogue(
        JSON.parse(
          readFileSync(join("src", "i18n", "locales", "zh-Hans.json"), "utf8"),
        ),
      ),
    );
    expect(teamWaitWords(member, zh, "起草发布说明")).toBe(
      "“起草发布说明”用完主任务的文件后，就轮到它。开始后即可回复。",
    );
  });
  // WEB-U-419: a title can be 200 characters; it is clipped to 60, its ellipsis inside the quotes.
  it("clips a long title to 60 characters", () => {
    const long = `${"Plan the quarter's launch for the new region ".repeat(4)}end`;
    expect(teamWaitWords(member, english, long)).toBe(
      `Its turn on the lead's files comes when “${Array.from(long).slice(0, 59).join("").trimEnd()}…” is done with them. You can reply once it starts.`,
    );
    const exact = "x".repeat(60);
    expect(teamWaitWords(member, english, exact)).toContain(`“${exact}”`);
    const zh = "规划新区域本季度的发布".repeat(8);
    expect(teamWaitWords(member, english, zh)).toContain(
      `“${Array.from(zh).slice(0, 59).join("")}…”`,
    );
  });
  it("keeps the general words with no title, and a lead's words whatever the holder", () => {
    expect(teamWaitWords(member, english, null)).toBe(
      "Its turn on the lead's files comes when the one using them finishes. You can reply once it starts.",
    );
    expect(
      teamWaitWords({ ...member, role: "lead" }, english, "helper-a"),
    ).toBe(
      "A helper is using the files; this task carries on when the helper's turn ends. You can reply once it starts.",
    );
  });
});

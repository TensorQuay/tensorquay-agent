import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { frame } from "../test/frames";

import { apply, emptyTranscript } from "./reducer";
import { TranscriptView } from "./TranscriptView";

const start = (content: string, profile: string | null = "researcher") =>
  [
    frame(1, { type: "step.started", step: 1, attempt: 1 }),
    frame(2, {
      type: "tool.call",
      call_id: "call-start",
      name: "start_member",
      arguments: {
        role: "DuckDB researcher",
        brief: "Research DuckDB.",
        ...(profile ? { profile } : {}),
      },
    }),
    frame(3, { type: "tool.start", call_id: "call-start" }),
    frame(4, { type: "tool.end", call_id: "call-start", status: "error" }),
    frame(5, {
      type: "tool.result",
      call_id: "call-start",
      status: "error",
      content,
    }),
  ].reduce(apply, emptyTranscript());

// WEB-U-399 (a review and real-world use before 17.0, 17 Oct 2026): "Started a helper: DuckDB researcher · Failed" over
// the runtime's line "unknown_reference: unknown reference: profile". A helper that couldn't start says so, and why in
// words under its step; the runtime's line stays folded in the step's Result.
describe("WEB-U-399 a helper that couldn't start, in words", () => {
  it("names the agent profile that doesn't exist", () => {
    const { container } = render(
      <TranscriptView
        transcript={start("unknown_reference: unknown reference: profile")}
      />,
    );
    expect(
      screen.getByText("Couldn't start a helper: DuckDB researcher"),
    ).toBeVisible();
    expect(
      screen.getByText("There's no agent profile named “researcher”."),
    ).toHaveClass("activity-next");
    expect(container.textContent).not.toContain("unknown_reference");
  });

  it("says when the team already has all the helpers it may", () => {
    render(
      <TranscriptView
        transcript={start(
          "member_limit: the agent team already has 3 helpers, the most allowed by max_members; a helper that is done still counts until you stop it with stop_member, which frees its seat",
          null,
        )}
      />,
    );
    expect(
      screen.getByText(
        "The team already has as many helpers as it may. A finished helper counts until it's stopped.",
      ),
    ).toHaveClass("activity-next");
  });

  it("says when the team has ended", () => {
    render(
      <TranscriptView
        transcript={start(
          "team_ended: the agent team has ended; its team tools no longer work",
          null,
        )}
      />,
    );
    expect(screen.getByText("The team had already ended.")).toHaveClass(
      "activity-next",
    );
  });

  it("adds nothing for a code this page doesn't know, beyond the step's own line", () => {
    const { container } = render(
      <TranscriptView transcript={start("brand_new_code: something", null)} />,
    );
    expect(container.querySelector(".activity-next")).toBeNull();
    expect(
      screen.getByText("Couldn't start a helper: DuckDB researcher"),
    ).toBeVisible();
  });
});

// WEB-U-497 (18.1, ADR-0034's rounds amendment A6): a task that may not lead is answered in words by its runtime
// (`teams_unavailable`), and its page says so once, muted: "Can't lead helpers here", with the reason in the page's
// words, never "Couldn't start a helper". A real failure still reads as one.
describe("WEB-U-497 a team tool refused in words reads as such", () => {
  it.each([
    [
      "Helpers are switched off for this installation. Do the work yourself, and say in your answer that you worked without helpers.",
      "Helpers are switched off for this installation.",
    ],
    [
      "This task was a helper, and a helper can't lead a team. Do the work yourself, and say so.",
      "A task that was a helper can't lead a team.",
    ],
    [
      "This environment can't lead helpers (its runtime is too old). Do the work yourself, and say so; the person can start the task in a newer environment.",
      "This task's environment is too old to lead helpers.",
    ],
    [
      "This team has ended for good (its lead task was deleted or its person's account was turned off). Do the work yourself, and say so.",
      "Its team has ended for good.",
    ],
    [
      "Your team's round has ended. Read its end note with read_reports, or start the next round with start_member; the other team tools come back once a round runs.",
      "Its team's round has ended.",
    ],
  ])("%s", (sentence, words) => {
    const { container } = render(
      <TranscriptView
        transcript={start(`teams_unavailable: ${sentence}`, null)}
      />,
    );
    expect(screen.getByText("Can't lead helpers here")).toBeVisible();
    expect(screen.getByText(words)).toHaveClass("activity-next");
    expect(container.textContent).not.toContain("Couldn't start a helper");
    expect(container.textContent).not.toContain("teams_unavailable");
  });

  it("keeps a real failure's words", () => {
    render(
      <TranscriptView
        transcript={start(
          "member_limit: the agent team already has 3 helpers, the most allowed by max_members",
          null,
        )}
      />,
    );
    expect(
      screen.getByText("Couldn't start a helper: DuckDB researcher"),
    ).toBeVisible();
    expect(screen.queryByText("Can't lead helpers here")).toBeNull();
  });
});

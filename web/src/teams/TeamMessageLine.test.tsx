import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { english } from "../i18n/translate";

import type { TeamMessageView } from "./team-view";
import { TeamMessageLine, teamMessageWords } from "./TeamMessageLine";

const lead = { kind: "lead" as const };
const member = (name: string) => ({ kind: "member" as const, name });
const message = (
  type: TeamMessageView["type"],
  from: TeamMessageView["from"],
  to: TeamMessageView["to"],
  text: string,
  extra: Partial<TeamMessageView> = {},
): TeamMessageView => ({ seq: 1, type, from, to, text, ...extra });

// WEB-U-222 (UI-GUIDELINES §13b): the team's messages in the lead task's transcript, one quiet line each, the full
// text in place on a click.
describe("WEB-U-222 team messages as quiet lines", () => {
  it.each([
    [
      message("assign", lead, member("Researcher"), "Find the Q3 revenue."),
      "To Researcher: Find the Q3 revenue.",
    ],
    [
      message("question", lead, { kind: "members" }, "Which format?"),
      "Asked all helpers: Which format?",
    ],
    [
      message("answer", member("Researcher"), lead, "Table."),
      "Researcher answered: Table.",
    ],
    [
      message("report", member("Researcher"), lead, "North: 4.1 M", {
        files: 2,
      }),
      "Researcher reported: North: 4.1 M · 2 files",
    ],
    [
      message("report", member("Researcher"), lead, "Done.", { files: 1 }),
      "Researcher reported: Done. · 1 file",
    ],
    [
      message("ask", member("Reviewer"), lead, "Which quarter?"),
      "Reviewer asked: Which quarter?",
    ],
    [
      message("decision", lead, { kind: "members" }, "Use format Y."),
      "Decided: Use format Y.",
    ],
    [
      message("person_question", lead, { kind: "person" }, "Which quarter?"),
      "Asked you: Which quarter?",
    ],
    [
      message(
        "person_answer",
        { kind: "person", you: true },
        lead,
        "July to September.",
      ),
      "You answered: July to September.",
    ],
    [
      message(
        "person_message",
        { kind: "person", you: true },
        lead,
        "Use the new figures.",
        { member: "Reviewer" },
      ),
      "You told Reviewer: Use the new figures.",
    ],
    [
      message("done", lead, { kind: "members" }, "All merged."),
      "Finished: All merged.",
    ],
  ])("words a %s", (value, words) => {
    render(<TeamMessageLine message={value} />);
    expect(screen.getByRole("button")).toHaveTextContent(words);
  });

  it.each([
    [
      { kind: "member_idle", member: "Reviewer" },
      "Reviewer stopped without reporting.",
    ],
    [{ kind: "member_stuck", member: "Reviewer" }, "Reviewer seems stuck."],
    [{ kind: "member_failed", member: "Reviewer" }, "Reviewer failed."],
    [{ kind: "member_stopped", member: "Reviewer" }, "Reviewer was stopped."],
    [{ kind: "member_deleted" }, "A helper was deleted."],
    // `position` counts from 1 (ADR-0034 S6): third in line has two ahead; first is next.
    [
      { kind: "member_queued", member: "Reviewer", position: 3 },
      "Reviewer is waiting for a slot (2 ahead).",
    ],
    [
      { kind: "member_queued", member: "Reviewer", position: 1 },
      "Reviewer is waiting for a slot (next).",
    ],
    [{ kind: "budget_80" }, "The team has used 80 % of its budget."],
    [{ kind: "deadline_near" }, "The team's time is nearly up."],
    [{ kind: "something_new" }, "The team has news for the lead."],
  ])("words a notice of kind %j", (notice, words) => {
    render(
      <TeamMessageLine
        message={message("notice", { kind: "gateway" }, lead, "", { notice })}
      />,
    );
    expect(screen.getByRole("button")).toHaveTextContent(words);
  });

  it("tints a decision, and opens a long line in full in place", () => {
    const long = "A".repeat(400);
    render(
      <TeamMessageLine
        message={message("decision", lead, { kind: "members" }, long)}
      />,
    );
    const toggle = screen.getByRole("button", { name: /^Decided:/ });
    const line = toggle.parentElement;
    expect(line).toHaveAttribute("data-tone", "info");
    expect(toggle).toHaveAttribute("aria-expanded", "false");
    fireEvent.click(toggle);
    expect(toggle).toHaveAttribute("aria-expanded", "true");
    expect(line).toHaveTextContent(long);
  });

  it("shows a message of a type it doesn't know by its text alone, never nothing", () => {
    render(
      <TeamMessageLine
        message={message(
          "brand_new" as TeamMessageView["type"],
          lead,
          lead,
          "Something.",
        )}
      />,
    );
    expect(screen.getByRole("button")).toHaveTextContent("Something.");
  });
});

// WEB-U-507 (18.1, ADR-0034's rounds amendment A5): the gateway's round_started reads as one quiet line, the round
// named, never the stored text's words.
describe("WEB-U-507 a round's start as a quiet line", () => {
  it("says Round 2 started", () => {
    const words = teamMessageWords(
      message(
        "round_started",
        { kind: "gateway" },
        { kind: "person" },
        "Round 2 of this team started.",
        { round: 2 },
      ),
      english,
    );
    expect(words).toBe("Round 2 started");
  });
});

// WEB-U-512 (18.2, the first hour's finding 6): a helper's failed run names its reason in plain words and links to
// the helper's task; while that helper works again, the line says so instead of "failed".
describe("WEB-U-512 a helper's failed run on the lead's page", () => {
  const failed = message(
    "notice",
    { kind: "gateway" },
    lead,
    "places failed: the model's reply was cut off.",
    { notice: { kind: "member_failed", member: "places", code: "reply_cut" } },
  );
  const helper = (state: string) => [
    { name: "places", conversation: "c-places", state },
  ];

  it("names the reason in plain words and links the line to the helper's task", () => {
    render(<TeamMessageLine message={failed} helpers={helper("failed")} />);
    const link = screen.getByRole("link", {
      name: "places failed: The model's reply was cut off at its output limit.",
    });
    expect(link).toHaveAttribute("href", "/c/c-places");
  });

  it("says the helper works again, never failed, while it does", () => {
    render(<TeamMessageLine message={failed} helpers={helper("working")} />);
    const link = screen.getByRole("link");
    expect(link).toHaveTextContent(
      "places stopped: The model's reply was cut off at its output limit. It's working again.",
    );
    expect(link.textContent).not.toMatch(/failed/);
  });

  it("keeps the short words for a code it doesn't know, and no link without the helper", () => {
    const unknown = {
      ...failed,
      notice: {
        kind: "member_failed",
        member: "places",
        code: "something_new",
      },
    };
    render(<TeamMessageLine message={unknown} />);
    expect(screen.getByText("places failed.")).toBeVisible();
    expect(screen.queryByRole("link")).toBeNull();
  });
});

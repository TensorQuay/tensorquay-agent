import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import type { TeamMessageView } from "./team-view";
import { TeamMessageLine } from "./TeamMessageLine";

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

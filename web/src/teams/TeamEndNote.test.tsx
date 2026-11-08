import { readFileSync } from "node:fs";
import { join } from "node:path";

import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { english } from "../i18n/translate";

import type { TeamEndView, TeamMessageView } from "./team-view";
import { TeamEndNote } from "./TeamEndNote";
import { teamMessageWords } from "./TeamMessageLine";

const note = (extra: Partial<TeamEndView> = {}): TeamEndView => ({
  reason: "done",
  summary: "Combined the three regions into results.md.",
  tokens: {
    total: 1_250_000,
    members: [
      { name: "Researcher", tokens: 600_000 },
      { name: "Reviewer", tokens: 150_000 },
    ],
  },
  reports: [
    { member: "Researcher", seq: 9 },
    { member: "Researcher", seq: 14 },
    { member: "Reviewer", seq: 12 },
  ],
  files: [{ member: "Researcher", seq: 14, name: "north.csv", size: 2048 }],
  ...extra,
});

// WEB-U-222 (ADR-0034 point 10, the S6 contract's `ended`): when a team stops, the lead task's transcript says why,
// what each helper used, their reports and the files they handed back.
describe("WEB-U-222 the team's end note", () => {
  it("says why the team stopped, what it used, its reports and the files handed back", async () => {
    render(<TeamEndNote note={note()} />);
    const card = screen.getByRole("region", { name: "The team finished." });
    // The summary is formatted once the Markdown renderer has loaded (WEB-U-396).
    expect(
      await screen.findByText("Combined the three regions into results.md."),
    ).toBeVisible();
    expect(card).toHaveTextContent(
      // Shares of the team's use, never the word tokens (PRD §12 item 11): the lead task's is what the helpers didn't use.
      "Share of the work: Researcher 48 %, Reviewer 12 %, the lead task 40 %",
    );
    // Each helper's count in words, by the language's plural rule.
    expect(card).toHaveTextContent(
      "2 reports from Researcher and 1 report from Reviewer",
    );
    expect(card).toHaveTextContent("Handed back: north.csv (2 kB, Researcher)");
  });

  it.each([
    ["stopped", "You stopped the team."],
    ["budget", "The team ran out of budget before it finished."],
    ["deadline", "The team ran out of time."],
    ["idle", "The team stopped after a while with nothing happening."],
    [
      "person_disabled",
      "The team stopped: its person's account was turned off.",
    ],
    ["lead_deleted", "The team stopped."],
    ["something_new", "The team stopped."],
  ])("says a stop for %s in words", (reason, words) => {
    render(
      <TeamEndNote
        note={note({ reason, summary: null, reports: [], files: [] })}
      />,
    );
    const card = screen.getByRole("region", { name: words });
    expect(card).not.toHaveTextContent(/reports? from/);
    expect(card).not.toHaveTextContent("Handed back:");
  });
});

// WEB-U-375 (the lead's live session on 16e, L48/L49): the handed-back files are joined in the reader's language, "、"
// in Chinese, never the comma each file's own parentheses already use ("…（401 B，coder）, test_primes.py…" before).
describe("WEB-U-375 the handed-back list's separator", () => {
  it("joins files with the catalogue's separator", () => {
    render(
      <TeamEndNote
        note={note({
          files: [
            { member: "coder", seq: 14, name: "primes.py", size: 401 },
            { member: "coder", seq: 14, name: "test_primes.py", size: 380 },
          ],
        })}
      />,
    );
    expect(
      screen.getByText(
        /^Handed back: primes\.py \(401 B, coder\), test_primes\.py \(380 B, coder\)$/,
      ),
    ).toBeInTheDocument();
  });

  it("is 、 in Chinese, never the comma inside a file's parentheses", () => {
    const zh = (
      JSON.parse(
        readFileSync(join("src", "i18n", "locales", "zh-Hans.json"), "utf8"),
      ) as {
        messages: Record<string, string>;
      }
    ).messages;
    expect(zh["team.endFilesSeparator"]).toBe("、");
    expect(zh["team.endFile"]).toContain("，");
  });
});

// WEB-U-396 (a review and real-world use before 17.0, 17 Oct 2026): the end card showed the lead's summary with its
// Markdown marks ("**Day-by-day plan**", "**Cost estimate table**"). It reads as the lead's reply does: formatted,
// never raw marks.
describe("WEB-U-396 the end note's summary is formatted", () => {
  it("renders the lead's Markdown, with no raw marks", async () => {
    render(
      <TeamEndNote
        note={note({
          summary:
            "Delivered trip.md:\n\n- **Day-by-day plan**\n- **Cost estimate table**",
        })}
      />,
    );
    const card = screen.getByRole("region", { name: "The team finished." });
    expect(
      await screen.findByText("Day-by-day plan", { selector: "strong" }),
    ).toBeVisible();
    expect(card).toHaveTextContent("Cost estimate table");
    expect(card.textContent).not.toContain("**");
    expect(screen.getAllByRole("listitem")).toHaveLength(2);
  });
});

// WEB-U-394 (real-world use before 17.0, 17 Oct 2026): a team that ran out of budget said only
// "The team used its whole budget." under a green Done, with no word that the goal wasn't met or what to do. Its end
// note says so and gives the next step; the team's own lines say what the 80 % and helpers' notices asked.
describe("WEB-U-394 a budget end and the budget notices say what happens next", () => {
  it("gives a budget end its next step", () => {
    render(<TeamEndNote note={note({ reason: "budget", summary: null })} />);
    const card = screen.getByRole("region", {
      name: "The team ran out of budget before it finished.",
    });
    expect(card).toHaveTextContent(
      "Send a message to carry on with what it handed back, or raise Tokens per team.",
    );
  });

  it("gives no other end that line", () => {
    render(<TeamEndNote note={note()} />);
    expect(screen.queryByText(/raise Tokens per team/)).not.toBeInTheDocument();
  });

  it("says what the budget notices asked", () => {
    const line = (kind: string) =>
      teamMessageWords(
        {
          seq: 1,
          type: "notice",
          from: { kind: "gateway" },
          to: { kind: "lead" },
          text: "",
          notice: { kind },
        } as unknown as TeamMessageView,
        english,
      );
    expect(line("budget_80")).toBe(
      "The team has used 80 % of its budget. Each helper was asked to report what it has.",
    );
    expect(line("budget_helpers_stopped")).toBe(
      "The helpers stop at their next step: the rest of the budget is the lead's, to write up.",
    );
  });
});

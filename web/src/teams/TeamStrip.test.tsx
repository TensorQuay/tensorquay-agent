import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import type { HelperView, TeamView } from "./team-view";
import { TeamStrip } from "./TeamStrip";

const helper = (name: string, state: HelperView["state"], extra = {}) => ({
  conversation: `task-${name}`,
  name,
  state,
  ...extra,
});
const team = (helpers: HelperView[]): TeamView => ({
  state: "running",
  budget: { tokens: 2_000_000, spent: 400_000 },
  deadlineAt: "2026-10-02T11:12:00Z",
  helpers,
  waitingForYou: null,
});
const NOW = Date.parse("2026-10-02T10:00:00Z");

// WEB-U-221 (UI-GUIDELINES §13b): one quiet line under the lead's bar: a chip per helper, then the budget; on a phone,
// one line that opens All helpers.
describe("WEB-U-221 the team strip", () => {
  it("shows a chip per helper with its state, and the budget with its exact figures", () => {
    render(
      <TeamStrip
        team={team([
          helper("Researcher", "working"),
          helper("Reviewer", "stuck"),
        ])}
        now={NOW}
        onAll={vi.fn()}
      />,
    );
    const strip = screen.getByRole("region", { name: "Helpers" });
    const researcher = within(strip).getByRole("link", {
      name: "Researcher: Working…",
    });
    expect(researcher).toHaveAttribute("href", expect.stringMatching(/^\/c\//));
    expect(
      within(strip).getByRole("link", { name: "Reviewer: Stuck" }),
    ).toHaveAttribute("data-tone", "danger");
    const budget = within(strip).getByText(
      "Budget 20 % used · 1 h 12 min left",
    );
    // The exact figures: the share and the end's clock time, never tokens (§13b).
    expect(budget.closest("[title]")).toHaveAttribute(
      "title",
      expect.stringMatching(/^20 % of the team's budget used · Ends at /),
    );
    expect(strip).not.toHaveTextContent(/token/i);
    expect(
      within(strip).getByRole("meter", { name: "Budget used" }),
    ).toHaveAttribute("aria-valuenow", "20");
  });

  // The review of 7 Oct (§13b): with four helpers or fewer the sheet was out of reach on a wide screen.
  it("opens All helpers from its own button, whatever the count", () => {
    const onAll = vi.fn();
    render(
      <TeamStrip
        team={team([helper("Researcher", "working")])}
        now={NOW}
        onAll={onAll}
      />,
    );
    const strip = screen.getByRole("region", { name: "Helpers" });
    expect(within(strip).queryByRole("button", { name: /more\)$/ })).toBeNull();
    fireEvent.click(within(strip).getByRole("button", { name: "All helpers" }));
    expect(onAll).toHaveBeenCalledOnce();
  });

  it("ends the row with +N over four helpers, which opens All helpers", () => {
    const onAll = vi.fn();
    render(
      <TeamStrip
        team={team(
          ["One", "Two", "Three", "Four", "Five", "Six"].map((name) =>
            helper(name, "working"),
          ),
        )}
        now={NOW}
        onAll={onAll}
      />,
    );
    const strip = screen.getByRole("region", { name: "Helpers" });
    expect(within(strip).getAllByRole("link")).toHaveLength(4);
    fireEvent.click(
      within(strip).getByRole("button", { name: "All helpers (2 more)" }),
    );
    expect(onAll).toHaveBeenCalledOnce();
  });

  it("has a phone line that says the count and the budget, and opens All helpers", () => {
    const onAll = vi.fn();
    render(
      <TeamStrip
        team={team([
          helper("A", "working"),
          helper("B", "done"),
          helper("C", "queued", { position: 1 }),
        ])}
        now={NOW}
        onAll={onAll}
      />,
    );
    const narrow = screen.getByRole("button", {
      name: "3 helpers · Budget 20 % used",
    });
    expect(narrow.closest(".helpers-strip-narrow")).not.toBeNull();
    fireEvent.click(narrow);
    expect(onAll).toHaveBeenCalledOnce();
  });

  it("says a state it can't read as State unknown, never a blank", () => {
    render(
      <TeamStrip
        team={team([helper("Odd", "unknown")])}
        now={NOW}
        onAll={vi.fn()}
      />,
    );
    expect(
      screen.getByRole("link", { name: "Odd: State unknown" }),
    ).toBeInTheDocument();
  });
});

// WEB-U-310: an ended team's strip keeps its chips (each helper's page stays one click away) but has no time and no meter.
describe("WEB-U-310 an ended team's strip", () => {
  it("says how the team ended in place of the budget and the time left, on a wide screen and a phone", () => {
    render(
      <TeamStrip
        team={{
          ...team([helper("Researcher", "done"), helper("Writer", "stopped")]),
          state: "ended",
          stopReason: "done",
        }}
        now={NOW}
        onAll={vi.fn()}
      />,
    );
    const strip = screen.getByRole("region", { name: "Helpers" });
    expect(
      within(strip).getByText("Finished · 20 % of the budget used"),
    ).toBeVisible();
    expect(within(strip).queryByRole("meter")).toBeNull();
    expect(strip).not.toHaveTextContent(/left|Ends at/);
    expect(within(strip).getAllByRole("link")).toHaveLength(2);
    expect(strip).toHaveTextContent(
      "2 helpers · Finished · 20 % of the budget used",
    );
  });
});

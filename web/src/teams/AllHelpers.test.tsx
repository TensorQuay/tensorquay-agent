import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { AllHelpers } from "./AllHelpers";
import type { HelperView } from "./team-view";

const push = vi.fn();
vi.mock("next/navigation", () => ({
  useRouter: () => ({ push, replace: vi.fn() }),
}));

const helpers: HelperView[] = [
  {
    conversation: "task-researcher",
    name: "Researcher",
    state: "working",
    lastMessage: { text: "North: 4.1 M, from q3-north.pdf page 2" },
    canStop: true,
  },
  {
    conversation: "task-reviewer",
    name: "Reviewer",
    state: "stuck",
    lastMessage: null,
    canStop: true,
  },
];

// WEB-U-221 (UI-GUIDELINES §13b): All helpers is a side sheet: one row per helper, its state in words, its last
// message in one muted line, Open, and ⋯ with Stop helper. No forms in the sheet.
describe("WEB-U-221 All helpers", () => {
  it("lists each helper with its state and last message, in a sheet at the end", () => {
    render(<AllHelpers helpers={helpers} onClose={vi.fn()} onStop={vi.fn()} />);
    const sheet = screen.getByRole("dialog", { name: "All helpers" });
    expect(sheet).toHaveAttribute("data-placement", "end");
    const rows = within(sheet).getAllByRole("listitem");
    expect(rows).toHaveLength(2);
    expect(rows[0]).toHaveTextContent("Researcher");
    expect(rows[0]).toHaveTextContent("Working…");
    expect(rows[0]).toHaveTextContent("North: 4.1 M, from q3-north.pdf page 2");
    expect(rows[1]).toHaveTextContent("Stuck");
    expect(within(sheet).queryByRole("textbox")).toBeNull();
  });

  // The team's exact figures, which the strip keeps in a tooltip, are here for a keyboard or a
  // touch screen too.
  it("says the team's exact figures under its title when given them", () => {
    render(
      <AllHelpers
        helpers={helpers}
        budget="20 % of the team's budget used · Ends at 11:12"
        onClose={vi.fn()}
        onStop={vi.fn()}
      />,
    );
    const sheet = screen.getByRole("dialog", { name: "All helpers" });
    expect(
      within(sheet).getByText("20 % of the team's budget used · Ends at 11:12"),
    ).toBeVisible();
  });

  it("opens a helper's page from its Open, and asks to stop one from its ⋯", () => {
    const onStop = vi.fn();
    const onClose = vi.fn();
    render(<AllHelpers helpers={helpers} onClose={onClose} onStop={onStop} />);
    fireEvent.click(
      screen.getByRole("button", { name: "More actions for Reviewer" }),
    );
    fireEvent.click(
      screen.getByRole("menuitem", { name: "Stop helper Reviewer" }),
    );
    expect(onStop).toHaveBeenCalledWith(helpers[1]);
    fireEvent.click(screen.getByRole("button", { name: "Open Researcher" }));
    expect(push).toHaveBeenCalledWith("/c/task-researcher");
    expect(onClose).toHaveBeenCalled();
  });

  // WEB-U-308 (GW-U-168): Stop is offered where the gateway's can_stop says it would stop the helper, not by state. A
  // done helper and one whose run failed both hold their place until stopped (ADR-0034 member states, GW-I-260); one
  // stopped, or failed before it ever ran, has nothing to stop.
  it("offers Stop where can_stop says so: a done helper and one whose run failed, not one stopped or never started", () => {
    render(
      <AllHelpers
        helpers={[
          {
            conversation: "task-done",
            name: "Done one",
            state: "done",
            canStop: true,
          },
          {
            conversation: "task-ran",
            name: "Run failed",
            state: "failed",
            canStop: true,
          },
          {
            conversation: "task-stopped",
            name: "Stopped one",
            state: "stopped",
            canStop: false,
          },
          {
            conversation: "task-failed",
            name: "Never started",
            state: "failed",
            canStop: false,
          },
        ]}
        onClose={vi.fn()}
        onStop={vi.fn()}
      />,
    );
    for (const name of ["Done one", "Run failed"]) {
      fireEvent.click(
        screen.getByRole("button", { name: `More actions for ${name}` }),
      );
      expect(
        screen.getByRole("menuitem", { name: `Stop helper ${name}` }),
      ).toBeVisible();
      fireEvent.keyDown(document.activeElement ?? document.body, {
        key: "Escape",
      });
    }
    // Nothing to stop, so no ⋯ at all; Open stays.
    for (const name of ["Stopped one", "Never started"]) {
      expect(
        screen.queryByRole("button", { name: `More actions for ${name}` }),
      ).toBeNull();
      expect(
        screen.getByRole("button", { name: `Open ${name}` }),
      ).toBeVisible();
    }
  });
});

// WEB-U-221: each row says what the helper is doing: its current step in words and for how
// long, its last message's kind and when, and its share of the team's work so far; never a tool's name or "tokens".
describe("WEB-U-221 what each helper is doing", () => {
  const now = Date.parse("2026-10-05T10:00:00Z");
  const rows = (list: HelperView[], spent = 0) => {
    render(
      <AllHelpers
        helpers={list}
        now={now}
        spent={spent}
        onClose={vi.fn()}
        onStop={vi.fn()}
      />,
    );
    return within(
      screen.getByRole("dialog", { name: "All helpers" }),
    ).getAllByRole("listitem");
  };

  it("says a working helper's step in words, and for how long", () => {
    const [shell, thinking, other] = rows([
      {
        conversation: "a",
        name: "Researcher",
        state: "working",
        activity: { tool: "shell", since: "2026-10-05T09:57:00Z" },
      },
      {
        conversation: "b",
        name: "Writer",
        state: "working",
        activity: { tool: null, since: "2026-10-05T09:59:40Z" },
      },
      {
        conversation: "c",
        name: "Reviewer",
        state: "working",
        activity: {
          tool: "connector__docs__search",
          since: "2026-10-05T09:50:00Z",
        },
      },
    ]);
    expect(shell).toHaveTextContent("Running a command for 3 minutes");
    expect(thinking).toHaveTextContent("Thinking");
    expect(thinking).not.toHaveTextContent("for 0");
    expect(other).toHaveTextContent("Using a tool for 10 minutes");
    expect(other).not.toHaveTextContent("connector__docs__search");
  });

  it("names the last message's kind and when, and the helper's share of the team's work", () => {
    const [row] = rows(
      [
        {
          conversation: "a",
          name: "Researcher",
          state: "done",
          activity: { tool: "shell", since: "2026-10-05T09:00:00Z" },
          lastMessage: {
            text: "North: 4.1 M",
            type: "report",
            at: "2026-10-05T09:48:00Z",
          },
          tokens: 300,
        },
      ],
      1200,
    );
    // A done helper isn't doing anything now, whatever its last step was.
    expect(row).not.toHaveTextContent("Running a command");
    expect(row).toHaveTextContent(/Reported · /);
    expect(row).toHaveTextContent("North: 4.1 M");
    expect(row).toHaveTextContent("25 % of the team's work so far");
    expect(row).not.toHaveTextContent(/tokens/);
  });

  // WEB-U-337: part of a helper's use is the gateway's estimate (a step stopped before its server reported it).
  it("WEB-U-337 says a helper's share is about that much when part of it is estimated", () => {
    const [row] = rows(
      [
        {
          conversation: "a",
          name: "Researcher",
          state: "stopped",
          tokens: 300,
          tokensEstimated: true,
        },
      ],
      1200,
    );
    expect(row).toHaveTextContent(
      "About 25 % of the team's work so far; part of it is estimated",
    );
    expect(row).not.toHaveTextContent(/tokens/);
  });

  it("leaves out the share before the team has used anything", () => {
    const [row] = rows([
      { conversation: "a", name: "Researcher", state: "queued", tokens: 0 },
    ]);
    expect(row).not.toHaveTextContent(/team's work/);
  });
});

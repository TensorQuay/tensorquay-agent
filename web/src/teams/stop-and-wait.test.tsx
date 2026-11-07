import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { ApiError } from "../api/error";
import { teamMessage } from "../test/teams";

import { StopHelperDialog, StopTeamDialog } from "./StopDialogs";
import { teamLines } from "./TeamOnPage";
import { WaitingCard } from "./WaitingCard";

// WEB-U-224 (UI-GUIDELINES §13b): when the team needs its person, the lead's transcript holds the question in a warn
// card that points to the message box, says Answered once it is, and No longer needed once the team no longer waits.
describe("WEB-U-224 the Waiting for you card", () => {
  it("shows the lead's question and where to answer", () => {
    render(
      <WaitingCard
        question="Which quarter counts as Q3 for you?"
        state="open"
      />,
    );
    const card = screen.getByRole("region", { name: "Waiting for you" });
    expect(card).toHaveAttribute("data-tone", "warn");
    expect(card).toHaveTextContent("Which quarter counts as Q3 for you?");
    expect(card).toHaveTextContent("Answer in the message box below.");
  });

  it("says Answered quietly once it is", () => {
    render(<WaitingCard question="Which quarter?" state="answered" />);
    const card = screen.getByRole("region", { name: "Answered" });
    expect(card).not.toHaveAttribute("data-tone");
    expect(card).not.toHaveTextContent("Answer in the message box below.");
    expect(card).toHaveTextContent("Which quarter?");
  });

  it("says No longer needed quietly once the team stopped waiting", () => {
    render(<WaitingCard question="Which quarter?" state="closed" />);
    const card = screen.getByRole("region", { name: "No longer needed" });
    expect(card).not.toHaveAttribute("data-tone");
    expect(card).not.toHaveTextContent("Answer in the message box below.");
    expect(card).toHaveTextContent("Which quarter?");
  });
});

// WEB-U-411 (c17 U6; c16k W6-09): after Stop team a question to the person read a bare "No longer needed", which
// said neither what was dropped nor why. The card says why: the person stopped the team, the team ended or is
// stopping, or the lead asked something else.
describe("WEB-U-411 a closed question says why it is no longer needed", () => {
  const question = teamMessage(4, "person_question", {
    to: { kind: "person" },
    text: "Team is forming: storage helper started.",
  });
  const show = (extra: Parameters<typeof teamMessage>[2]) => {
    const lines = teamLines(
      [question, teamMessage(9, "ended", extra)],
      null,
      null,
    );
    render(
      <>
        {lines.map((line) => (
          <div key={line.key}>{line.node}</div>
        ))}
      </>,
    );
  };
  it("says the person stopped the team", () => {
    show({ reason: "stopped", tokens: { total: 0, members: [] } });
    const card = screen.getByRole("region", {
      name: "No longer needed: you stopped the team",
    });
    expect(card).toHaveTextContent("Team is forming: storage helper started.");
  });
  it("says the team ended, for its other ends", () => {
    show({ reason: "budget", tokens: { total: 0, members: [] } });
    expect(
      screen.getByRole("region", {
        name: "No longer needed: the team has ended",
      }),
    ).toBeVisible();
  });
});

// WEB-U-225 (UI-GUIDELINES §13b): Stop team and Stop helper ask first, and say what stays.
describe("WEB-U-225 stopping a team or a helper", () => {
  it("asks before stopping the team, naming how many helpers stop", () => {
    const onConfirm = vi.fn();
    render(
      <StopTeamDialog
        helpers={3}
        pending={false}
        error={null}
        onConfirm={onConfirm}
        onClose={vi.fn()}
      />,
    );
    const dialog = screen.getByRole("dialog", { name: "Stop the team?" });
    expect(dialog).toHaveTextContent(
      "The lead and 3 helpers stop now. Their tasks and files stay.",
    );
    expect(
      within(dialog).getByRole("button", { name: "Cancel" }),
    ).toBeVisible();
    fireEvent.click(within(dialog).getByRole("button", { name: "Stop team" }));
    expect(onConfirm).toHaveBeenCalledOnce();
  });

  it("asks before stopping one helper, and says the lead is told not to redo its part", () => {
    render(
      <StopHelperDialog
        name="Reviewer"
        pending={false}
        error={null}
        onConfirm={vi.fn()}
        onClose={vi.fn()}
      />,
    );
    const dialog = screen.getByRole("dialog", { name: "Stop Reviewer?" });
    expect(dialog).toHaveTextContent(
      "Reviewer stops now. Its task and files stay. The lead is told not to redo its part.",
    );
    expect(
      within(dialog).getByRole("button", { name: "Stop helper" }),
    ).toBeVisible();
  });

  it("says a team that had already stopped in words", () => {
    render(
      <StopTeamDialog
        helpers={1}
        pending={false}
        error={new ApiError(409, "team_stopped", "x")}
        onConfirm={vi.fn()}
        onClose={vi.fn()}
      />,
    );
    expect(screen.getByRole("dialog")).toHaveTextContent(
      "The team had already stopped.",
    );
  });
});

// WEB-U-391 (c16k real-world B6b): Stop team stops the lead too, and the dialog says so, with or without helpers.
describe("WEB-U-391 the Stop team dialog names the lead", () => {
  it.each([
    [0, "The lead stops now. Their tasks and files stay."],
    [1, "The lead and 1 helper stop now. Their tasks and files stay."],
    [2, "The lead and 2 helpers stop now. Their tasks and files stay."],
  ])("with %i helpers", (helpers, text) => {
    render(
      <StopTeamDialog
        helpers={helpers}
        pending={false}
        error={null}
        onConfirm={vi.fn()}
        onClose={vi.fn()}
      />,
    );
    expect(
      screen.getByRole("dialog", { name: "Stop the team?" }),
    ).toHaveTextContent(text);
  });
});

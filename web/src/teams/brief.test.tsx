import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { frame } from "../test/frames";
import { apply, emptyTranscript } from "../transcript/reducer";
import { TranscriptView } from "../transcript/TranscriptView";

const FRAME =
  "You are the researcher of an agent team. You report to your lead with send_to_lead.";
const brief = (extra = {}) =>
  frame(1, {
    type: "message.user",
    text: FRAME,
    attachments: [],
    lead_brief: {
      role: "researcher",
      workspace: "own_copy",
      brief: {
        objective: "Find the Q3 revenue by region in the attached reports.",
        output: "A table: region, revenue, source file and page.",
        sources: "Only the files in reports/; no web.",
        boundaries: "Don't edit the reports; write only results.md.",
        ...extra,
      },
    },
  });

// WEB-U-223 (ADR-0034 point 3, UI-GUIDELINES §13b): a helper's first item is its brief from the lead, as a card,
// never the person's words, and never the gateway's frame.
describe("WEB-U-223 the Brief from the lead", () => {
  // The card loads with its own code (only a helper's first turn has one, WEB-U-014), so each test waits for it.
  it("keeps the brief with the first turn, and shows it as a card, never as the person's message", async () => {
    const state = [brief()].reduce(apply, emptyTranscript());
    expect(state.items[0]).toMatchObject({
      kind: "user",
      brief: {
        role: "researcher",
        objective: "Find the Q3 revenue by region in the attached reports.",
      },
    });
    render(<TranscriptView transcript={state} />);
    const card = await screen.findByRole("region", {
      name: "Brief from the lead",
    });
    expect(card).toHaveTextContent(
      "Find the Q3 revenue by region in the attached reports.",
    );
    for (const [label, text] of [
      ["Output", "A table: region, revenue, source file and page."],
      ["Sources", "Only the files in reports/; no web."],
      ["Boundaries", "Don't edit the reports; write only results.md."],
    ] as const)
      expect(within(card).getByText(label).nextSibling).toHaveTextContent(text);
    expect(screen.queryByRole("article", { name: "Your message" })).toBeNull();
    expect(document.body).not.toHaveTextContent("send_to_lead");
  });

  it("leaves Sources out when the lead gave none", async () => {
    render(
      <TranscriptView
        transcript={[brief({ sources: null })].reduce(apply, emptyTranscript())}
      />,
    );
    const card = await screen.findByRole("region", {
      name: "Brief from the lead",
    });
    expect(within(card).queryByText("Sources")).toBeNull();
  });

  it("folds a brief over six lines, with Show the whole brief", async () => {
    render(
      <TranscriptView
        transcript={[
          brief({
            objective: ["One.", "Two.", "Three.", "Four.", "Five."].join("\n"),
          }),
        ].reduce(apply, emptyTranscript())}
      />,
    );
    const card = await screen.findByRole("region", {
      name: "Brief from the lead",
    });
    expect(card).toHaveAttribute("data-folded");
    fireEvent.click(
      within(card).getByRole("button", { name: "Show the whole brief" }),
    );
    expect(card).not.toHaveAttribute("data-folded");
    expect(
      within(card).queryByRole("button", { name: "Show the whole brief" }),
    ).toBeNull();
  });

  it("doesn't fold a short brief", async () => {
    render(
      <TranscriptView
        transcript={[brief({ sources: null })].reduce(apply, emptyTranscript())}
      />,
    );
    expect(
      await screen.findByRole("region", { name: "Brief from the lead" }),
    ).not.toHaveAttribute("data-folded");
  });
});

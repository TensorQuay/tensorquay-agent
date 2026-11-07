import { render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { english } from "../i18n/translate";
import type { QueuedMessage } from "../protocol/generated/api";
import { Composer } from "../transcript/Composer";

import { heldForLead } from "./person-steer";
import { TeamMessageLine } from "./TeamMessageLine";

const queued = (id: string, text: string, email: string): QueuedMessage =>
  ({
    id,
    text,
    state: "held",
    attachments: [],
    author: { id: email, email },
    created_at: "2026-10-17T12:00:00Z",
  }) as unknown as QueuedMessage;

// WEB-U-402 (c16k real-world B5, 17 Oct 2026): a message typed to a leading lead said "Sends when the agent finishes",
// which for a lead is after its whole team. While its team runs and its run is active, the lead reads it at its next
// check of the team (GW-I-307), and the page says so; the team's log shows it as told to the lead.
describe("WEB-U-402 a message to a leading lead goes at its next check of the team", () => {
  it("words the held line only for a lead whose run is active while its team runs", () => {
    const line = (over: Partial<Parameters<typeof heldForLead>[0]>) =>
      heldForLead(
        { lead: true, state: "running", runOver: false, ...over },
        english,
      );
    expect(line({})?.("Add MySQL.", null)).toBe(
      "The lead gets this at its next check of the team: Add MySQL.",
    );
    expect(line({})?.("Add MySQL.", "colleague@example.invalid")).toBe(
      "From colleague@example.invalid, the lead gets this at its next check of the team: Add MySQL.",
    );
    expect(line({ runOver: true })).toBeNull();
    expect(line({ state: "ended" })).toBeNull();
    expect(line({ lead: false })).toBeNull();
  });

  it("shows it in the composer's held list, still cancellable", () => {
    render(
      <Composer
        run={{ id: "r-1", status: "running" } as never}
        sandbox="ready"
        sending={false}
        cancelling={false}
        onSend={vi.fn(() => Promise.resolve(true))}
        onStop={vi.fn(() => Promise.resolve())}
        held={{
          held: [
            queued("1", "Add MySQL.", "me@example.invalid"),
            queued("2", "Skip SQLite.", "colleague@example.invalid"),
          ],
          dropped: [],
          me: "me@example.invalid",
          canCancel: () => true,
          notice: null,
          line: heldForLead(
            { lead: true, state: "running", runOver: false },
            english,
          ),
          onCancel: vi.fn(),
          onSendAgain: vi.fn(),
        }}
      />,
    );
    const [mine, theirs] = within(
      screen.getByRole("list", { name: "Messages waiting for the agent" }),
    ).getAllByRole("listitem");
    expect(mine).toHaveTextContent(
      "The lead gets this at its next check of the team: Add MySQL.",
    );
    expect(theirs).toHaveTextContent(
      "From colleague@example.invalid, the lead gets this at its next check of the team: Skip SQLite.",
    );
    expect(
      within(mine as HTMLElement).getByRole("button", { name: "Cancel" }),
    ).toBeInTheDocument();
  });

  it("shows the team's copy as told to the lead", () => {
    render(
      <TeamMessageLine
        message={{
          seq: 7,
          type: "person_message",
          from: { kind: "person", you: true },
          to: { kind: "lead" },
          text: "Add MySQL.",
        }}
      />,
    );
    expect(screen.getByRole("button")).toHaveTextContent(
      "You told the lead: Add MySQL.",
    );
  });
});

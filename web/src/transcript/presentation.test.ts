import { describe, expect, it } from "vitest";

import { english } from "../i18n/translate";

import { numberLabel } from "./numbers";
import { composerReason } from "./presentation";

const idle = { id: null, status: "idle" } as const;

// The composer's reason follows the recorded state (WEB-U-002: starting and queued render; a sleeping environment shows
// nothing, UI-GUIDELINES §8).
describe("WEB-U-002 sandbox and composer states", () => {
  it("says why a message cannot be sent now, and nothing while one is being sent", () => {
    const running = { id: "run", status: "running" } as const;
    expect(composerReason(idle, "creating", false)).toBe("Starting…");
    expect(composerReason(idle, "queued", false)).toBe(
      "You can reply once it starts.",
    );
    expect(
      composerReason({ id: "run", status: "queued" }, "ready", false),
    ).toBe("You can reply once it starts.");
    // ADR-0035: nothing parks; an old environment reported as parked blocks nothing.
    expect(composerReason(idle, "parked", false)).toBe(null);
    expect(composerReason(running, "busy", false)).toBe(
      "The agent is working. Stop it to send a new message.",
    );
    expect(composerReason(idle, "ready", false)).toBeNull();
    expect(composerReason(running, "creating", true)).toBeNull();
  });

  // The 16d page pass (Z24): the header's "Waiting…" leaves out when a desktop frees, so the composer says it, with the
  // tasks ahead when there are any.
  it("says when a desktop frees while the task waits for one", () => {
    const frees =
      "Other tasks are using every desktop; one frees soon after another task finishes. You can reply once it starts.";
    expect(
      composerReason(idle, "queued", false, english, {
        ahead: 0,
        reason: "desktop_sessions",
      }),
    ).toBe(frees);
    expect(
      composerReason(idle, "queued", false, english, {
        ahead: 2,
        reason: "desktop_sessions",
      }),
    ).toBe(`2 ahead of this task. ${frees}`);
    expect(
      composerReason(idle, "queued", false, english, {
        ahead: 1,
        reason: "slot",
      }),
    ).toBe("You can reply once it starts.");
  });

  it("groups thousands with a narrow no-break space, as UI-GUIDELINES §10 asks", () => {
    expect(numberLabel(12345)).toBe("12 345");
    expect(numberLabel(999)).toBe("999");
  });
});

// WEB-U-410 (c17 U5; c16k W5-04, 16u live L3): a helper waiting for its turn on the lead's files read only "Waiting…";
// the reason was in the header's tooltip alone, which a phone can't hover, and the body said nothing. As for a desktop
// wait (Z24), the composer says what it waits for: in a team, in the team's words (its lazily loaded layer's).
describe("WEB-U-410 a workspace wait says its reason in the composer", () => {
  const queue = { ahead: 0, reason: "workspace" } as const;
  it("says the team's words in a team", () => {
    expect(
      composerReason(idle, "queued", false, english, queue, "Its turn comes."),
    ).toBe("Its turn comes.");
  });
  it("says only when it can reply while the team's words load", () => {
    expect(composerReason(idle, "queued", false, english, queue, null)).toBe(
      "You can reply once it starts.",
    );
  });
  it("says another task holds the workspace outside a team", () => {
    expect(composerReason(idle, "queued", false, english, queue)).toBe(
      "Another task is using this workspace; this one starts when that one finishes. You can reply once it starts.",
    );
  });
});

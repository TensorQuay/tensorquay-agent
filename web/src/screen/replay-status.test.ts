import { describe, expect, it } from "vitest";

import { english } from "../i18n/translate";

import { statusKey } from "./ScreenPanel";

const watching = { kind: "watching" as const, takeOverFailed: false };

// WEB-U-366 (the 16e first hour, v03): with Replay showing a past step while the desktop is live, the status line still
// said "Watching." It says what the panel shows, and that Live has the desktop as it is now; switched back to Live, the
// usual words.
describe("WEB-U-366 the Screen panel's status line during Replay", () => {
  it("says the panel shows past steps, with the agent working or not", () => {
    expect(english(statusKey(watching, true, false, false, true))).toBe(
      "Showing past steps. The agent is still working: Live shows the desktop now.",
    );
    expect(english(statusKey(watching, false, false, false, true))).toBe(
      "Showing past steps. Live shows the desktop now.",
    );
  });

  it("keeps the live words in Live, and every other state's own words in Replay", () => {
    expect(english(statusKey(watching, true, false, false, false))).toBe(
      "Watching. The agent is working.",
    );
    expect(english(statusKey(watching, false, false, false))).toBe("Watching.");
    // In control, the person acts on the live desktop whatever the panel shows: that still comes first.
    expect(
      statusKey(
        { kind: "control", ready: true, resetOk: true },
        false,
        false,
        false,
        true,
      ),
    ).toBe("screen.control");
    expect(
      statusKey(
        { kind: "watching", takeOverFailed: true },
        false,
        false,
        false,
        true,
      ),
    ).toBe("screen.takeOverFailed");
  });
});

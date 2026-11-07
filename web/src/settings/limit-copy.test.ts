import { describe, expect, it } from "vitest";

import { english } from "../i18n/translate";

import { display, INPUT } from "./limit-copy";

// The helper limits' units (ADR-0034 S6): a count of helpers, a number of tokens, and minutes that read as the
// page's other durations do.
describe("WEB-U-227 the helper limits' units in words", () => {
  it("counts helpers and tokens as the gateway counts them", () => {
    expect(display(english, "helpers", 1)).toBe("1 helper");
    expect(display(english, "helpers", 3)).toBe("3 helpers");
    // Narrow no-break space thousands, as every number (UI-GUIDELINES §10).
    expect(display(english, "tokens", 2_000_000)).toBe(
      "2\u202f000\u202f000 tokens",
    );
  });

  it("reads minutes in whole hours when they are, and in minutes otherwise", () => {
    expect(display(english, "minutes", 120)).toBe("2 h");
    expect(display(english, "minutes", 90)).toBe("90 min");
    expect(display(english, "minutes", 10)).toBe("10 min");
  });

  it("edits each in its own whole unit", () => {
    expect(INPUT.helpers).toEqual({
      scale: 1,
      step: 1,
      label: "limits.inputHelpers",
    });
    expect(INPUT.tokens).toEqual({
      scale: 1,
      step: 1,
      label: "limits.inputTokens",
    });
    expect(INPUT.minutes).toEqual({
      scale: 1,
      step: 1,
      label: "limits.inputMinutes",
    });
  });
});

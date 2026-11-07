import { describe, expect, it } from "vitest";

import { initials } from "./initials";

// UI-GUIDELINES §7 account row: an avatar with initials derived from the account's email.
describe("WEB-U-007 account initials", () => {
  it.each([
    ["member@example.invalid", "ME"],
    ["ada.lovelace@example.invalid", "AL"],
    ["first_last-extra@example.invalid", "FL"],
    ["x@example.invalid", "X"],
    ["1234@example.invalid", "12"],
    ["", "?"],
  ])("shows %s as %s", (email, expected) => {
    expect(initials(email)).toBe(expected);
  });
});

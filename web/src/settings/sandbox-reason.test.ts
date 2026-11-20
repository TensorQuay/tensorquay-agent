import { describe, expect, it } from "vitest";

import { english } from "../i18n/translate";

import { reasonWords } from "./sandbox-reason";

// WEB-U-489: a removal's failure is said in plain words, never as its code (the real-stack run read
// "sandbox_failed" on Status and the page, 18 Nov 2026).
describe("WEB-U-489 a left-behind thing's reason in plain words", () => {
  it.each([
    ["timeout", "the engine timed out"],
    ["engine_unavailable", "the engine couldn't be reached"],
    ["store_unavailable", "the gateway couldn't read its records"],
    ["sandbox_failed", "the engine wouldn't remove it"],
    ["a_code_from_a_newer_gateway", "the engine wouldn't remove it"],
  ])("%s reads “%s”", (code, words) => {
    expect(reasonWords(code, english)).toBe(words);
  });

  it("says nothing for no reason", () => {
    expect(reasonWords(null, english)).toBe("");
  });
});

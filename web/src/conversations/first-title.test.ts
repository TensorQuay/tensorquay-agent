import { describe, expect, it } from "vitest";

import { firstTitle } from "./first-title";

describe("WEB-U-017 first-line titles", () => {
  it("marks truncation and keeps the complete title within sixty Unicode code points", () => {
    for (const source of [
      "word ".repeat(30),
      "界".repeat(100),
      "a".repeat(60) + " tail",
    ]) {
      const title = firstTitle(source);
      expect(title.endsWith("…")).toBe(true);
      expect(Array.from(title).length).toBeLessThanOrEqual(60);
      expect(title).not.toMatch(/\s…$/);
    }
    expect(firstTitle("A short title\nA second line")).toBe("A short title");
    expect(firstTitle("界".repeat(60))).toBe("界".repeat(60));
  });
});

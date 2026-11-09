// WEB-U-461: token counts read in their language's short form (2.8M, 282.5万, 2,8 M), never as a long row of digits.
import { describe, expect, it } from "vitest";

import { formatCompact } from "./message";

describe("WEB-U-461 short counts", () => {
  it("shortens thousands, millions and billions with one decimal at most", () => {
    expect(
      [0, 999, 1000, 2400, 66566, 999950, 2824643, 159000000, 1234567890].map(
        (value) => formatCompact(value, "en"),
      ),
    ).toEqual([
      "0",
      "999",
      "1K",
      "2.4K",
      "66.6K",
      "1M",
      "2.8M",
      "159M",
      "1.2B",
    ]);
  });

  it("follows each interface language's own way of grouping large numbers", () => {
    expect(formatCompact(2824643, "zh-Hans")).toBe("282.5万");
    expect(formatCompact(159000000, "ja")).toBe("1.6億");
    expect(formatCompact(2824643, "ko")).toBe("282.5만");
    expect(formatCompact(2824643, "fr")).toMatch(/^2,8\s?M$/u);
    expect(formatCompact(2824643, "es")).toMatch(/^2,8\s?M$/u);
    for (const language of ["vi", "th", "ms", "ta"])
      expect(formatCompact(2824643, language)).not.toMatch(/2[\s,.]?824/u);
  });
});

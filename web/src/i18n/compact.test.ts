// WEB-U-461: token counts read in their language's short form (2.8M, 282.5万, 2,8 M), never as a long row of digits.
import { describe, expect, it, vi } from "vitest";

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

  // A use against its limit rounds down (17.4 fix): 29 960 000 of 30 000 000 must not read "30M of 30M" while 40 000
  // tokens are left. Totals and limits keep the nearest.
  it("rounds a use down, so it never reads as its limit before it is reached", () => {
    expect(
      [29_960_000, 999_960, 4_249, 30_000_000].map((value) =>
        formatCompact(value, "en", "down"),
      ),
    ).toEqual(["29.9M", "999.9K", "4.2K", "30M"]);
    expect(formatCompact(29_960_000, "zh-Hans", "down")).toBe("2996万");
    expect(formatCompact(29_960_000, "en")).toBe("30M");
  });

  // The Usage tables shorten every cell: one formatter per language and rounding, not one per call.
  it("builds one formatter per language and rounding", () => {
    const Real = Intl.NumberFormat;
    // Counts each formatter built, and builds the real one (a function, so `new` works on the spy).
    const built = vi.spyOn(Intl, "NumberFormat").mockImplementation(function (
      ...args: ConstructorParameters<typeof Intl.NumberFormat>
    ) {
      return new Real(...args);
    });
    try {
      for (const value of [1234, 5678, 9012]) formatCompact(value, "en-GB");
      expect(built).toHaveBeenCalledTimes(1);
      formatCompact(1234, "en-GB", "down");
      expect(built).toHaveBeenCalledTimes(2);
    } finally {
      built.mockRestore();
    }
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

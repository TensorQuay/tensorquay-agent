import { readFileSync } from "node:fs";
import { join } from "node:path";

import { describe, expect, it } from "vitest";

import { catalogue, english, translator } from "../i18n/translate";

import { secondsLabel } from "./numbers";
import { durationLabel } from "./presentation";

const language = (code: string) =>
  translator(
    code,
    catalogue(
      JSON.parse(
        readFileSync(join("src", "i18n", "locales", `${code}.json`), "utf8"),
      ),
    ),
  );

// Review of 059: the run status, "Worked for" and tool durations use the reader's own units.
describe("WEB-U-019 duration labels", () => {
  it("rounds run and turn durations to whole seconds", () => {
    expect(secondsLabel(5_400, english)).toBe("5 s");
    // zh spaces the number from its unit, as its catalogue does (UI-GUIDELINES §11b, WEB-U-305); Intl alone attaches it.
    expect(secondsLabel(5_400, language("zh-Hans"))).toBe("5 秒");
    expect(secondsLabel(5_400, language("ja"))).toBe("5 秒");
    expect(secondsLabel(5_400, language("ko"))).toBe("5초");
    expect(secondsLabel(-20, english)).toBe("0 s");
  });

  it("shows tool durations in milliseconds below a second, else with two decimals", () => {
    expect(durationLabel(850, english)).toBe("850 ms");
    expect(durationLabel(850, language("zh-Hans"))).toBe("850 毫秒");
    expect(durationLabel(1_234, english)).toBe("1.23 s");
    expect(durationLabel(1_234, language("fr"))).toBe("1,23 s");
  });
});

// WEB-U-395 (a review and real-world use before 17.0, 17 Oct 2026):
// "Worked for 837 s · created 1 file, ran 27 commands". Past a minute a duration reads in minutes and seconds, past an
// hour in hours and minutes, in the reader's units.
describe("WEB-U-395 durations past a minute", () => {
  it("reads minutes and seconds under an hour, hours and minutes past it", () => {
    expect(secondsLabel(59_400, english)).toBe("59 s");
    expect(secondsLabel(60_000, english)).toBe("1 min");
    expect(secondsLabel(837_000, english)).toBe("13 min 57 s");
    expect(secondsLabel(3_599_600, english)).toBe("1 h");
    expect(secondsLabel(3_720_000, english)).toBe("1 h 2 min");
    expect(secondsLabel(7_200_000, english)).toBe("2 h");
  });

  it("uses the reader's own units", () => {
    expect(secondsLabel(837_000, language("zh-Hans"))).toBe("13 分钟 57 秒");
    expect(secondsLabel(3_720_000, language("zh-Hans"))).toBe("1 小时 2 分钟");
    expect(secondsLabel(837_000, language("ko"))).toBe("13분 57초");
  });
});

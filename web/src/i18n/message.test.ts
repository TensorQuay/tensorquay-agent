import { readdirSync, readFileSync } from "node:fs";

import { describe, expect, it } from "vitest";

import {
  formatDuration,
  formatMessage,
  formatMib,
  formatNumber,
  formatSize,
} from "./message";

// WEB-U-019: catalogue messages use the ICU subset the product needs - arguments, numbers, plurals and selects.
describe("WEB-U-019 message format", () => {
  it.each([
    ["Plain text", {}, "Plain text"],
    ["Switched to {template}", { template: "Minimal" }, "Switched to Minimal"],
    ["{count, number} input", { count: 218469 }, "218 469 input"],
    ["{count, plural, one {# file} other {# files}}", { count: 1 }, "1 file"],
    ["{count, plural, one {# file} other {# files}}", { count: 2 }, "2 files"],
    [
      "{count, plural, =0 {none} one {# file} other {# files}}",
      { count: 0 },
      "none",
    ],
    [
      "{count, plural, one {# match} other {# matches}}",
      { count: 1200 },
      "1 200 matches",
    ],
    [
      "{kind, select, created {Created} other {Updated}} {path}",
      { kind: "created", path: "a.py" },
      "Created a.py",
    ],
    [
      "{kind, select, created {Created} other {Updated}}",
      { kind: "edited" },
      "Updated",
    ],
    ["Say '{'braces'}' and it''s fine", {}, "Say {braces} and it's fine"],
  ])("formats %s", (pattern, values, expected) => {
    expect(formatMessage(pattern, values, "en")).toBe(expected);
  });

  it("uses the language's plural rules", () => {
    const pattern = "{count, plural, one {# fichier} other {# fichiers}}";
    expect(formatMessage(pattern, { count: 0 }, "fr")).toBe("0 fichier");
    expect(formatMessage(pattern, { count: 2 }, "fr")).toBe("2 fichiers");
    expect(
      formatMessage(
        "{count, plural, other {# 个文件}}",
        { count: 3 },
        "zh-Hans",
      ),
    ).toBe("3 个文件");
  });

  it("keeps a missing argument visible as its name and rejects malformed patterns", () => {
    expect(formatMessage("Hello {name}", {}, "en")).toBe("Hello {name}");
    expect(() => formatMessage("Unclosed {name", {}, "en")).toThrow();
    expect(() =>
      formatMessage("{count, plural, one {x}}", { count: 1 }, "en"),
    ).toThrow(/other/);
  });
});

// Review of 059: durations use each language's own unit (Intl), with narrow no-break space thousands as every number (§10).
// Review of the environment sizes (nit 7): a fractional count reads in the reader's language, in the plural `#` and
// in a plain number argument alike (1.5 is "1,5" in French).
describe("WEB-U-019 fractional counts", () => {
  it("formats half CPUs for the language, in a plural and in a plain argument", () => {
    const cpus = "{cpus, plural, one {# CPU} other {# CPUs}}";
    expect(formatMessage(cpus, { cpus: 1.5 }, "en")).toBe("1.5 CPUs");
    expect(formatMessage(cpus, { cpus: 1.5 }, "fr")).toBe("1,5 CPU");
  });

  // A plain `{cpus}` stays as given (ports and years must not be grouped), so every catalogue's CPU count uses the
  // plural `#`, which is formatted for the language.
  it("words every CPU count through the plural, in every catalogue", () => {
    for (const file of readdirSync("src/i18n/locales")) {
      const messages = (
        JSON.parse(readFileSync(`src/i18n/locales/${file}`, "utf8")) as {
          messages: Record<string, string>;
        }
      ).messages;
      for (const key of ["limits.cpus", "picker.cpus"])
        if (messages[key]) expect(messages[key], `${file} ${key}`).toMatch(/#/);
    }
  });
});

describe("WEB-U-019 durations", () => {
  it.each([
    ["en", 5, "second", "5 s"],
    ["en", 1234, "millisecond", "1\u202f234 ms"],
    // Spaced from the unit, as the zh catalogue writes numbers (UI-GUIDELINES §11b, WEB-U-305), where Intl attaches them.
    ["zh-Hans", 5, "second", "5 秒"],
    ["zh-Hans", 12, "millisecond", "12 毫秒"],
    ["ja", 5, "second", "5 秒"],
    ["ko", 5, "second", "5초"],
    ["fr", 1.25, "second", "1,25\u202fs"],
    ["es", 5, "second", "5 s"],
    ["vi", 5, "second", "5 giây"],
    ["th", 5, "second", "5 วิ"],
    ["ms", 5, "second", "5 saat"],
    ["ta", 5, "second", "5 விநாடி"],
    ["en", 12345, "second", "12\u202f345 s"],
  ] as const)("%s %d %s -> %s", (locale, value, unit, expected) => {
    expect(formatDuration(value, unit, locale)).toBe(expected);
  });

  it("keeps at most two decimals", () => {
    expect(formatDuration(1.2345, "second", "en")).toBe("1.23 s");
  });
});

// Task 068b: file sizes in binary steps with each language's own short unit; English and Japanese write "B".
describe("WEB-U-020 file sizes", () => {
  it.each([
    ["en", 0, "0 B"],
    ["en", 512, "512 B"],
    ["en", 1536, "1.5 kB"],
    ["en", 25 * 1024 * 1024, "25 MB"],
    ["en", 1024 ** 3, "1 GB"],
    ["en", 1023.9 * 1024, "1\u202f023.9 kB"],
    ["ja", 512, "512 B"],
    ["zh-Hans", 25 * 1024 * 1024, "25 MB"],
    ["fr", 1536, "1,5\u202fko"],
  ] as const)("%s %d -> %s", (locale, bytes, expected) => {
    expect(formatSize(bytes, locale)).toBe(expected);
  });
});

// The dev screen review (011e8972), #10: sizes were written several ways ("4 GiB" in the environment menu, "10 GB" on
// Workspaces). One formatter for every size; memory and disk are set in whole binary units, so what an administrator
// typed as 4 reads back as 4, never "4.3".
describe("WEB-U-185 a size reads back as it was set", () => {
  it.each([
    [4096, "4 GB"],
    [2048, "2 GB"],
    // The GB fields take quarters: what was typed reads back.
    [512, "0.5 GB"],
    [256, "0.25 GB"],
    [100, "100 MB"],
    [1536, "1.5 GB"],
    [10 * 1024, "10 GB"],
  ])("writes %s MiB as %s", (mib, expected) => {
    expect(formatMib(mib, "en")).toBe(expected);
  });

  it("never reads 4 GiB as 4.3", () => {
    expect(formatSize(4 * 1024 ** 3, "en")).toBe("4 GB");
    expect(formatMib(4096, "en")).not.toMatch(/4\.3/);
  });
});

// WEB-U-354: a grouped number never breaks across lines: thousands are grouped with U+202F NARROW NO-BREAK SPACE,
// which looks like a thin space but doesn't let a line wrap inside the number ("4 / 096" at 390, the 16f page pass).
describe("WEB-U-354 grouped numbers never break", () => {
  it.each([
    ["en", 4096, "4 096"],
    ["zh-Hans", 2000000, "2 000 000"],
    ["ja", 16384, "16 384"],
  ])("%s %d", (locale, value, words) => {
    expect(formatNumber(value, locale)).toBe(words);
  });

  it("puts no breakable space inside a formatted message's number", () => {
    const words = formatMessage("{n, number} processes", { n: 4096 }, "en");
    expect(words).toBe("4 096 processes");
    expect(words).not.toMatch(/\d[  ]\d/);
  });
});

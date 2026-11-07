import { describe, expect, it } from "vitest";

import en from "./locales/en.json";
import zh from "./locales/zh-Hans.json";

// WEB-U-312 (the c16 zh copy pass): the Simplified Chinese strings c16 added (the Desktop, its screen and steps, the
// helpers' and the Desktop's limits, the waits and Stop lines) say each thing with one term, keep the Limits page's
// "when it applies" hints in one form, and leave no English but names and the catalogue's own terms.

const messages = (catalogue: unknown): Record<string, string> => {
  const found = catalogue as { messages?: Record<string, string> };
  return found.messages ?? (catalogue as Record<string, string>);
};
const english = messages(en);
const chinese = messages(zh);

/** The c16 strings, by their keys' prefixes. */
const C16 = [
  "screen.",
  "desktop.",
  "tool.computer",
  "limits.applies",
  "limits.desktop",
  "limits.computer",
  "limits.screenshot",
  "composer.queuedSendHandBack",
  "composer.queuedSendByHandBack",
  "run.desktopSessions",
  "team.stillWorking",
  "team.stopLeadHint",
  "team.stripEnded",
  "profiles.toolComputer",
];
const c16 = Object.keys(english).filter((key) =>
  C16.some((prefix) => key.startsWith(prefix)),
);

/** A message's own words: its ICU syntax and placeholder names taken out. */
function words(text: string): string {
  return text
    .replace(
      /\{\s*[a-zA-Z_]+\s*,\s*(plural|select|selectordinal|number)\s*,?/g,
      "",
    )
    .replace(/(=?[0-9]+|[a-z_]+)\s*\{/g, "")
    .replace(/\{\s*[a-zA-Z_]+\s*\}/g, "")
    .replace(/[{}#]/g, "");
}

/** English that may stay: names, the key a person presses, and the catalogue's word for tokens. */
const KEPT = new Set(["Escape", "DuckDuckGo", "Google", "token"]);

/** One term each: the word c16 uses, and the alternatives that mustn't creep in. */
const TERMS: readonly [string, readonly string[]][] = [
  ["接管", ["接手", "接过控制"]],
  ["交还", ["归还", "交回"]],
  ["助手", ["帮手", "子代理", "子智能体"]],
  ["团队", ["队伍"]],
  ["桌面", ["台式"]],
];

describe("WEB-U-312 the c16 strings in Simplified Chinese", () => {
  it("has every c16 string, and leaves no English but names and kept terms", () => {
    expect(c16.length).toBeGreaterThan(100);
    for (const key of c16) {
      const text = chinese[key];
      expect(text, key).toBeTypeOf("string");
      const latin = words(text ?? "").match(/[A-Za-z]{2,}/g) ?? [];
      expect(
        latin.filter((word) => !KEPT.has(word)),
        key,
      ).toEqual([]);
    }
  });

  it("says each thing with one term", () => {
    for (const key of c16)
      for (const [, others] of TERMS)
        for (const other of others)
          expect(chinese[key], `${key} uses ${other}`).not.toContain(other);
  });

  it("keeps the Limits page's hints in one form: a short hint ends in 生效, the whole rule says 更改…生效", () => {
    const hints = c16.filter((key) => key.startsWith("limits.applies"));
    expect(hints.length).toBeGreaterThanOrEqual(10);
    for (const key of hints) {
      const text = chinese[key] ?? "";
      if (key.endsWith("Whole")) {
        expect(text, key).toMatch(/^更改.*生效/);
        // A team's budget is 预算 everywhere else on the page.
        expect(text, key).not.toContain("token");
      } else expect(text, key).toMatch(/生效$/);
    }
  });

  it("says how long the desktop was held when it is handed back, never a moment", () => {
    expect(chinese["screen.noticeHandedBack"]).toContain("接管了 {time}");
    expect(chinese["screen.noticeHandedBackClosed"]).toContain("接管了 {time}");
  });
});

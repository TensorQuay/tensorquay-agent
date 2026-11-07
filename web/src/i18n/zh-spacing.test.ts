import { readFileSync } from "node:fs";
import { join } from "node:path";

import { describe, expect, it } from "vitest";

import { display } from "../settings/limit-copy";

import { formatDuration } from "./message";
import { catalogue, translator } from "./translate";

const file = (code: string): unknown =>
  JSON.parse(
    readFileSync(join("src", "i18n", "locales", `${code}.json`), "utf8"),
  );
const messages = (code: string) =>
  (file(code) as { messages: Record<string, string> }).messages;
const en = messages("en");
const zh = messages("zh-Hans");
const chinese = translator("zh-Hans", catalogue(file("zh-Hans")));

const HAN = "[\\u3400-\\u9fff]";
const escape = (text: string) => text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");

/**
 * Whether a placeholder stands for a number: typed as one (`number`, `plural`) in either language, or followed in
 * English by a unit ("{seconds} s", "{n} min"). Any other placeholder stands for words ("与{against}相比").
 */
function numeric(key: string, name: string): boolean {
  const typed = new RegExp(`\\{${escape(name)}, *(number|plural)`);
  const unit = new RegExp(
    `\\{${escape(name)}\\} (s|ms|min|h|KB|MB|GB|seconds?|minutes?|hours?|days?)\\b`,
  );
  const english = en[key] ?? "";
  return typed.test(zh[key] ?? "") || typed.test(english) || unit.test(english);
}

/** Where a zh message writes a number against Chinese with no space (UI-GUIDELINES §11b); % and ° belong to it. */
function attachedNumbers(key: string, message: string): string[] {
  const found: string[] = [];
  for (const match of message.matchAll(
    new RegExp(`\\{(\\w+)(, *number[^{}]*)?\\}(?=${HAN})`, "g"),
  ))
    if (numeric(key, match[1] ?? "")) found.push(match[0]);
  // A plural's # is its number; a digit written in the text is one too.
  for (const match of message.matchAll(new RegExp(`#(?=${HAN})`, "g")))
    found.push(match[0]);
  const text = message.replace(/\{[^{}]*\}/g, " ");
  for (const match of text.matchAll(
    new RegExp(`[0-9](?=${HAN})|${HAN}(?=[0-9])`, "g"),
  ))
    found.push(match[0]);
  return found;
}

// UI-GUIDELINES §11b: in Simplified Chinese a half-width space separates Chinese from numbers, on both sides and before
// a unit ("{seconds} 秒后重试"), except % and °. The catalogue had 秒, 分钟, 小时 and 毫秒 attached in 11 strings and
// spaced in the rest; Intl attaches 秒 and 毫秒, so the duration formatter adds the space (UNIT_OVERRIDES).
describe("WEB-U-305 Simplified Chinese spaces numbers from the words around them", () => {
  it("writes no number attached to Chinese anywhere in the zh catalogue", () => {
    const attached = Object.entries(zh).flatMap(([key, message]) =>
      attachedNumbers(key, message).map((what) => `${key}: ${what}`),
    );
    expect(attached).toEqual([]);
  });

  it("finds a number placeholder, a plural's # and a written digit attached, but not words or %", () => {
    expect(attachedNumbers("limits.seconds", "{n}秒")).toEqual(["{n}"]);
    expect(attachedNumbers("models.milliseconds", "{n, number}毫秒")).toEqual([
      "{n, number}",
    ]);
    expect(attachedNumbers("x", "{count, plural, other {已重试 #次}}")).toEqual(
      ["#"],
    );
    expect(attachedNumbers("x", "过去7天")).toEqual(["去", "7"]);
    expect(attachedNumbers("usage.change", "与{against}相比 {change}")).toEqual(
      [],
    );
    expect(attachedNumbers("x", "已使用 {percent, number}%")).toEqual([]);
  });

  it.each([
    [5, "second", "5 秒"],
    [1.25, "second", "1.25 秒"],
    [12, "millisecond", "12 毫秒"],
    [1234, "millisecond", "1 234 毫秒"],
  ] as const)("formats %s %s as “%s”", (value, unit, shown) => {
    expect(formatDuration(value, unit, "zh-Hans")).toBe(shown);
  });

  it("spaces minutes, hours and a plural's count in the catalogue's own words", () => {
    expect(chinese("limits.minutes", { n: 5 })).toBe("5 分钟");
    expect(chinese("limits.hours", { n: 2 })).toBe("2 小时");
    expect(chinese("team.minutesLeft", { minutes: 12 })).toBe("还剩 12 分钟");
    expect(chinese("run.recoveryIn", { losses: 3, seconds: 30 })).toBe(
      "环境已停止 3 次；30 秒后重启",
    );
  });

  // The 16d page pass (Z28): only a number takes the space; a placeholder that ends in Chinese ("4 096 个进程") meets
  // the next Chinese word with none ("个进程和"), while one that starts with a number ("100 GB") keeps its own.
  it("joins the environment ceiling's words with no space between Chinese", () => {
    const sentence = chinese("limits.ceiling", {
      cpus: display(chinese, "millicores", 8000),
      memory: display(chinese, "bytes", 16 * 1024 ** 3),
      pids: display(chinese, "processes", 4096),
      disk: display(chinese, "bytes", 100 * 1024 ** 3),
    });
    expect(sentence).not.toMatch(new RegExp(`${HAN} ${HAN}`));
    expect(sentence).toContain("个进程和 100 GB 磁盘");
  });
});

// @vitest-environment node
import { describe, expect, it } from "vitest";

import zh from "./locales/zh-Hans.json";
import { formatDuration, formatMessage } from "./message";

const messages = zh.messages as Record<string, string>;

// A screen review before 17.0: Chinese wrote the same quantities two ways: "2秒" from the formatter beside
// "{seconds} 秒" in messages, and "运行了 1 条命令" in a turn's summary beside "运行了一条命令" on its line. One way is
// now spaced, the formatter included (UI-GUIDELINES §11b, WEB-U-305), as the catalogue writes every other number.
describe("WEB-U-185 Chinese counts and units read one way", () => {
  it("spaces a time unit from its number, as the formatter does", () => {
    expect(formatDuration(2, "second", "zh-Hans")).toBe("2 秒");
    // The formatter's units (seconds, milliseconds), and the Limits page's units beside them, as counts of days
    // ("最近 7 天").
    const attached = Object.entries(messages).filter(
      ([key, text]) =>
        /(\}|#)(秒|毫秒)/.test(text) ||
        (key.startsWith("limits.") && /\}(分钟|小时)/.test(text)),
    );
    expect(attached.map(([key]) => key)).toEqual([]);
  });

  it("words a count of one as a step's line does", () => {
    const say = (key: string, count: number) =>
      formatMessage(messages[key] ?? "", { count }, "zh-Hans");
    expect(say("summary.ran", 1)).toBe("运行了一条命令");
    expect(say("summary.ran", 3)).toBe("运行了 3 条命令");
    expect(say("summary.created", 1)).toBe("创建了一个文件");
    expect(say("summary.updated", 2)).toBe("更新了 2 个文件");
    expect(say("summary.files", 1)).toBe("已更改一个文件");
    expect(say("summary.commands", 1)).toBe("一条命令");
  });
});

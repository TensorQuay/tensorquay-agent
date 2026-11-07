import { describe, expect, it } from "vitest";

import { catalogue, negotiate, translator } from "./translate";

// WEB-U-019: a missing string falls back to English, never to a raw key; the browser language picks the default.
describe("WEB-U-019 translation", () => {
  it("translates from the chosen catalogue and falls back to English per key", () => {
    const t = translator(
      "zh-Hans",
      catalogue({
        language: "简体中文",
        messages: { "sidebar.collapse": "收起侧边栏" },
      }),
    );
    expect(t("sidebar.collapse")).toBe("收起侧边栏");
    expect(t("sidebar.expand")).toBe("Expand sidebar");
    expect(t.locale).toBe("zh-Hans");
  });

  it("uses English when a translated pattern is malformed", () => {
    const t = translator(
      "fr",
      catalogue({
        language: "Français",
        messages: { "summary.files": "{count, plural, one {x}" },
      }),
    );
    expect(t("summary.files", { count: 2 })).toBe("2 files changed");
  });

  it("accepts only string messages from a catalogue file", () => {
    expect(() => catalogue({ messages: {} })).toThrow();
    expect(
      catalogue({ language: "X", messages: { a: "b", c: 3 } }).messages,
    ).toEqual({ a: "b" });
  });

  it.each([
    [["zh-CN", "en"], "zh-Hans"],
    [["zh-SG"], "zh-Hans"],
    [["zh-TW"], "en"],
    [["en-GB"], "en"],
    [["fr-CA", "zh"], "zh-Hans"],
    [[], "en"],
  ])("negotiates %j to %s", (preferred, expected) => {
    expect(negotiate(preferred, ["en", "zh-Hans"])).toBe(expected);
  });
});

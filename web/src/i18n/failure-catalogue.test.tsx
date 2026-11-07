import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";

import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

const GENERATED = join("src", "i18n", "generated");
const messages = (file: string): Record<string, string> =>
  (
    JSON.parse(readFileSync(file, "utf8")) as {
      messages: Record<string, string>;
    }
  ).messages;
const reference = messages(join("src", "i18n", "locales", "en.json"));
const IO = reference["fail.io"];

// WEB-U-392 (16n P0-budget): the copy a run's failure shows (fail, retry, retryShort, quota and the server's refusals)
// loads on its first use, so the conversation page's first scripts stay within 245,760 B (WEB-U-014) with room for
// fixes. The browser's own refusals (no connection, a timeout) and the request errors stay core: they show when a
// download may not work.
describe("WEB-U-392 run-failure copy loads on its first use", () => {
  it("keeps the failure copy out of the core catalogue, except what shows when the network may be down", async () => {
    const { FAILURE_KEYS } = await import("./translate");
    expect(FAILURE_KEYS).toBeInstanceOf(RegExp);
    // The split (scripts/i18n-areas.mjs) and the loader use the same rule.
    const { AREAS } = (await import("../../scripts/i18n-areas.mjs")) as {
      AREAS: { name: string; keys?: RegExp }[];
    };
    expect(AREAS.find((area) => area.name === "failures")?.keys?.source).toBe(
      FAILURE_KEYS.source,
    );
    const core = Object.keys(messages(join(GENERATED, "en.core.json")));
    expect(core.filter((key) => FAILURE_KEYS.test(key))).toEqual([]);
    for (const key of [
      "refuse.network",
      "refuse.timeout",
      "refuse.checkArrivedNext",
      "refuse.unknown",
      "refuse.unknownNext",
      "error.network",
    ])
      expect(core).toContain(key);
    const file = join(GENERATED, "en.failures.json");
    expect(existsSync(file)).toBe(true);
    const failures = Object.keys(messages(file));
    expect(failures).toContain("fail.io");
    expect(failures).toContain("refuse.taskGone");
    expect(failures.filter((key) => !FAILURE_KEYS.test(key))).toEqual([]);
    // Every reference key the rule names is in the failures catalogue, so none is left to no catalogue.
    expect(
      Object.keys(reference).filter(
        (key) => FAILURE_KEYS.test(key) && !failures.includes(key),
      ),
    ).toEqual([]);
  });

  it("shows nothing for a failure key until its copy arrives, then the words; the copy downloads once", async () => {
    // A fresh translator module: the test setup's copy of every area isn't in its reference.
    vi.resetModules();
    const { english, onReference } = await import("./translate");
    const arrived = vi.fn();
    const stop = onReference(arrived);
    expect(english("fail.io")).toBe("");
    expect(english("fail.internal")).toBe("");
    // A core key is there at once, never empty.
    expect(english("refuse.network")).toBe(reference["refuse.network"]);
    await vi.waitFor(() => expect(arrived).toHaveBeenCalledOnce());
    expect(english("fail.io")).toBe(IO);
    expect(english("fail.internal")).toBe(reference["fail.internal"]);
    stop();
  });

  it("re-renders the page's words once the copy arrives", async () => {
    vi.resetModules();
    const { I18nProvider, useT } = await import("./I18nProvider");
    function Reason() {
      const t = useT();
      return <p data-testid="reason">{t("fail.io")}</p>;
    }
    render(
      <I18nProvider>
        <Reason />
      </I18nProvider>,
    );
    expect(screen.getByTestId("reason")).toHaveTextContent(/^$/);
    expect(await screen.findByText(IO ?? "")).toBeVisible();
  });
});

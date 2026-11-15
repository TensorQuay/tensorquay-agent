import { readFileSync } from "node:fs";
import { join } from "node:path";

import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { AREAS } from "../../scripts/i18n-areas.mjs";

// WEB-U-473 (18.0 bundle room, WEB-U-014's headroom): a task page's first scripts hold no turn renderer. The
// transcript view loads the turns' renderers (the presenter, the activity group and lines, the summaries) and their
// copy with the first turn; a task without one never downloads them.
const read = (path: string) => readFileSync(join(process.cwd(), path), "utf8");

describe("WEB-U-473 the turn renderers load with the first turn", () => {
  it("keeps the transcript view free of static renderer imports and loads TranscriptTurns lazily", () => {
    const view = read("src/transcript/TranscriptView.tsx");
    for (const renderer of [
      "ActivityGroup",
      "ActivityLine",
      "present",
      "RecordedSummary",
      "turn-summary",
      "TranscriptTurns",
    ])
      expect(view, renderer).not.toMatch(
        new RegExp(`^import .* from "\\./${renderer}"`, "m"),
      );
    // The view asks the once-only loader, which holds the one dynamic import.
    expect(view).toContain('from "./turns-loader"');
    expect(read("src/transcript/turns-loader.ts")).toContain(
      'import("./TranscriptTurns")',
    );
  });

  // A new task has no turn, so nothing has asked for the renderers; the person's intent to send does, before the
  // first turn arrives, and a task page shown and left alone asks for nothing.
  it.each([
    ["focus in the composer", "focus"],
    ["a press in the composer", "pointerDown"],
  ] as const)(
    "loads the renderers on %s, and not just because the page shows",
    async (_, intent) => {
      vi.resetModules();
      const loader = await import("./turns-loader");
      const { Composer } = await import("./Composer");
      render(
        <Composer
          run={{ id: null, status: "idle" }}
          sandbox="ready"
          sending={false}
          cancelling={false}
          onSend={() => Promise.resolve(true)}
          onStop={() => Promise.resolve()}
        />,
      );
      expect(loader.turnsRequested()).toBe(false);
      fireEvent[intent](screen.getByLabelText("Message", { exact: true }));
      expect(loader.turnsRequested()).toBe(true);
      await loader.preloadTurns();
      expect(loader.loadedTurns()).not.toBeNull();
    },
  );

  // A chunk the browser couldn't fetch (a tab open across a deploy): the loader forgets it, a warm-up leaves nothing
  // unhandled, and the next ask tries again and succeeds.
  it("forgets a failed load, leaving no unhandled rejection, and the next ask loads them", async () => {
    const unhandled: unknown[] = [];
    const listen = (reason: unknown) => unhandled.push(reason);
    process.on("unhandledRejection", listen);
    try {
      vi.resetModules();
      vi.doMock("./TranscriptTurns", () => {
        throw new Error("chunk failed");
      });
      const failing = await import("./turns-loader");
      // (vitest wraps a mock factory's throw in its own message.)
      await expect(failing.preloadTurns()).rejects.toThrow();
      expect(failing.turnsRequested()).toBe(false);
      expect(failing.loadedTurns()).toBeNull();
      failing.warmTurns();
      await vi.dynamicImportSettled();
      await new Promise((resolve) => setImmediate(resolve));
      expect(unhandled).toEqual([]);
      expect(failing.turnsRequested()).toBe(false);
      vi.doUnmock("./TranscriptTurns");
      vi.resetModules();
      const loader = await import("./turns-loader");
      await loader.preloadTurns();
      expect(loader.loadedTurns()).not.toBeNull();
    } finally {
      process.off("unhandledRejection", listen);
    }
  });

  // The step summaries (tool-summary, with the Desktop tool's words) label the Screen tab's replay: that happens in the
  // lazily loaded files panel now, so the task page's first scripts carry only the words' loading state.
  it("keeps the live conversation free of the step summaries and the replay's steps", () => {
    const live = read("src/transcript/LiveConversation.tsx");
    expect(live).not.toMatch(/^import .* from "\.\/tool-summary"/m);
    expect(live).not.toMatch(/^import .* from "\.\.\/screen\/replay-steps"/m);
    expect(live).toContain('from "./desktop-words-state"');
  });

  it("gives the renderers' copy its own area, registered by every file that shows it", () => {
    const area = AREAS.find((candidate) => candidate.name === "turns");
    expect(area?.registration).toBe("src/transcript/turns-catalogue.ts");
    for (const file of [
      "TranscriptTurns.tsx",
      "ActivityGroup.tsx",
      "ActivityLine.tsx",
      "RecordedSummary.tsx",
      "present.ts",
      "turn-summary.ts",
    ])
      expect(read(`src/transcript/${file}`), file).toContain(
        'import "./turns-catalogue"',
      );
  });
});

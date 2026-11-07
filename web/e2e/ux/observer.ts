import { expect, type Page } from "@playwright/test";

type Observation = {
  phase: "following" | "paused" | "inspecting";
  flushes: number;
  followed: number;
  running: number;
  failures: { family: string; detail: string }[];
  failureCount: number;
};
declare global {
  interface Window {
    __ux: Observation;
    __uxLatest: () => {
      top: number;
      bottom: number;
      viewportTop: number;
      viewportBottom: number;
    } | null;
  }
}

/** Samples published DOM updates after layout, and checks status and shell geometry every frame. */
export async function observe(page: Page) {
  await page.addInitScript(() => {
    window.__ux = {
      phase: "following",
      flushes: 0,
      followed: 0,
      running: 0,
      failures: [],
      failureCount: 0,
    };
    let dirty = false;
    // Counts frames, so a recheck can prove it judged the frame it was asked about.
    let frame = 0;
    // A recheck that hasn't reported by the next frame is a failure too, never a silent pass.
    let unreported: (() => void) | null = null;
    const fail = (family: string, detail: string) => {
      window.__ux.failureCount++;
      if (window.__ux.failures.length < 20)
        window.__ux.failures.push({ family, detail });
    };
    window.__uxLatest = () => {
      const root = document.querySelector('[data-testid="transcript"]');
      const region = document.querySelector('[aria-label="Transcript"]');
      if (!root || !region) return null;
      const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
      let last: Node | null = null;
      for (let node = walker.nextNode(); node; node = walker.nextNode())
        if (node.textContent?.trim() && node.parentElement?.checkVisibility())
          last = node;
      if (!last) return null;
      const range = document.createRange();
      const end = last.textContent?.trimEnd().length ?? 0;
      range.setStart(last, Math.max(0, end - 1));
      range.setEnd(last, end);
      const line = range.getBoundingClientRect();
      const viewport = region.getBoundingClientRect();
      return {
        top: line.top,
        bottom: line.bottom,
        viewportTop: viewport.top,
        viewportBottom: viewport.bottom,
      };
    };
    const outside = (latest: ReturnType<Window["__uxLatest"]>) =>
      latest !== null &&
      (latest.bottom > latest.viewportBottom + 2 ||
        latest.top < latest.viewportTop - 2);
    const where = (latest: NonNullable<ReturnType<Window["__uxLatest"]>>) =>
      `latest line ${latest.top}…${latest.bottom}, viewport ${latest.viewportTop}…${latest.viewportBottom}`;
    const sample = () => {
      frame++;
      unreported?.();
      unreported = null;
      const run = document.querySelector(
        '[aria-label="Run status"]',
      )?.textContent;
      const sandbox =
        document.querySelector(".environment-status, .sandbox-status")
          ?.textContent ?? "";
      if (run?.startsWith("Working…")) {
        window.__ux.running++;
        if (/idle|sleep|asleep/i.test(sandbox))
          fail("status", `Working contradicts ${sandbox}`);
      }
      for (const element of document.querySelectorAll(
        ".conversation-bar, .environment-status, [role=alert]",
      ))
        if (
          element.checkVisibility() &&
          /\b(sandbox|idle)\b/i.test(element.textContent ?? "")
        )
          fail(
            "copy",
            "Internal state words are visible in application chrome",
          );
      const doc = document.scrollingElement;
      if (
        doc &&
        (doc.scrollWidth > innerWidth + 1 ||
          doc.scrollHeight > innerHeight + 1 ||
          doc.scrollTop !== 0)
      )
        fail(
          "layout",
          `Page overflow: ${doc.scrollWidth}×${doc.scrollHeight}, viewport ${innerWidth}×${innerHeight}, top ${doc.scrollTop}`,
        );
      const transcript = document.querySelector('[data-testid="transcript"]');
      const region = document.querySelector('[aria-label="Transcript"]');
      if (dirty && transcript && region && run?.startsWith("Working…")) {
        window.__ux.flushes++;
        if (window.__ux.phase === "following") {
          window.__ux.followed++;
          const latest = window.__uxLatest();
          if (latest !== null && outside(latest)) {
            // This runs before the frame's resize observations, where the page follows growth it didn't render
            // itself (a lazily loaded part committing alone): judge what this frame paints. A resize observer made
            // now reports in this frame's observations, after the page's (made earlier), and before paint; one that
            // reports in a later frame fails, so a jump a person sees is never excused (TEST-PLAN §19a).
            const flush = window.__ux.flushes;
            const asked = frame;
            const recheck = new ResizeObserver(() => {
              recheck.disconnect();
              unreported = null;
              const painted = window.__uxLatest();
              if (frame !== asked)
                fail(
                  "follow",
                  `Flush ${flush}: ${where(latest)}; its recheck came a frame late`,
                );
              else if (painted === null || outside(painted))
                fail(
                  "follow",
                  `Flush ${flush}: ${where(painted ?? latest)} as painted (before the page's resize handling: ${where(latest)})`,
                );
            });
            recheck.observe(region);
            unreported = () => {
              recheck.disconnect();
              fail(
                "follow",
                `Flush ${flush}: ${where(latest)}; its recheck never reported`,
              );
            };
          }
        }
      }
      dirty = false;
      requestAnimationFrame(sample);
    };
    addEventListener(
      "DOMContentLoaded",
      () => {
        new MutationObserver(() => {
          dirty = true;
        }).observe(document.body, {
          subtree: true,
          childList: true,
          characterData: true,
        });
        requestAnimationFrame(sample);
      },
      { once: true },
    );
  });
}

export async function phase(page: Page, value: Observation["phase"]) {
  await page.evaluate((value) => {
    window.__ux.phase = value;
  }, value);
}

export async function observations(page: Page) {
  const result = await page.evaluate(() => window.__ux);
  expect
    .soft(result.running, "status: sample the real running state")
    .toBeGreaterThan(0);
  expect
    .soft(result.followed, "follow: sample multiple live flushes")
    .toBeGreaterThan(2);
  expect
    .soft(
      result.failures,
      `Live UX invariants: ${result.failureCount} failures`,
    )
    .toEqual([]);
  return result;
}

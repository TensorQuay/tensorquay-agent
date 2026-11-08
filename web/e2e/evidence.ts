import { mkdir } from "node:fs/promises";
import { join } from "node:path";

import type { Page } from "@playwright/test";

/** How long a shot waits for the page's motion at most: a fade or a slide lasts a fraction of a second. */
const BOUND_MS = 5_000;

/**
 * In the page: every finite animation and transition running now (a fade, a slide) has finished, or `bound` ms have
 * passed; an endless one (the run mark's pulse, the live line's shimmer) is drawn where it is. Playwright sends its
 * source to the page, so it uses nothing from outside itself.
 */
function finiteMotionDone(bound: number): Promise<void> {
  const running = document
    .getAnimations()
    .filter(
      (motion) =>
        motion.playState === "running" &&
        motion.effect?.getComputedTiming().iterations !== Infinity,
    );
  return Promise.race([
    Promise.all(
      running.map((motion) => motion.finished.catch(() => undefined)),
    ).then(() => undefined),
    new Promise<void>((resolve) => setTimeout(resolve, bound)),
  ]);
}

/**
 * A screenshot for the reviewers when TQA_WEB_EVIDENCE_DIR names a folder, taken once the page's finite motion has
 * finished, so a shot never lands mid-fade (a field caught mid-way to its disabled look read as a stray box).
 */
export async function capture(page: Page, name: string) {
  const directory = process.env.TQA_WEB_EVIDENCE_DIR;
  if (!directory) return;
  await mkdir(directory, { recursive: true });
  await page.evaluate(finiteMotionDone, BOUND_MS);
  await page.screenshot({ path: join(directory, `${name}.png`) });
}

import type { Page } from "@playwright/test";

/** How long a popover's entrance motion may take, however slow the host: it lasts a fraction of a second. */
const BOUND = 10_000;

/** Waits for the open popovers' entrance motion, so contrast is measured on the settled layer, not mid-fade. */
export async function settled(page: Page) {
  await page.locator(".popover").evaluateAll(finishAll, BOUND);
}

/**
 * In the page: every animation of these elements finished, or, past `bound` ms, an error naming those that didn't, so a
 * stalled animation is told apart from a slow host whose test ran out of time (TEST-PLAN §19a). A cancelled one
 * rejects at once. Playwright sends its source to the page, so it uses nothing from outside itself.
 */
export function finishAll(elements: Element[], bound: number): Promise<void> {
  const motions = elements.flatMap((element) => element.getAnimations());
  let timer: ReturnType<typeof setTimeout> | undefined;
  const stalled = new Promise<never>((_, reject) => {
    timer = setTimeout(() => {
      const open = motions
        .filter((motion) => motion.playState !== "finished")
        .map(
          (motion) =>
            `${(motion as CSSAnimation).animationName || motion.id || "an animation"} (${motion.playState} at ${String(motion.currentTime)} ms)`,
        );
      reject(
        new Error(
          `A popover's motion did not finish within ${bound} ms: ${open.join(", ")}`,
        ),
      );
    }, bound);
  });
  return Promise.race([
    Promise.all(motions.map((motion) => motion.finished)).then(() => undefined),
    stalled,
  ]).finally(() => clearTimeout(timer));
}

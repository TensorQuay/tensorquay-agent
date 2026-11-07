import { mkdir, readFile } from "node:fs/promises";
import { join, resolve } from "node:path";

import { expect, type Locator, type Page } from "@playwright/test";

import { phase } from "./observer";

/** Capture only visible activity; later narration may grow below the scroll viewport. */
export async function liveVisual(page: Page, theme: string) {
  await phase(page, "inspecting");
  await page.setViewportSize({ width: 1440, height: 1200 });
  // The taller viewport can hold the whole live transcript for a moment; scrolling then leaves the reader at the
  // bottom and nothing is unseen. Wait until the stream overflows past the activity before inspecting it.
  await expect
    .poll(
      () =>
        page.locator(".activity-group").evaluate((element) => {
          const region = element.closest(".transcript-scroll");
          if (!region) return 0;
          const offset =
            element.getBoundingClientRect().top -
            region.getBoundingClientRect().top +
            region.scrollTop;
          return region.scrollHeight - region.clientHeight - offset;
        }),
      { message: "live visual: the stream must overflow below the activity" },
    )
    .toBeGreaterThan(100);
  await page.locator(".activity-group").evaluate((element) => {
    const region = element.closest(".transcript-scroll");
    if (!region)
      throw new Error("Activity must belong to the transcript scroll region");
    region.scrollTop +=
      element.getBoundingClientRect().top -
      region.getBoundingClientRect().top -
      4;
  });
  await expect(page.getByLabel("Run status")).toHaveText(/^Working…/);
  await expect(page.locator(".activity-heading")).toHaveAttribute(
    "aria-expanded",
    "true",
  );
  await expect(
    page.getByRole("button", { name: /^Jump to latest/ }),
  ).toBeVisible();
  const clip = await page.locator(".activity-group").evaluate((element) => {
    const region = element.closest(".transcript-scroll");
    if (!region)
      throw new Error("Activity must belong to the transcript scroll region");
    const activity = element.getBoundingClientRect();
    const viewport = region.getBoundingClientRect();
    const x = Math.max(activity.left, viewport.left);
    const y = Math.max(activity.top, viewport.top);
    return {
      x,
      y,
      width: Math.min(activity.right, viewport.right) - x,
      height: Math.min(activity.bottom, viewport.bottom) - y,
    };
  });
  await visual(page, page, `ux-live-${theme}`, clip);
  await page.setViewportSize({ width: 1440, height: 900 });
  await page
    .getByRole("region", { name: "Transcript", exact: true })
    .evaluate((element) => {
      element.scrollTop = element.scrollHeight;
    });
  await phase(page, "following");
}

export async function finishedVisuals(page: Page, theme: string) {
  await page
    .getByRole("region", { name: "Transcript", exact: true })
    .evaluate((element) => {
      element.scrollTop = 0;
    });
  await visual(
    page,
    page.locator(".conversation-page"),
    `ux-finished-${theme}`,
  );
  await page.setViewportSize({ width: 1440, height: 1440 });
  await page
    .locator(".markdown-answer")
    .last()
    .evaluate((element) => {
      const region = element.closest(".transcript-scroll");
      if (!region)
        throw new Error("Answer must belong to the transcript scroll region");
      region.scrollTop +=
        element.getBoundingClientRect().top -
        region.getBoundingClientRect().top -
        4;
    });
  await visual(
    page,
    page.locator(".conversation-page"),
    `ux-markdown-${theme}`,
  );
  await page.setViewportSize({ width: 1440, height: 900 });
}

/** Candidate recording is explicit and private; normal runs require reviewed baselines. */
export async function visual(
  page: Page,
  target: Locator | Page,
  name: string,
  clip?: { x: number; y: number; width: number; height: number },
) {
  const folder = process.env.TQA_WEB_EVIDENCE_DIR;
  const stylePath = resolve("e2e/ux/capture.css");
  // The pointer rests where the flow last clicked; parked in the corner, it hovers nothing in the capture.
  await page.mouse.move(0, 0);
  // Mask only what the reader can see: a mask ignores scroll clipping and would cover the fixed bar above.
  await page.evaluate(() => {
    const variable = document.querySelectorAll(
      "[aria-label='Last activity'], .recorded-duration, .run-elapsed",
    );
    for (const element of variable) {
      const box = element.getBoundingClientRect();
      const region = element.closest(".transcript-scroll");
      const view = region?.getBoundingClientRect();
      const shown = !view || (box.bottom > view.top && box.top < view.bottom);
      element.toggleAttribute("data-ux-mask", shown);
    }
  });
  const options = {
    ...(clip ? { clip } : {}),
    animations: "disabled" as const,
    caret: "hide" as const,
    mask: [page.locator("[data-ux-mask]")],
  };
  if (folder) {
    const destination =
      process.env.TQA_UX_CAPTURE === "1" ? folder : join(folder, "actual");
    if (
      process.env.TQA_UX_BASELINES &&
      resolve(destination) === resolve(process.env.TQA_UX_BASELINES)
    )
      throw new Error(
        "Actual screenshots must never overwrite reference screenshots",
      );
    await mkdir(destination, { recursive: true });
    await target.screenshot({
      ...options,
      style: await readFile(stylePath, "utf8"),
      path: join(destination, `${name}.png`),
    });
  }
  if (
    process.env.TQA_UX_CAPTURE === "1" ||
    process.env.TQA_UX_REAL_MODEL === "1"
  ) {
    if (!folder)
      throw new Error(
        "Candidate and smoke captures require TQA_WEB_EVIDENCE_DIR",
      );
    return;
  }
  await expect(target).toHaveScreenshot(`${name}.png`, {
    ...options,
    stylePath,
    maxDiffPixelRatio: 0.001,
    threshold: 0.1,
  });
}

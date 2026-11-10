import { readFileSync } from "node:fs";

import { expect, test } from "@playwright/test";

// WEB-U-464 (17.4 fix): a sent picture sits in a frame that hugs it, and fits its message on the narrowest screen. The
// list sizes each item to its content, and an image's max-width given as a percentage is left out of that size, so the
// frame around a wide picture kept the width the picture has at its 240 px height cap: an empty band beside it, in
// light and dark. Measured in the task page's own wrappers (the conversation's container, its page and the transcript's
// scroller, here with a vertical scrollbar) with the app's own stylesheets, for both frames (a button where the page has
// a Files panel, a plain one where it doesn't), on a desktop and on phones around the width where the picture starts
// to shrink.
const styles = [
  "src/app/globals.css",
  "src/app/conversation.css",
  "src/app/files.css",
]
  .map((file) => readFileSync(file, "utf8"))
  .join("\n");

// Headless Chromium hides scrollbars unless told otherwise; this file wants the classic ones a desktop draws.
test.use({ launchOptions: { ignoreDefaultArgs: ["--hide-scrollbars"] } });

for (const width of [1280, 390, 360, 330, 312, 300]) {
  for (const frame of ["button", "span"] as const) {
    test(`WEB-U-464 a sent picture's frame hugs the picture and fits its message at ${width} px (${frame} frame)`, async ({
      page,
    }) => {
      await page.setViewportSize({ width, height: 640 });
      const element =
        frame === "button"
          ? '<button type="button" class="sent-picture"><img alt="hills"></button>'
          : '<span class="sent-picture"><img alt="hills"></span>';
      // A tall spacer gives the transcript a vertical scrollbar, which headless Chromium on Linux draws as a classic one.
      await page.setContent(`<!doctype html><style>${styles}</style>
        <main class="app-content"><div class="conversation-layout" style="height: 600px">
        <div class="conversation-page"><div class="transcript-scroll">
        <div class="user-message"><p>hi</p><ul class="attachment-chips sent-attachments"><li>${element}</li></ul></div>
        <div style="height: 2000px"></div></div></div></div></main>`);
      // A 640 x 400 picture, drawn on a canvas so the test needs no file.
      await page.evaluate(async () => {
        const canvas = document.createElement("canvas");
        canvas.width = 640;
        canvas.height = 400;
        const image = document.querySelector("img");
        if (!image) throw new Error("no picture");
        image.src = canvas.toDataURL("image/png");
        await image.decode();
      });
      const box = await page.evaluate(() => {
        const element = (selector: string) => {
          const found = document.querySelector(selector);
          if (!found) throw new Error(`no ${selector}`);
          return found;
        };
        const overflow = (selector: string) => {
          const found = element(selector);
          return found.scrollWidth - found.clientWidth;
        };
        return {
          frame: element(".sent-picture").getBoundingClientRect().width,
          frameRight: element(".sent-picture").getBoundingClientRect().right,
          picture: element(".sent-picture img").getBoundingClientRect().width,
          bubbleRight: element(".user-message").getBoundingClientRect().right,
          pageScroll: document.documentElement.scrollWidth - window.innerWidth,
          contentScroll: overflow(".app-content"),
          transcriptScroll: overflow(".transcript-scroll"),
          transcriptScrollbar:
            element(".transcript-scroll").getBoundingClientRect().width -
            element(".transcript-scroll").clientWidth,
        };
      });
      expect(
        box.transcriptScrollbar,
        "the transcript shows a vertical scrollbar",
      ).toBeGreaterThan(0);
      if (width >= 1024)
        expect(box.picture, "the picture at its 240 px cap").toBe(240);
      expect(
        box.frame - box.picture,
        "the frame's room beside the picture: its two borders",
      ).toBeLessThanOrEqual(2);
      expect(
        box.frameRight,
        "the frame inside its message",
      ).toBeLessThanOrEqual(box.bubbleRight);
      expect(
        box.pageScroll,
        "no horizontal scroll on the page",
      ).toBeLessThanOrEqual(0);
      expect(
        box.contentScroll,
        "no horizontal scroll in the main area",
      ).toBeLessThanOrEqual(0);
      expect(
        box.transcriptScroll,
        "no horizontal scroll in the transcript",
      ).toBeLessThanOrEqual(0);
    });
  }
}

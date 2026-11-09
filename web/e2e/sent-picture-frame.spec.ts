import { readFileSync } from "node:fs";

import { expect, test } from "@playwright/test";

// WEB-U-464 (17.4 fix): a sent picture sits in a frame that hugs it, and fits its message on the narrowest phone. The
// list sizes each item to its content, and an image's max-width given as a percentage is left out of that size, so the
// frame around a wide picture kept the width the picture has at its 240 px height cap: an empty band beside it, in
// light and dark. Measured in the message bubble with the app's own stylesheets, for both frames (a button where the
// page has a Files panel, a plain one where it doesn't), on a desktop and on a phone narrower than the picture.
const styles = [
  "src/app/globals.css",
  "src/app/conversation.css",
  "src/app/files.css",
]
  .map((file) => readFileSync(file, "utf8"))
  .join("\n");

for (const [label, viewport] of [
  ["a desktop", { width: 1280, height: 800 }],
  ["a 300 px phone", { width: 300, height: 640 }],
] as const) {
  for (const frame of ["button", "span"] as const) {
    test(`WEB-U-464 a sent picture's frame hugs the picture and fits its message on ${label} (${frame} frame)`, async ({
      page,
    }) => {
      await page.setViewportSize(viewport);
      const element =
        frame === "button"
          ? '<button type="button" class="sent-picture"><img alt="hills"></button>'
          : '<span class="sent-picture"><img alt="hills"></span>';
      await page.setContent(`<!doctype html><style>${styles}</style>
        <main class="app-content"><div class="user-message"><p>hi</p>
        <ul class="attachment-chips sent-attachments"><li>${element}</li></ul></div></main>`);
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
        const rect = (selector: string) => {
          const found = document.querySelector(selector);
          if (!found) throw new Error(`no ${selector}`);
          return found.getBoundingClientRect();
        };
        const content = document.querySelector(".app-content");
        return {
          frame: rect(".sent-picture").width,
          frameRight: rect(".sent-picture").right,
          picture: rect(".sent-picture img").width,
          bubbleRight: rect(".user-message").right,
          pageScroll: document.documentElement.scrollWidth - window.innerWidth,
          contentScroll: content
            ? content.scrollWidth - content.clientWidth
            : 0,
        };
      });
      if (viewport.width >= 1024)
        expect(box.picture, "the picture at its 240 px cap").toBe(240);
      else
        expect(
          box.picture,
          "the picture shrinks to fit a narrow message",
        ).toBeLessThan(240);
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
    });
  }
}

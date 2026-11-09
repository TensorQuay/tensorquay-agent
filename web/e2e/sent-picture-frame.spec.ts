import { readFileSync } from "node:fs";

import { expect, test } from "./test";

// WEB-U-464 (17.4 fix): a sent picture sits in a frame that hugs it. The list sizes each item to its content, and an
// image's max-width given as a percentage is left out of that size, so the frame around a wide picture kept the width
// the picture would have at its 240 px height cap: an empty band beside the picture, in light and dark. Measured with
// the app's own stylesheets, in a wide message and a narrow one.
test("WEB-U-464 a sent picture's frame hugs the picture in a wide and a narrow message", async ({
  page,
}) => {
  const styles = ["src/app/globals.css", "src/app/files.css"]
    .map((file) => readFileSync(file, "utf8"))
    .join("\n");
  for (const width of [800, 300]) {
    await page.setContent(`<!doctype html><style>${styles}</style>
      <div style="width: ${width}px"><ul class="attachment-chips sent-attachments"><li>
      <button type="button" class="sent-picture"><img alt="hills"></button></li></ul></div>`);
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
    const [frame, picture] = await page.evaluate(() => [
      document.querySelector(".sent-picture")?.getBoundingClientRect().width ??
        0,
      document.querySelector(".sent-picture img")?.getBoundingClientRect()
        .width ?? 0,
    ]);
    expect(picture, `the picture in a ${width} px message`).toBe(240);
    expect(
      frame - picture,
      `the frame's room beside the picture in a ${width} px message (its two borders)`,
    ).toBeLessThanOrEqual(2);
  }
});

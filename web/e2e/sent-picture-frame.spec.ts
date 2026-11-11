import { expect, test, type Page } from "@playwright/test";

// WEB-U-464 (17.4 fix; the width of the transcript's items, 17.4.1 fix): a sent picture sits in a frame that hugs it,
// and fits its message however narrow the transcript. The list sizes each item to its content, and an image's
// max-width given as a percentage is left out of that size, so the frame around a wide picture kept the width the
// picture has at its 240 px height cap: an empty band beside it. The picture's cap is the width of the transcript's
// items, the container it measures (the scroller's content, less any classic scrollbar, which container units on the
// scroller itself would count), less the bubble's padding and the frame's borders; it never goes under 48 px, so a
// column narrower than that keeps the picture to see.
//
// Measured in the task page's own wrappers, in both frames (a button where the page has a Files panel, a plain one
// where it doesn't), styled by the app's built stylesheets (those its pages link: Tailwind's preflight and every sheet
// globals.css imports), on a page of the app's own origin.

const PICTURE = { width: 640, height: 400 };
const CAP = 240;
const FLOOR = 48;

/** The stylesheets the app's pages link, as built. */
async function stylesheets(page: Page): Promise<string[]> {
  await page.goto("/login");
  const links = await page.evaluate(() =>
    [
      ...document.querySelectorAll<HTMLLinkElement>('link[rel="stylesheet"]'),
    ].map((link) => link.href),
  );
  expect(links.length, "the app's stylesheets").toBeGreaterThan(0);
  return links;
}

/**
 * The task page's wrappers around one sent picture, with a tall transcript, and the Files panel when docked. A
 * transcript with overlay scrollbars gives them no room (`scrollbar-width: none` stands in for them).
 */
function wrappers(input: {
  frame: "button" | "span";
  scrollbars: "classic" | "overlay";
  docked: boolean;
  column: number | null;
}) {
  const element =
    input.frame === "button"
      ? '<button type="button" class="sent-picture"><img alt="hills"></button>'
      : '<span class="sent-picture"><img alt="hills"></span>';
  const size =
    input.column === null
      ? "height: 600px"
      : `width: ${input.column}px; height: 600px`;
  return `<main class="app-content"><div class="conversation-layout" data-panel="${input.docked}" style="${size}">
    <div class="conversation-page"><div class="transcript-scroll"${input.scrollbars === "overlay" ? ' style="scrollbar-width: none"' : ""}>
    <div class="transcript-items"><article class="conversation-turn stack" aria-label="Turn">
    <article class="user-message" aria-label="Your message"><p class="message-text">hi</p>
    <ul class="attachment-chips sent-attachments"><li>${element}</li></ul></article></article>
    <div style="height: 2000px"></div></div></div></div>
    ${input.docked ? '<aside class="files-panel" aria-label="Files"></aside>' : ""}</div></main>`;
}

/** Shows the wrappers on a page of the app's origin with its stylesheets, and draws a 640 x 400 picture in them. */
async function show(page: Page, body: string) {
  const links = (await stylesheets(page))
    .map((href) => `<link rel="stylesheet" href="${href}">`)
    .join("");
  await page.route("**/sent-picture-frame", (route) =>
    route.fulfill({
      contentType: "text/html",
      body: `<!doctype html><html lang="en"><head><meta charset="utf-8">${links}</head><body>${body}</body></html>`,
    }),
  );
  await page.goto("/sent-picture-frame");
  await page.evaluate(async (size) => {
    const canvas = document.createElement("canvas");
    canvas.width = size.width;
    canvas.height = size.height;
    const image = document.querySelector("img");
    if (!image) throw new Error("no picture");
    image.src = canvas.toDataURL("image/png");
    await image.decode();
  }, PICTURE);
}

/** Where the frame, the picture, the bubble's content box and the transcript's content box are. */
function measure(page: Page) {
  return page.evaluate(() => {
    const element = (selector: string) => {
      const found = document.querySelector<HTMLElement>(selector);
      if (!found) throw new Error(`no ${selector}`);
      return found;
    };
    /** An element's content box, across: its box less its borders and padding (and a scroller's scrollbar). */
    const content = (found: HTMLElement) => {
      const box = found.getBoundingClientRect();
      const style = getComputedStyle(found);
      const left =
        box.left +
        parseFloat(style.borderLeftWidth) +
        parseFloat(style.paddingLeft);
      return {
        left,
        width:
          found.clientWidth -
          parseFloat(style.paddingLeft) -
          parseFloat(style.paddingRight),
      };
    };
    const overflow = (selector: string) => {
      const found = element(selector);
      return found.scrollWidth - found.clientWidth;
    };
    const frame = element(".sent-picture").getBoundingClientRect();
    const bubble = content(element(".user-message"));
    const scroller = element(".transcript-scroll");
    const panel = document.querySelector(".files-panel");
    return {
      frameLeft: frame.left,
      frameRight: frame.right,
      frame: frame.width,
      picture: element(".sent-picture img").getBoundingClientRect().width,
      bubbleLeft: bubble.left,
      bubbleRight: bubble.left + bubble.width,
      bubbleWidth: element(".user-message").getBoundingClientRect().width,
      transcript: content(scroller).width,
      scrollbar:
        scroller.offsetWidth -
        scroller.clientWidth -
        parseFloat(getComputedStyle(scroller).borderLeftWidth) -
        parseFloat(getComputedStyle(scroller).borderRightWidth),
      pageScroll: document.documentElement.scrollWidth - window.innerWidth,
      contentScroll: overflow(".app-content"),
      transcriptScroll: overflow(".transcript-scroll"),
      panelPosition: panel ? getComputedStyle(panel).position : null,
      panelLeft: panel?.getBoundingClientRect().left ?? null,
      transcriptRight: scroller.getBoundingClientRect().right,
    };
  });
}

type Box = Awaited<ReturnType<typeof measure>>;

/** The frame hugs the picture, sits inside the bubble's content box, and nothing scrolls sideways. */
function fits(box: Box) {
  expect(
    box.frame - box.picture,
    "the frame's room beside the picture: its two borders",
  ).toBeLessThanOrEqual(2);
  expect(
    box.frameLeft,
    "the frame inside the bubble's content box, at its start",
  ).toBeGreaterThanOrEqual(box.bubbleLeft - 0.5);
  expect(
    box.frameRight,
    "the frame inside the bubble's content box, at its end",
  ).toBeLessThanOrEqual(box.bubbleRight + 0.5);
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
  // Where the column holds the picture under its cap, the picture takes all the room: the bubble is as wide as the
  // transcript's content box, with nothing set aside for a scrollbar that isn't there.
  if (box.picture < CAP - 0.5)
    expect(
      Math.abs(box.bubbleWidth - box.transcript),
      "a picture under its cap uses the transcript's whole width",
    ).toBeLessThanOrEqual(1);
}

// A desktop draws classic scrollbars, which take the transcript's width; a phone, and macOS, draw overlay ones, which
// take none. Headless Chromium hides scrollbars unless told otherwise; this file wants a desktop's classic ones.
test.use({ launchOptions: { ignoreDefaultArgs: ["--hide-scrollbars"] } });

for (const scrollbars of ["classic", "overlay"] as const)
  test.describe(`with ${scrollbars} scrollbars`, () => {
    for (const width of [1280, 390, 360, 330, 312, 300])
      for (const frame of ["button", "span"] as const)
        test(`WEB-U-464 a sent picture's frame hugs the picture and fits its message at ${width} px (${frame} frame)`, async ({
          page,
        }) => {
          await page.setViewportSize({ width, height: 640 });
          await show(
            page,
            wrappers({ frame, scrollbars, docked: false, column: null }),
          );
          const box = await measure(page);
          if (scrollbars === "classic")
            // macOS Chromium draws overlay scrollbars even when told to show them: the case has no scrollbar there.
            test.skip(
              box.scrollbar === 0,
              "Chromium draws overlay scrollbars here",
            );
          else expect(box.scrollbar, "no scrollbar takes room").toBe(0);
          fits(box);
          if (width === 1280)
            expect(box.picture, "the picture at its cap").toBeCloseTo(CAP, 0);
        });

    // With the Files panel docked beside the transcript, the picture measures the transcript, not the whole layout.
    for (const frame of ["button", "span"] as const)
      test(`WEB-U-464 a sent picture fits its message beside the docked Files panel (${frame} frame)`, async ({
        page,
      }) => {
        await page.setViewportSize({ width: 1440, height: 640 });
        await show(
          page,
          wrappers({ frame, scrollbars, docked: true, column: null }),
        );
        const box = await measure(page);
        if (scrollbars === "classic")
          test.skip(
            box.scrollbar === 0,
            "Chromium draws overlay scrollbars here",
          );
        expect(
          box.panelPosition,
          "the panel docked, not over the column",
        ).not.toBe("absolute");
        expect(
          box.panelLeft ?? 0,
          "the panel beside the transcript",
        ).toBeGreaterThanOrEqual(box.transcriptRight - 0.5);
        fits(box);
      });

    // A column narrower than the picture's floor keeps the picture to see: 48 px across, never squeezed to nothing.
    test(`WEB-U-464 a sent picture keeps 48 px in a column narrower than that`, async ({
      page,
    }) => {
      await page.setViewportSize({ width: 390, height: 640 });
      await show(
        page,
        wrappers({ frame: "button", scrollbars, docked: false, column: 56 }),
      );
      const box = await measure(page);
      if (scrollbars === "classic")
        test.skip(
          box.scrollbar === 0,
          "Chromium draws overlay scrollbars here",
        );
      expect(box.picture, "the picture at its floor").toBeCloseTo(FLOOR, 0);
    });
  });

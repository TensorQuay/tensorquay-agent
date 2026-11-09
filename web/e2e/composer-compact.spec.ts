import { mkdir } from "node:fs/promises";
import { join } from "node:path";

import { createConversation, signIn } from "./session";
import { expect, test, type Page } from "./test";

const HOLD = "Summarize the workspace, then wait.";
const LANGUAGES = [
  "en",
  "es",
  "fr",
  "ja",
  "ko",
  "ms",
  "ta",
  "th",
  "vi",
  "zh-Hans",
];

/** A part of the page for the reviewers, at its 2x scale, once its finite motion has ended. */
async function shot(page: Page, name: string, whole = false) {
  const directory = process.env.TQA_WEB_EVIDENCE_DIR;
  if (!directory) return;
  await mkdir(directory, { recursive: true });
  await page.evaluate(() =>
    Promise.race([
      Promise.all(
        document
          .getAnimations()
          .filter(
            (motion) =>
              motion.effect?.getComputedTiming().iterations !== Infinity,
          )
          .map((motion) => motion.finished.catch(() => undefined)),
      ),
      new Promise((resolve) => setTimeout(resolve, 5_000)),
    ]),
  );
  const path = join(directory, `${name}.png`);
  if (whole) await page.screenshot({ path });
  else
    await page
      .getByRole("form", { name: "Message composer" })
      .screenshot({ path });
}

/** The composer's parts as the browser lays them out: its card, its text, its + and what sends or stops. */
function parts(page: Page) {
  return page.evaluate(() => {
    const card = document.querySelector(".composer-card");
    const text = card?.querySelector("textarea");
    const buttons = [
      ...(card?.querySelectorAll<HTMLElement>(".icon-action > .button") ?? []),
    ];
    const add = buttons[0];
    const send = card?.querySelector(".composer-submit .button");
    if (!card || !text || !add || !send)
      throw new Error("The composer must be laid out");
    const rect = (element: Element) => {
      const box = element.getBoundingClientRect();
      return {
        top: box.top,
        bottom: box.bottom,
        left: box.left,
        right: box.right,
        height: box.height,
        middle: box.top + box.height / 2,
      };
    };
    const style = getComputedStyle(card);
    const page = getComputedStyle(document.body).backgroundColor;
    return {
      card: rect(card),
      text: rect(text),
      add: rect(add),
      send: rect(send),
      padding:
        parseFloat(style.paddingBottom) + parseFloat(style.borderBottomWidth),
      chips: [...card.querySelectorAll('ul, [role="list"]')].map((list) =>
        rect(list),
      ),
      border: style.borderTopColor,
      outline:
        style.outlineStyle === "none" ? 0 : parseFloat(style.outlineWidth),
      page,
      scrolls: text.scrollHeight > text.clientHeight + 1,
    };
  });
}

/** The relative luminance contrast of two computed colours, as WCAG reckons it. */
function contrast(a: string, b: string) {
  // A transparent colour shows the page itself: no contrast.
  if (/rgba\([^)]*,\s*0\)$/.test(a)) return 1;
  const lum = (value: string) => {
    const [r, g, bl] = (value.match(/[\d.]+/g) ?? [])
      .slice(0, 3)
      .map(Number)
      .map((channel) => {
        const c = channel / 255;
        return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
      }) as [number, number, number];
    return 0.2126 * r + 0.7152 * g + 0.0722 * bl;
  };
  const [x, y] = [lum(a), lum(b)].sort((p, q) => q - p) as [number, number];
  return (x + 0.05) / (y + 0.05);
}

const near = (a: number, b: number, by = 1.5) => Math.abs(a - b) <= by;
const csv = (name: string) => ({
  name,
  mimeType: "text/csv",
  buffer: Buffer.from("region,total\nNorth,12\n"),
});

/** One row at rest: + at the start, the text, send at the end, all on one middle line, about 52 px high. */
async function oneRow(page: Page, at: string) {
  const row = await parts(page);
  expect
    .soft(
      row.card.height >= 50 && row.card.height <= 56,
      `${at}: one row (${row.card.height})`,
    )
    .toBe(true);
  expect
    .soft(
      row.add.right <= row.text.left && row.text.right <= row.send.left,
      `${at}: +, the text, send`,
    )
    .toBe(true);
  for (const part of [row.add, row.send])
    expect
      .soft(
        near(part.middle, row.text.middle),
        `${at}: on the text's middle line`,
      )
      .toBe(true);
  return row;
}

// WEB-E2E-118 (UI-GUIDELINES §7 Composer, decided on 9 Nov 2026): the composer is one row at rest, + at its start, the
// text, and send (or stop) at its end, about 52 px high; it grows with the text line by line, + and send staying at
// its bottom ends, so nothing they show moves while a line is typed; attachments show as chips above the text, only
// when there are any; its focus is a thin --focus border meeting 3:1, no 2 px ring. New task's starts the same, two
// lines only while its placeholder carries the keyboard hint. At 1280 and 390, light and dark.
for (const width of [1280, 390] as const)
  for (const colorScheme of ["light", "dark"] as const)
    test(`WEB-E2E-118 the compact composer at ${width} ${colorScheme}`, async ({
      browser,
    }) => {
      test.setTimeout(150_000);
      const context = await browser.newContext({
        viewport: { width, height: width === 390 ? 844 : 900 },
        deviceScaleFactor: 2,
        colorScheme,
        reducedMotion: "reduce",
        ...(width === 390 ? { isMobile: true, hasTouch: true } : {}),
      });
      const page = await context.newPage();
      await signIn(page);
      const suffix = `${width}-${colorScheme}`;
      // A person with a task already: New task's placeholder has no keyboard hint.
      await createConversation(page);
      for (const where of ["task", "new"] as const) {
        await page.goto(
          where === "task" ? await createConversation(page) : "/",
        );
        const input = page.getByRole("textbox", {
          name: "Message",
          exact: true,
        });
        await expect(input).toBeVisible();
        await page.mouse.move(5, 5);
        await shot(page, `page-${where}-${suffix}`, true);
        const empty = await oneRow(page, `${where} empty`);
        expect
          .soft(empty.chips, `${where}: no chips while there are none`)
          .toEqual([]);
        await shot(page, `composer-${where}-empty-${suffix}`);

        await input.fill("Summarize the figures");
        const line = await oneRow(page, `${where} one line`);
        if (width === 1280) {
          // Focus: a thin --focus border, which stands out from the page at 3:1 at least; no 2 px ring.
          await input.focus();
          const focused = await parts(page);
          expect.soft(focused.outline, `${where}: no ring`).toBe(0);
          expect
            .soft(
              contrast(focused.border, focused.page) >= 3,
              `${where}: the border at 3:1`,
            )
            .toBe(true);
          await page.locator("body").click({ position: { x: 5, y: 5 } });
        }
        await shot(page, `composer-${where}-one-line-${suffix}`);

        await input.fill(["One", "Two", "Three", "Four", "Five"].join("\n"));
        const five = await parts(page);
        expect
          .soft(
            five.text.height >= 5 * 24,
            `${where}: five lines (${five.text.height})`,
          )
          .toBe(true);
        // On a task's page the composer sits at the foot: it grows upward and + and send stay put. New task's start
        // block stays centred, so its card moves as it grows; + and send keep their place in it (below).
        if (where === "task")
          expect
            .soft(
              near(five.send.bottom, line.send.bottom) &&
                near(five.add.bottom, line.add.bottom),
              `${where}: + and send stay put`,
            )
            .toBe(true);
        expect
          .soft(
            near(five.send.bottom, five.card.bottom - five.padding),
            `${where}: at the bottom end`,
          )
          .toBe(true);
        await shot(page, `composer-${where}-five-lines-${suffix}`);

        await input.fill("");
        await page
          .getByLabel("Choose files to attach")
          .setInputFiles([csv("North.csv"), csv("South.csv")]);
        const files = page
          .getByRole("form", { name: "Message composer" })
          .getByRole("list", { name: "Attached files" });
        await expect(files.getByText("North.csv")).toBeVisible();
        const attached = await parts(page);
        expect
          .soft(attached.chips.length, `${where}: one row of chips`)
          .toBe(1);
        expect
          .soft(
            (attached.chips[0]?.bottom ?? Infinity) <= attached.text.top,
            `${where}: chips above the text`,
          )
          .toBe(true);
        await shot(page, `composer-${where}-attachments-${suffix}`);
      }

      // While a run goes, Stop is at the row's end, the composer still one row.
      await page.goto(await createConversation(page));
      const input = page.getByRole("textbox", { name: "Message", exact: true });
      await input.fill(HOLD);
      await input.press("Enter");
      await expect(page.getByRole("button", { name: "Stop" })).toBeVisible();
      await page.mouse.move(5, 5);
      await oneRow(page, "running");
      await shot(page, `composer-task-running-${suffix}`);
      const task = new URL(page.url()).pathname.split("/").at(-1) ?? "";
      await page.request.post(`/api/v1/conversations/${task}/fixture-resume`);
      await context.close();
    });

// WEB-E2E-118 in every language, on a phone: the placeholder fits the one-row box, nothing cut or scrolled.
test("WEB-E2E-118 the composer's placeholder fits one row in every language at 390", async ({
  browser,
}) => {
  test.setTimeout(150_000);
  const context = await browser.newContext({
    viewport: { width: 390, height: 844 },
    isMobile: true,
    hasTouch: true,
    reducedMotion: "reduce",
  });
  const page = await context.newPage();
  await signIn(page);
  const task = await createConversation(page);
  await page.goto(task);
  for (const language of LANGUAGES) {
    await page.evaluate(
      (code) => localStorage.setItem("tensorquay-language", code),
      language,
    );
    for (const path of [task, "/"]) {
      await page.goto(path);
      await expect(page.locator("html")).toHaveAttribute("lang", language);
      await expect(page.locator(".composer-card textarea")).toBeVisible();
      const at = await parts(page);
      expect
        .soft(at.scrolls, `${language} ${path}: the placeholder fits`)
        .toBe(false);
      expect
        .soft(
          at.card.height <= 56,
          `${language} ${path}: one row (${at.card.height})`,
        )
        .toBe(true);
    }
  }
  await page.evaluate(() => localStorage.removeItem("tensorquay-language"));
  await context.close();
});

// WEB-E2E-118 on a phone while a run goes, with a draft (decided on 9 Nov 2026): "Send after this" sits under the row,
// so the text keeps at least 60 % of the row's width, and nothing runs past the card.
for (const width of [360, 390] as const)
  test(`WEB-E2E-118 the composer while a run goes, with a draft, at ${width}`, async ({
    browser,
  }) => {
    const context = await browser.newContext({
      viewport: { width, height: 800 },
      deviceScaleFactor: 2,
      isMobile: true,
      hasTouch: true,
      reducedMotion: "reduce",
    });
    const page = await context.newPage();
    await signIn(page);
    await page.goto(await createConversation(page));
    const input = page.getByRole("textbox", { name: "Message", exact: true });
    await input.fill(HOLD);
    await input.press("Enter");
    await expect(page.getByRole("button", { name: "Stop" })).toBeVisible();
    await input.fill("Also check the totals for the north region");
    await expect(
      page.getByRole("button", { name: "Send after this" }),
    ).toBeVisible();
    const sizes = await page.evaluate(() => {
      const card = document.querySelector(".composer-card");
      const row = card?.querySelector(".composer-row");
      const field = row?.querySelector(".field");
      const after = [...(card?.querySelectorAll("button") ?? [])].find(
        (button) => button.textContent?.trim() === "Send after this",
      );
      if (!card || !row || !field || !after)
        throw new Error("The composer must be laid out");
      const box = (element: Element) => element.getBoundingClientRect();
      return {
        row: box(row).width,
        field: box(field).width,
        card: { left: box(card).left, right: box(card).right },
        after: { left: box(after).left, right: box(after).right },
        past:
          card.scrollWidth > card.clientWidth + 1 ||
          row.scrollWidth > row.clientWidth + 1,
      };
    });
    expect
      .soft(
        sizes.field / sizes.row >= 0.6,
        `the text's share (${sizes.field} of ${sizes.row})`,
      )
      .toBe(true);
    expect
      .soft(
        sizes.after.left >= sizes.card.left &&
          sizes.after.right <= sizes.card.right,
        "Send after this in the card",
      )
      .toBe(true);
    expect.soft(sizes.past, "nothing past the card").toBe(false);
    await shot(page, `composer-task-running-draft-${width}`);
    const task = new URL(page.url()).pathname.split("/").at(-1) ?? "";
    await page.request.post(`/api/v1/conversations/${task}/fixture-resume`);
    await context.close();
  });

import { createConversation, signIn } from "./session";
import { expect, test, type Page } from "./test";

// WEB-E2E-141 (18.2): a text button's label is never cut and never sticks out of the room it has, in any of the ten
// languages, at a phone's width and a desktop's, on the screens with the most buttons. A long translation wraps inside
// its button (WEB-U-511); icon-only buttons have no label and are left out.
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
] as const;
const WIDTHS = [390, 1280] as const;
const PATHS = [
  "/",
  "/workspaces",
  "/channels",
  "/settings",
  "/settings/account",
  "/settings/keys",
  "/settings/keys/shared",
  "/settings/models",
  "/settings/people",
  "/settings/environments",
  "/settings/connectors",
  "/settings/internet-access",
  "/settings/limits",
  "/settings/profiles",
  "/settings/tokens",
  "/settings/usage",
  "/settings/status",
  "/settings/sandboxes",
  // The component gallery: its "long label" state is a button whose room is narrower than its words.
  "/design",
];

/** In the page: each visible text button whose label doesn't fit, with the words and by how much. */
function cutButtons(): string[] {
  const found: string[] = [];
  const width = document.documentElement.clientWidth;
  for (const button of document.querySelectorAll<HTMLElement>(".button")) {
    const label = button.querySelector<HTMLElement>(".button-label");
    const words = (label?.textContent ?? "").trim();
    if (!label || !words) continue;
    const box = button.getBoundingClientRect();
    if (box.width === 0 || box.height === 0) continue;
    const style = getComputedStyle(button);
    if (style.visibility === "hidden") continue;
    // Cut inside its own box: the label is wider or taller than the button's content area.
    const inner =
      button.clientWidth -
      parseFloat(style.paddingLeft) -
      parseFloat(style.paddingRight);
    const text = label.getBoundingClientRect();
    if (label.scrollWidth > inner + 1 || text.height > box.height + 1)
      found.push(
        `"${words}": its label is ${Math.round(label.scrollWidth - inner)} px wider than the button`,
      );
    // Out of its room: past the window, or past the nearest parent that clips.
    else if (box.right > width + 1 || box.left < -1)
      found.push(
        `"${words}": ${Math.round(Math.max(box.right - width, -box.left))} px outside the window`,
      );
    else
      for (let at = button.parentElement; at; at = at.parentElement) {
        const overflow = getComputedStyle(at).overflowX;
        if (overflow === "visible") continue;
        const room = at.getBoundingClientRect();
        if (box.right > room.right + 1)
          found.push(
            `"${words}": ${Math.round(box.right - room.right)} px outside its clipping parent`,
          );
        break;
      }
  }
  return [...new Set(found)];
}

/** Each page's widest label, for the run's report: how close the longest translations come. */
const WIDEST: {
  language: string;
  viewport: number;
  path: string;
  words: string;
  px: number;
}[] = [];

async function check(
  page: Page,
  path: string,
  language: string,
  width: number,
  failures: string[],
) {
  const response = await page.goto(path);
  if (!response?.ok()) return;
  await expect(page.locator("main")).toBeVisible();
  await page.waitForLoadState("networkidle");
  await page.evaluate(() => document.fonts.ready);
  const widest = await page.evaluate(
    () =>
      [...document.querySelectorAll<HTMLElement>(".button .button-label")]
        .map((label) => ({
          words: (label.textContent ?? "").trim(),
          px: Math.round(label.scrollWidth),
        }))
        .sort((a, b) => b.px - a.px)[0],
  );
  if (widest) WIDEST.push({ language, viewport: width, path, ...widest });
  for (const finding of await page.evaluate(cutButtons))
    failures.push(`${language} ${width} px ${path}: ${finding}`);
}

for (const language of LANGUAGES)
  test(`WEB-E2E-141 button labels fit in ${language}`, async ({
    browser,
    baseURL,
  }) => {
    test.setTimeout(600_000);
    const failures: string[] = [];
    for (const width of WIDTHS) {
      const context = await browser.newContext({
        ...(baseURL ? { baseURL } : {}),
        locale: language,
        viewport: { width, height: 900 },
      });
      const page = await context.newPage();
      await page.addInitScript((code) => {
        try {
          localStorage.setItem("tensorquay-language", code);
        } catch {
          // A blocked storage leaves the browser's own language, which the context sets too.
        }
      }, language);
      await signIn(page);
      const task = await createConversation(page);
      for (const path of [...PATHS, task])
        await check(page, path, language, width, failures);
      await context.close();
    }
    const top = WIDEST.filter((entry) => entry.language === language)
      .sort((a, b) => b.px - a.px)
      .slice(0, 3);
    console.log(`Widest button labels in ${language}: ${JSON.stringify(top)}`);
    expect(failures, failures.join("\n")).toEqual([]);
  });

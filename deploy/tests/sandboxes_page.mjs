// E2E-OPS-001's browser half: one step on the real stack's pages, as a signed-in person, for sandboxes_stack.py's
// `page` case (the Sandboxes page design note). Its one argument is JSON:
//   {"origin", "cookies": [{name, value}], "out": folder, "step": "read", "path", "name", "widths": [1280, 390]}
//   {"origin", "cookies", "out", "step": "stop", "email"}  (Sandboxes: the row's menu, Stop now, Stop sandbox)
// It prints one JSON line: {"texts": {width: text}} for a read, {"said": text} for a stop. Screenshots go to `out`
// as <name>-<width>.png. Light theme, reduced motion; a console error or a failed request fails the step.
import { createRequire } from "node:module";
import { join } from "node:path";

const require = createRequire(new URL("../../web/package.json", import.meta.url));
const { chromium } = require("@playwright/test");
const input = JSON.parse(process.argv[2]);
const browser = await chromium.launch();
const problems = [];

async function open(width, path) {
  const context = await browser.newContext({ viewport: { width, height: 900 }, colorScheme: "light", reducedMotion: "reduce" });
  const { hostname } = new URL(input.origin);
  await context.addCookies(input.cookies.map((cookie) => ({ name: cookie.name, value: cookie.value, domain: hostname, path: "/" })));
  const page = await context.newPage();
  page.on("console", (message) => { if (message.type() === "error") problems.push(`console: ${message.text()}`); });
  // A link's prefetch the page drops when it moves on, or when the context closes, is aborted, not failed.
  page.on("requestfailed", (request) => {
    if (request.failure()?.errorText !== "net::ERR_ABORTED") problems.push(`request failed: ${request.url()} ${request.failure()?.errorText}`);
  });
  await page.goto(input.origin + path, { waitUntil: "networkidle" });
  return { context, page };
}

try {
  if (input.step === "read") {
    const texts = {};
    for (const width of input.widths ?? [1280, 390]) {
      const { context, page } = await open(width, input.path);
      // The page has read its answer when its loading line is gone.
      await page.getByText(/^Loading/).first().waitFor({ state: "detached", timeout: 15000 }).catch(() => {});
      await page.waitForTimeout(500);
      texts[width] = await page.locator("main").innerText();
      const sideways = await page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);
      if (sideways > 1) problems.push(`${input.name} at ${width}: ${sideways} px of sideways scroll`);
      await page.screenshot({ path: join(input.out, `${input.name}-${width}.png`), fullPage: true });
      await context.close();
    }
    console.log(JSON.stringify({ texts, problems }));
  } else if (input.step === "stop") {
    const { context, page } = await open(1280, "/settings/sandboxes");
    await page.getByRole("button", { name: `More actions for ${input.email}'s sandbox` }).click();
    await page.getByRole("menuitem", { name: `Stop ${input.email}'s sandbox now` }).click();
    const dialog = page.getByRole("dialog", { name: `Stop ${input.email}'s sandbox?` });
    await dialog.waitFor();
    const asked = await dialog.innerText();
    await page.screenshot({ path: join(input.out, "stop-dialog-1280.png") });
    await dialog.getByRole("button", { name: "Stop sandbox" }).click();
    const said = page.getByText(`Stopping ${input.email}'s sandbox.`);
    await said.waitFor({ timeout: 10000 });
    const text = await said.first().innerText();
    await context.close();
    console.log(JSON.stringify({ said: text, asked, problems }));
  } else {
    throw new Error(`unknown step ${input.step}`);
  }
} finally {
  await browser.close();
}

import { randomUUID } from "node:crypto";

import {
  expect,
  test,
  type ConsoleMessage,
  type Locator,
  type Page,
  type Request,
  type Response,
} from "@playwright/test";

import { capture } from "./evidence";
import { openFilesPanel } from "./files-panel";
import { afterStart, budget } from "./stack-start";
import { bootstrap, signIn } from "./ux/flow";

// WEB-E2E-122 (18.0 previews for real sites, GW-I-321 and WEB-U-470): on the previews stack the agent writes, beside the
// previews, the owner's sg-weather-map (six files), a built-shape module-script site, the two freeze pages of 12 Nov
// 2026 and a page that reaches for the witness. Each page is framed from its folder served as a site by a pass, so its
// own styles and scripts load; a built site's route answers its page; what the page would load from outside is named
// as blocked by the network setting; a page that keeps the frame busy or retries without pause never slows the app, and
// Stop scripts reloads it inert; nothing a site page tries leaves the browser.
test.skip(!process.env.TQA_STACK_URL, "Requires the owned previews demo stack");
test.beforeAll(async ({ browser, baseURL }) => {
  if (!baseURL) throw new Error("The driver must supply the stack origin");
  const page = await browser.newPage({ baseURL });
  try {
    await bootstrap(page);
  } finally {
    await page.close();
  }
});

const witness = process.env.TQA_WITNESS_URL ?? "";
const step = (title: string) => test.step(title, () => undefined);
const PASS_URL =
  /\/api\/v1\/conversations\/[0-9a-f-]{36}\/site\/[A-Za-z0-9_-]{43}\/[^/]+$/;

/** Opens a file under the site folder in the Files panel, from wherever the panel is: the root, then each folder. */
async function openSite(panel: Locator, folders: string[], name: string) {
  const path = panel.getByRole("navigation", { name: "Folder path" });
  if (await path.getByRole("button", { name: "Workspace" }).isVisible())
    await path.getByRole("button", { name: "Workspace" }).click();
  for (const folder of folders)
    await panel.getByRole("button", { name: folder, exact: true }).click();
  await panel
    .getByRole("button", { name: new RegExp(`^${name.replace(".", "\\.")}`) })
    .click();
  return panel.getByRole("region", { name });
}

/** The longest gap between two animation frames of the app's own page over 2.5 s: its main thread's worst stall. */
function worstStall(page: Page) {
  return page.evaluate(
    () =>
      new Promise<number>((resolve) => {
        let worst = 0;
        let last = performance.now();
        const until = last + 2500;
        const tick = () => {
          const now = performance.now();
          worst = Math.max(worst, now - last);
          last = now;
          if (now < until) requestAnimationFrame(tick);
          else resolve(worst);
        };
        requestAnimationFrame(tick);
      }),
  );
}

async function witnessed(page: Page): Promise<string[]> {
  const answer = await page.request.get(`${witness}/log`);
  expect(answer.status()).toBe(200);
  return (await answer.json()) as string[];
}

test("WEB-E2E-122 real sites in the Files panel: the owner's map, a built site, the freeze pages", async ({
  page,
  baseURL,
  browserName,
}) => {
  test.setTimeout(budget(1, 300_000));
  if (!baseURL) throw new Error("The driver must supply the stack origin");
  if (!witness) throw new Error("The driver must supply the witness server");

  await step("the agent writes the sites");
  await signIn(page);
  const created = await page.request.post("/api/v1/conversations", {
    headers: { "Idempotency-Key": randomUUID(), Origin: baseURL },
    data: { title: "Sites", model_route: "local" },
  });
  expect(created.status()).toBe(201);
  const task = (await created.json()) as { id: string };
  await page.goto(`/c/${task.id}`);
  await page
    .getByLabel("Message", { exact: true })
    .fill("Make the preview files");
  await page.getByRole("button", { name: "Send", exact: true }).click();
  await expect(page.getByLabel("Run status")).toHaveText(/^Done/, {
    timeout: afterStart(90_000),
  });
  const panel = await openFilesPanel(page);
  await panel.getByRole("button", { name: "Refresh files" }).click();

  await step(
    "the owner's weather map: framed from its folder, styled, its outside hosts named as blocked",
  );
  const map = await openSite(panel, ["site", "sg-weather-map"], "index.html");
  const frame = map.getByTitle("Preview of index.html");
  await expect(frame).toHaveAttribute("src", PASS_URL);
  await expect(frame).toHaveAttribute("sandbox", "allow-scripts");
  const inside = map.frameLocator("iframe");
  await expect(inside.locator("h1")).toContainText("Singapore Weather");
  // Its stylesheet and scripts came from the folder: the panel has the sheet's corner radius, and config.js ran. (The sheet's
  // rules can't be read across the frame's opaque origin; its effect can.)
  await expect(inside.locator("#panel")).toHaveCSS("border-radius", "14px");
  await expect
    .poll(() =>
      inside.locator("body").evaluate(
        () =>
          (
            window as unknown as {
              SG_WEATHER_CONFIG?: { REFRESH_MINUTES: number };
            }
          ).SG_WEATHER_CONFIG?.REFRESH_MINUTES,
      ),
    )
    .toBe(5);
  await expect(
    map.getByText(/^Blocked by the network setting: .*unpkg\.com/),
  ).toBeVisible();
  await expect(
    map.getByText("Scripts are running in a sandbox", { exact: true }),
  ).toBeVisible();
  await capture(page, "previews-site-weather-map");

  await step(
    "a built site: module scripts and styles by their types, a route answered by its page",
  );
  // What the frame's page said and loaded, so a module that doesn't run says why.
  const said: string[] = [];
  const note = (line: string) => {
    if (said.length < 200) said.push(line);
  };
  const onConsole = (message: ConsoleMessage) =>
    note(`${message.type()}: ${message.text()}`);
  const onResponse = (response: Response) => {
    if (response.url().includes("/site/"))
      note(
        `${response.status()} ${response.request().resourceType()} ${new URL(response.url()).pathname.split("/").slice(-2).join("/")} ${response.headers()["content-type"] ?? ""}`,
      );
  };
  const onFailed = (request: Request) => {
    if (request.url().includes("/site/"))
      note(
        `failed ${request.url().split("/").slice(-2).join("/")}: ${request.failure()?.errorText ?? ""}`,
      );
  };
  page.on("console", onConsole);
  page.on("response", onResponse);
  page.on("requestfailed", onFailed);
  const built = await openSite(panel, ["site", "dist"], "index.html");
  const builtInside = built.frameLocator("iframe");
  await expect(builtInside.locator("#title"), said.join("\n")).toHaveText(
    "Built site: home",
    { timeout: 15_000 },
  );
  await expect
    .poll(() => said.join("\n"), { timeout: 15_000 })
    .toMatch(/200 script .*index-7f3a9c\.js/);
  // The retrying page floods the console: the collector is not kept past this step.
  page.off("console", onConsole);
  page.off("response", onResponse);
  page.off("requestfailed", onFailed);
  await expect(builtInside.locator("#title")).toHaveCSS(
    "color",
    "rgb(0, 128, 128)",
  );
  await builtInside.locator("#about").click();
  await expect(builtInside.locator("#title")).toHaveText("Built site: about");
  await expect(
    built.getByText(/^Links in a preview don't open here/),
  ).toHaveCount(0);

  await step(
    "a page busy for 8 s never stalls the app; Stop scripts reloads it inert",
  );
  const busy = await openSite(panel, ["site", "freeze"], "busy.html");
  const busyInside = busy.frameLocator("iframe");
  // Measured at once, while the frame's thread is held by its loop (a locator inside the frame would wait for the
  // loop's end): the app's own frames keep coming.
  const stall = await worstStall(page);
  if (browserName === "webkit")
    // The known limit (the 18.0.0 changelog's known limits): WebKit runs the
    // preview on the app's own thread, so its 8 s loop holds the app for 8 s. Expected here, and said if it ever
    // stops: then this branch goes and WebKit is held to the same bound. Chromium is held to it now.
    expect(
      stall,
      "WebKit no longer stalls on a busy preview: drop the known-limit branch",
    ).toBeGreaterThanOrEqual(500);
  else expect(stall).toBeLessThan(500);
  await expect(busyInside.locator("#s")).toHaveText(/^done \d+$/, {
    timeout: 15_000,
  });
  await busy.getByRole("button", { name: "Stop scripts" }).click();
  await expect(busy.getByTitle("Preview of busy.html")).toHaveAttribute(
    "sandbox",
    "",
  );
  await expect(
    busy.getByText("Scripts are stopped", { exact: true }),
  ).toBeVisible();
  await page.waitForTimeout(1000);
  await expect(busyInside.locator("#s")).toHaveText("busy…");

  await step(
    "a page that retries failed requests without pause never stalls the app either",
  );
  const retry = await openSite(panel, ["site", "freeze"], "retry.html");
  const retryInside = retry.frameLocator("iframe");
  // Nothing is read inside the frame while it storms (its own process never yields to a reader); the app's frames
  // keep coming, and Stop scripts ends the storm with the frame.
  expect(await worstStall(page)).toBeLessThan(500);
  await retry.getByRole("button", { name: "Stop scripts" }).click();
  await expect(retry.getByTitle("Preview of retry.html")).toHaveAttribute(
    "sandbox",
    "",
  );
  await expect(retryInside.locator("#o")).toHaveText("Loading…");
  expect(await worstStall(page)).toBeLessThan(500);
  await capture(page, "previews-site-scripts-stopped");

  await step(
    "nothing a site page tries leaves the browser: the witness saw no leak",
  );
  const leak = await openSite(panel, ["site", "leak"], "index.html");
  await expect(leak.frameLocator("iframe").locator("#leak")).toHaveText(
    "leak page",
  );
  await page.waitForTimeout(2000);
  expect(
    (await witnessed(page)).filter((path) => path.includes("site-leak")),
  ).toEqual([]);
  expect(new URL(page.url()).pathname).toBe(`/c/${task.id}`);
});

// WEB-E2E-140 (18.1, built web apps preview as they are; GW-U-223, GW-U-224, WEB-U-500): a default Vite build names
// its files from the server's root. The gateway moves its page's and stylesheet's root paths under the pass, so it
// draws, styled; its logo is a root-path string in its script, which is never rewritten, so the panel names it and says
// what to ask. An app that draws nothing says so, and a static export follows its link to its second page.
test("WEB-E2E-140 a built web app previews as it is, and the panel says what it couldn't load", async ({
  page,
  baseURL,
}) => {
  test.setTimeout(budget(1, 240_000));
  if (!baseURL) throw new Error("The driver must supply the stack origin");

  await step("the agent writes the sites");
  await signIn(page);
  const created = await page.request.post("/api/v1/conversations", {
    headers: { "Idempotency-Key": randomUUID(), Origin: baseURL },
    data: { title: "Built apps", model_route: "local" },
  });
  expect(created.status()).toBe(201);
  const task = (await created.json()) as { id: string };
  await page.goto(`/c/${task.id}`);
  await page
    .getByLabel("Message", { exact: true })
    .fill("Make the preview files");
  await page.getByRole("button", { name: "Send", exact: true }).click();
  await expect(page.getByLabel("Run status")).toHaveText(/^Done/, {
    timeout: afterStart(90_000),
  });
  const panel = await openFilesPanel(page);
  await panel.getByRole("button", { name: "Refresh files" }).click();

  await step(
    "a default Vite build draws, styled; the logo its script names at the root is named on the line",
  );
  const vite = await openSite(panel, ["site", "vite-default"], "index.html");
  const inside = vite.frameLocator("iframe");
  await expect(inside.locator("#title")).toHaveText("Built app", {
    timeout: 15_000,
  });
  await expect(inside.locator("#title")).toHaveCSS("color", "rgb(0, 128, 128)");
  await expect(
    vite.getByText(
      "This page asks for files at the server's root (/assets/logo-9f8e7d.png), which a preview can't serve. Ask the agent to build it with relative paths.",
    ),
  ).toBeVisible({ timeout: 15_000 });
  await capture(page, "previews-built-vite-default");

  await step("an app that draws nothing says so");
  const empty = await openSite(panel, ["site", "empty-app"], "index.html");
  await expect(
    empty.getByText(
      "This page drew nothing. An app with routes previews with a hash router: ask the agent to use one.",
    ),
  ).toBeVisible({ timeout: 15_000 });
  await capture(page, "previews-built-empty");

  await step(
    "an app that keeps its data in localStorage draws and adds through its form (the sandbox refuses both; the preview stands in)",
  );
  const stored = await openSite(panel, ["site", "storage-app"], "index.html");
  await expect(stored.frameLocator("iframe").locator("#todos li")).toHaveText(
    ["Buy coffee beans"],
    { timeout: 15_000 },
  );
  await expect(stored.getByText(/^This page /)).toHaveCount(0);
  // Its form adds an item: the sandbox blocks forms, so the browser never fires submit; the preview fires it.
  const app = stored.frameLocator("iframe");
  await app.locator("#item").fill("Water the plants");
  await app.getByRole("button", { name: "Add" }).click();
  await expect(app.locator("#todos li")).toHaveText([
    "Buy coffee beans",
    "Water the plants",
  ]);
  await capture(page, "previews-built-storage");

  await step("an app that stops on an error gets no router advice");
  const stopped = await openSite(panel, ["site", "error-app"], "index.html");
  // The reporter waits 2 s after load before it judges an empty page; give it that and more.
  await page.waitForTimeout(3_500);
  await expect(stopped.getByText(/^This page drew nothing/)).toHaveCount(0);

  await step("a static export follows its link to its second page");
  const exported = await openSite(panel, ["site", "export"], "index.html");
  const pages = exported.frameLocator("iframe");
  await expect(pages.locator("#title")).toHaveText("Home");
  await pages.locator("#about").click();
  await expect(pages.locator("#title")).toHaveText("About us");
  await expect(exported.getByText(/^This page /)).toHaveCount(0);
  await expect(
    exported.getByText(/^Links in a preview don't open here/),
  ).toHaveCount(0);
});

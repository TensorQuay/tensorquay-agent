import { randomUUID } from "node:crypto";
import { readFileSync } from "node:fs";
import { mkdir } from "node:fs/promises";
import { join } from "node:path";

import AxeBuilder from "@axe-core/playwright";

import { settled } from "./motion";
import { signIn } from "./session";
import { expect, resetFront, test, type Page } from "./test";

// Read, not imported: Playwright loads specs as ES modules, which refuse a JSON import without an attribute. Copy
// that a branch may reword (the Limits hint) is read from it, so the spec holds whichever wording is merged.
const en = JSON.parse(readFileSync("src/i18n/locales/en.json", "utf8")) as {
  messages: Record<string, string>;
};

// WEB-E2E-037 (ADR-0039 §5 and §6): a desktop task's Screen panel on the fake front's screen, which relays as the
// gateway does (view-only until Take over, control frames, close codes): watch, take over, input reaching the desktop
// only while this page holds it, hand back, and the desktop stopping under the panel; at 390 and 1280, light and dark.

async function serious(page: Page) {
  await settled(page);
  const { violations } = await new AxeBuilder({ page }).analyze();
  return violations
    .filter((violation) =>
      ["serious", "critical"].includes(violation.impact ?? ""),
    )
    .map((violation) => violation.id);
}

async function capture(page: Page, name: string) {
  const directory = process.env.TQA_WEB_EVIDENCE_DIR;
  if (!directory) return;
  await settled(page);
  await mkdir(directory, { recursive: true });
  await page.screenshot({ path: join(directory, `screen-${name}.png`) });
}

/** A new task on the Desktop environment, which the "desktop" scenario offers. */
async function desktopTask(page: Page): Promise<string> {
  const response = await page.request.post("/api/v1/conversations", {
    headers: { "Idempotency-Key": randomUUID() },
    data: {
      title: "Fill in the form",
      model_route: "local",
      template: "desktop",
    },
  });
  expect(response.status()).toBe(201);
  const { id } = (await response.json()) as { id: string };
  return id;
}

/** What the fake screen saw: who holds it, and the input that got through. */
async function seen(page: Page, id: string) {
  const response = await page.request.get(
    `/__fake/screen?conversation=${encodeURIComponent(id)}`,
  );
  return (await response.json()) as { holder: string; inputs: string[] };
}

test.beforeEach(async ({ request }) => {
  await resetFront(request, ["desktop"]);
});

for (const width of [390, 1280] as const)
  for (const colorScheme of ["light", "dark"] as const)
    test(`WEB-E2E-037 a desktop task's screen: watch, take over, hand back (${width} ${colorScheme})`, async ({
      page,
    }) => {
      await page.setViewportSize({ width, height: width === 390 ? 844 : 800 });
      await page.emulateMedia({ colorScheme });
      await signIn(page);
      const id = await desktopTask(page);
      await page.goto(`/c/${encodeURIComponent(id)}`);
      // Nothing has acted on the desktop yet, so the screen is opened by the person, from the bar.
      await page.getByRole("button", { name: "Screen", exact: true }).click();
      const panel = page.getByRole("region", { name: "Screen" });
      await expect(panel.getByRole("status").first()).toHaveText("Watching.");
      const picture = panel.getByRole("img", {
        name: "The agent's desktop, live",
      });
      // noVNC draws two canvases, a 0×0 one for the cursor first, then the picture.
      await expect(picture.locator("canvas").last()).toBeVisible();
      expect(await serious(page)).toEqual([]);
      await capture(page, `watching-${width}-${colorScheme}`);

      // View-only: a click on the picture never reaches the desktop.
      await picture.click();
      expect((await seen(page, id)).inputs).toEqual([]);

      await panel.getByRole("button", { name: "Take over" }).click();
      await expect(panel.getByRole("status").first()).toHaveText(
        "You have control. When you're done, hand back, then send a message to let the agent carry on.",
      );
      // In control the picture is an application, as a screen reader passes keys to one (the c16 a11y pass).
      const controlled = panel.getByRole("application", {
        name: "The agent's desktop, live, you have control",
      });
      await controlled.click();
      await expect
        .poll(async () => (await seen(page, id)).inputs)
        .toContain("pointer");
      expect(await serious(page)).toEqual([]);
      await capture(page, `control-${width}-${colorScheme}`);

      await panel.getByRole("button", { name: "Hand back" }).click();
      await expect(panel.getByRole("status").first()).toHaveText(
        "You handed back the desktop. Send a message to let the agent carry on.",
      );
      const before = (await seen(page, id)).inputs.length;
      await picture.click();
      expect((await seen(page, id)).inputs).toHaveLength(before);

      // The desktop stops under the panel: it says so, offers no Take over, and nothing scrolls sideways.
      await page.request.post(
        `/__fake/screen?conversation=${encodeURIComponent(id)}`,
        { data: { refuse: "sandbox_not_running" } },
      );
      await expect(panel.getByRole("status").first()).toHaveText(
        "The desktop isn't running. It starts when you send a message.",
      );
      await expect(
        panel.getByRole("button", { name: "Take over" }),
      ).toHaveCount(0);
      expect(
        await page.evaluate(
          () => document.documentElement.scrollWidth <= window.innerWidth,
        ),
      ).toBe(true);
      await capture(page, `stopped-${width}-${colorScheme}`);
    });

// A restart under a Take over: the holder's socket goes with the old process, the new one
// records the hand-back the restart cut short before it listens (ended restart, no time), and the page, back on its streams, reads it:
// the panel watches again without control, offers Take over, and the transcript says the desktop was handed back.
test("WEB-E2E-037 a Take over cut short by a gateway restart is handed back", async ({
  page,
}) => {
  await signIn(page);
  const id = await desktopTask(page);
  await page.goto(`/c/${encodeURIComponent(id)}`);
  await page.getByRole("button", { name: "Screen", exact: true }).click();
  const panel = page.getByRole("region", { name: "Screen" });
  await expect(panel.getByRole("status").first()).toHaveText("Watching.");
  await panel.getByRole("button", { name: "Take over" }).click();
  await expect(panel.getByRole("status").first()).toHaveText(
    "You have control. When you're done, hand back, then send a message to let the agent carry on.",
  );
  expect((await seen(page, id)).holder).toBe("someone");

  const restarted = await page.request.post(
    `/__fake/restart?conversation=${encodeURIComponent(id)}`,
  );
  expect(restarted.status()).toBe(204);
  // Back without control, as the restarted gateway gives it: Take over is offered again.
  await expect(panel.getByRole("status").first()).toHaveText("Watching.", {
    timeout: 15_000,
  });
  await expect(panel.getByRole("button", { name: "Take over" })).toBeVisible();
  await expect(panel.getByRole("button", { name: "Hand back" })).toHaveCount(0);
  expect((await seen(page, id)).holder).toBe("none");
  // The transcript reads the stored notices again once its stream is back: the Take over and its hand-back.
  await expect(page.getByText("You took over the desktop.")).toBeVisible();
  await expect(
    page.getByText("The desktop was handed back when the server restarted."),
  ).toBeVisible();
  expect(await serious(page)).toEqual([]);
  await capture(page, "restart-handed-back");
});

test("WEB-E2E-037 a task without a desktop has no Screen", async ({ page }) => {
  await signIn(page);
  const response = await page.request.post("/api/v1/conversations", {
    headers: { "Idempotency-Key": randomUUID() },
    data: { title: "Plain", model_route: "local" },
  });
  const { id } = (await response.json()) as { id: string };
  await page.goto(`/c/${encodeURIComponent(id)}`);
  // The task header's toggles (the composer's Attach menu has a Files item too).
  const header = page.getByRole("region", { name: "Task header" });
  await expect(header.getByRole("button", { name: "Files" })).toBeVisible();
  await expect(
    header.getByRole("button", { name: "Screen", exact: true }),
  ).toHaveCount(0);
});

// WEB-E2E-037, the record (ADR-0039 §5 and §9, revision 14): a desktop run's actions name their element, their cards
// show the pictures the gateway serves, and the Screen tab replays them; at 390 and 1280.
for (const width of [390, 1280] as const)
  test(`WEB-E2E-037 a desktop run's pictures and its replay (${width})`, async ({
    page,
  }) => {
    await page.setViewportSize({ width, height: width === 390 ? 844 : 800 });
    await signIn(page);
    const id = await desktopTask(page);
    await page.goto(`/c/${encodeURIComponent(id)}`);
    const message = page.getByLabel("Message", { exact: true });
    await message.fill("Act on the desktop");
    await message.press("Enter");
    // The first desktop action opens the Screen tab by itself, once (WEB-U-243). On a phone the panel lies over the
    // transcript, so once the run is done a self-opened, untouched panel gives it back by itself (WEB-U-332); the
    // person opens it again below.
    const panel = page.getByRole("region", { name: "Screen" });
    if (width === 390) {
      await expect(page.getByLabel("Run status")).toHaveText(/^Done/);
      await expect(panel).toBeHidden();
    } else await expect(panel).toBeVisible();
    await page.getByRole("button", { name: /Worked for/ }).click();
    const click = page.getByRole("button", { name: /^Clicked “Save”/ });
    await expect(click).toBeVisible();
    await click.click();
    const before = page.getByRole("img", { name: "Before" });
    await expect(before).toBeVisible();
    // The picture loaded from the gateway's route, in the room its size reserves.
    expect(
      await before.evaluate((image: HTMLImageElement) => image.naturalWidth),
    ).toBeGreaterThan(0);
    await expect(page.getByRole("img", { name: "After" })).toBeVisible();

    if (width === 390)
      await page.getByRole("button", { name: "Screen", exact: true }).click();
    await panel.getByRole("button", { name: "Replay" }).click();
    await expect(
      panel.getByText(/^Step 2 of 2 · Took a screenshot/),
    ).toBeVisible();
    await expect(
      panel.getByText("The numbers mark the elements the agent could act on."),
    ).toBeVisible();
    await panel.getByRole("button", { name: "Previous step" }).click();
    await expect(
      panel.getByText(/^Step 1 of 2 · Clicked “Save”/),
    ).toBeVisible();
    expect(await serious(page)).toEqual([]);
    expect(
      await page.evaluate(
        () => document.documentElement.scrollWidth <= window.innerWidth,
      ),
    ).toBe(true);
    await capture(page, `replay-${width}`);
  });

// WEB-E2E-037, the Desktop's limits (ADR-0039 §9-10, GW-U-146): their own section, the range, and the order refusal.
test("WEB-E2E-037 Settings › Limits › Desktop", async ({ page }) => {
  await signIn(page);
  await page.goto("/settings/limits");
  const desktop = page.getByRole("table", { name: "Desktop limits" });
  await expect(
    desktop.getByRole("row", { name: /Desktop environments at once/ }),
  ).toContainText("2 tasks");
  await expect(
    desktop.getByRole("row", { name: /Keep action screenshots for/ }),
  ).toContainText("30 days");
  // The computer tool's caps per run, beside them, and when a change applies (WEB-U-280): the hint's words from the
  // catalogue, whichever wording the branch has.
  const applies = en.messages["limits.appliesNextDesktop"] ?? "";
  expect(applies).not.toBe("");
  const screenshots = desktop.getByRole("row", {
    name: /Screenshots a run may send the model/,
  });
  await expect(screenshots).toContainText("200 screenshots");
  await expect(screenshots).toContainText(applies);
  const actions = desktop.getByRole("row", {
    name: /Desktop actions a run may take/,
  });
  await expect(actions).toContainText(/1.000 actions/);
  await expect(actions).toContainText(applies);
  await page
    .getByRole("button", { name: "Edit Desktop environments at once" })
    .click();
  const dialog = page.getByRole("dialog");
  await dialog.getByRole("spinbutton").fill("9");
  await dialog.getByRole("button", { name: "Save limit" }).click();
  await expect(
    dialog.getByText(
      "Desktop environments at once can't be more than Tasks running at once, which is 8 now.",
    ),
  ).toBeVisible();
  expect(await serious(page)).toEqual([]);
  await capture(page, "limits-desktop");
});

// WEB-E2E-037, a run whose pictures weren't all kept (WEB-U-282, WEB-U-241): a screenshot the store didn't keep reads
// "What the model saw" with "This picture couldn't be saved." (from its evidence, never a missing picture), and a screenshot refused as too
// detailed says so on its line, never "Took a screenshot".
test("WEB-E2E-037 a screenshot not kept, and one refused as too detailed", async ({
  page,
}) => {
  await signIn(page);
  const id = await desktopTask(page);
  await page.goto(`/c/${encodeURIComponent(id)}`);
  const message = page.getByLabel("Message", { exact: true });
  await message.fill("Act on the desktop, some pictures lost");
  await message.press("Enter");
  await page.getByRole("button", { name: /Worked for/ }).click();
  await page.getByRole("button", { name: /^Took a screenshot/ }).click();
  const figure = page.locator("figure", { hasText: "What the model saw" });
  await expect(figure).toContainText("This picture couldn't be saved.");
  await expect(page.getByText("After", { exact: true })).toHaveCount(0);
  await expect(
    page.getByText("No picture this time: the screen was too detailed to send"),
  ).toBeVisible();
  expect(await serious(page)).toEqual([]);
});

// WEB-E2E-037, Expand in control at a laptop's height (the 16d page pass, Z14): at 1280×800 the picture filled the
// dialog's width at 16:10 and pushed Hand back, ⋯ and the lines below the fold. The whole dialog fits without scrolling:
// the status, the picture, its controls and the lines, at each size; the picture keeps a usable size.
for (const [width, height] of [
  [1280, 800],
  [1280, 720],
  [1440, 900],
  [390, 844],
] as const)
  test(`WEB-E2E-037 Expand in control keeps its controls in view (${width}×${height})`, async ({
    page,
  }) => {
    await page.setViewportSize({ width, height });
    await signIn(page);
    const id = await desktopTask(page);
    await page.goto(`/c/${encodeURIComponent(id)}`);
    await page.getByRole("button", { name: "Screen", exact: true }).click();
    const panel = page.getByRole("region", { name: "Screen" });
    await panel.getByRole("button", { name: "Take over" }).click();
    await expect(panel.getByRole("status").first()).toHaveText(
      /^You have control\./,
    );
    await panel.getByRole("button", { name: "Expand" }).click();
    const dialog = page.getByRole("dialog", { name: "Screen" });
    const picture = dialog.getByRole("application", {
      name: "The agent's desktop, live, you have control",
    });
    await expect(picture.locator("canvas").last()).toBeVisible();
    await settled(page);
    const content = dialog.locator(".dialog-content");
    expect(
      await content.evaluate(
        (element) => element.scrollHeight <= element.clientHeight + 1,
      ),
    ).toBe(true);
    const shown = await content.boundingBox();
    if (!shown) throw new Error("The dialog's content has no box");
    for (const part of [
      dialog.getByRole("status").first(),
      picture,
      dialog.getByRole("button", { name: "Hand back" }),
      dialog.getByRole("button", { name: "More actions for the screen" }),
      dialog.getByText(/^Keys go to the desktop while it has focus/),
      dialog.getByText(/^A sign-in here lasts for the agent's next run only/),
    ]) {
      const box = await part.boundingBox();
      expect(box, String(part)).not.toBeNull();
      if (!box) continue;
      expect(box.y + box.height, `${String(part)} in view`).toBeLessThanOrEqual(
        shown.y + shown.height + 1,
      );
      expect(box.y, `${String(part)} in view`).toBeGreaterThanOrEqual(
        shown.y - 1,
      );
    }
    // Still a picture to work in: at least half the dialog's width, or 320 px wide on a phone.
    const box = await picture.boundingBox();
    expect(box?.width ?? 0).toBeGreaterThanOrEqual(
      Math.min(shown.width / 2, 320),
    );
    await capture(page, `expanded-control-${width}x${height}`);
  });

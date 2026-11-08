import { addFromPlus, openChoices } from "./choices";
import { signIn } from "./session";
import { expect, test, type Locator, type Page } from "./test";

// The task header with the side panel open (UI-GUIDELINES §6): the title and the profile label give way (truncated or
// wrapped), and never overlap the run status or the header's controls (its ⋯ among them), at 1280
// and 1024, for a finished task from a profile and for a failed one.
type Box = { x: number; y: number; width: number; height: number };
const overlap = (a: Box, b: Box) =>
  a.x < b.x + b.width &&
  b.x < a.x + a.width &&
  a.y < b.y + b.height &&
  b.y < a.y + a.height;
async function box(locator: Locator): Promise<Box> {
  const found = await locator.boundingBox();
  if (!found) throw new Error("The header part must be visible");
  return found;
}
/** The visible part of an element: its box cut by every ancestor that clips. */
async function shown(locator: Locator): Promise<Box> {
  return locator.evaluate((element) => {
    let { left, top, right, bottom } = element.getBoundingClientRect();
    for (
      let parent = element.parentElement;
      parent;
      parent = parent.parentElement
    ) {
      if (getComputedStyle(parent).overflow === "visible") continue;
      const clip = parent.getBoundingClientRect();
      left = Math.max(left, clip.left);
      top = Math.max(top, clip.top);
      right = Math.min(right, clip.right);
      bottom = Math.min(bottom, clip.bottom);
    }
    return {
      x: left,
      y: top,
      width: Math.max(0, right - left),
      height: Math.max(0, bottom - top),
    };
  });
}

async function checkHeader(page: Page, origin: string | null) {
  const bar = page.getByRole("region", { name: "Task header" });
  const title = bar.getByRole("heading", { level: 1 });
  const status = page.getByLabel("Run status");
  const pickers = bar.locator(".conversation-pickers");
  const parts: [string, Box][] = [
    ["title", await shown(title)],
    ["status", await box(status)],
    ["pickers", await box(pickers)],
  ];
  if (origin)
    parts.push(["origin", await shown(bar.getByText(origin, { exact: true }))]);
  for (const [index, [name, part]] of parts.entries())
    for (const [other, next] of parts.slice(index + 1))
      if (part.width > 0 && next.width > 0)
        expect(overlap(part, next), `${name} overlaps ${other}`).toBe(false);
  // The ⋯ (the keyboard's way to rename) is whole and on screen, never clipped away.
  const more = bar.getByRole("button", { name: /^More actions for / });
  const moreShown = await shown(more);
  const moreBox = await box(more);
  expect(moreShown.width).toBeCloseTo(moreBox.width, 0);
  // The title keeps some room: at least a few characters, with the rest in its tooltip.
  expect((await shown(title)).width).toBeGreaterThan(40);
  // The run status gives way after the title and before the controls (found before 17.0, WEB-U-112):
  // it keeps room for its first word ("Done"), and has its tooltip (the whole text, or how long a finished run took).
  expect((await shown(status)).width).toBeGreaterThan(20);
  await expect(status).toHaveAttribute("title", /\S/);
  // Nothing runs past the bar's right edge.
  const edge = await box(bar);
  const last = await box(pickers);
  expect(last.x + last.width).toBeLessThanOrEqual(edge.x + edge.width + 1);
}

for (const width of [1280, 1024] as const)
  test(`the task header gives way without overlapping at ${width}`, async ({
    page,
  }) => {
    await page.setViewportSize({ width, height: 900 });
    await signIn(page);
    await page.goto("/");
    // "+" adds the profile chip, its picker open (the UI standard's composer).
    await addFromPlus(page, "Agent profile…");
    await page.getByRole("menuitemradio", { name: /Team default/ }).click();
    const message = page.getByLabel("Message", { exact: true });
    await message.fill("Summarize the workspace and list every file it holds.");
    await message.press("Enter");
    await expect(page.getByLabel("Run status")).toHaveText(/^Done/, {
      timeout: 60_000,
    });
    // The side panel open: below 1440 it overlays the page under the bar, so the header keeps its room.
    const toggle = page
      .getByRole("region", { name: "Task header" })
      .getByRole("button", { name: "Files", exact: true });
    if ((await toggle.getAttribute("aria-expanded")) !== "true")
      await toggle.click();
    await expect(toggle).toHaveAttribute("aria-expanded", "true");
    await checkHeader(page, "Agent: Team default");
    // A picker the profile fixes looks disabled beside one it leaves free (in Change).
    await openChoices(page);
    const look = (name: RegExp) =>
      page.getByRole("button", { name }).evaluate((button) => {
        const style = getComputedStyle(button);
        return {
          color: style.color,
          border: style.borderStyle,
          chevron: getComputedStyle(
            button.querySelector("svg:last-child") ?? button,
          ).display,
        };
      });
    const fixed = await look(/^Model: /);
    const free = await look(/^Environment: /);
    await expect(
      page.getByRole("button", { name: /^Model: / }),
    ).toHaveAttribute("aria-disabled", "true");
    expect(fixed.color).not.toBe(free.color);
    expect(fixed.border).toBe("dashed");
    expect(fixed.chevron).toBe("none");

    await page.goto("/");
    await message.fill("Fail at the provider");
    await message.press("Enter");
    await expect(page.getByLabel("Run status")).toHaveText(/^Failed/, {
      timeout: 60_000,
    });
    if ((await toggle.getAttribute("aria-expanded")) !== "true")
      await toggle.click();
    await checkHeader(page, null);
  });

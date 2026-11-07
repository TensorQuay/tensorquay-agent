import { expect, type Page } from "@playwright/test";

import { phase } from "./observer";

/** A deliberate scroll must pause following without losing the newly arrived event count. */
export async function pauseAndResume(page: Page) {
  const region = page.getByRole("region", { name: "Transcript", exact: true });
  await expect
    .poll(
      () =>
        region.evaluate(
          (element) => element.scrollHeight - element.clientHeight,
        ),
      { message: "follow: the live scenario must overflow" },
    )
    .toBeGreaterThan(100);
  await expect(page.getByLabel("Run status")).toHaveText(/^Working…/);
  await phase(page, "paused");
  await region.evaluate((element) => {
    element.scrollTop = 0;
  });
  const jump = page.getByRole("button", {
    name: /Jump to latest · [1-9][\d\u202f]* new/,
  });
  await expect(
    jump,
    "follow: paused output shows a positive unseen count",
  ).toBeVisible();
  expect(
    await region.evaluate((element) => element.scrollTop),
    "follow: output must not drag a paused reader",
  ).toBe(0);
  const latest = await page.evaluate(() => window.__uxLatest());
  expect(
    latest,
    "follow: paused transcript still has a latest line",
  ).not.toBeNull();
  expect(
    latest?.bottom,
    "follow: scrolling up puts the latest line below the viewport",
  ).toBeGreaterThan(latest?.viewportBottom ?? Infinity);
  // A live transcript isn't monotonic: when a new step arrives, the previous latest tool card's open body folds
  // (finding T2), so its height drops by hundreds of pixels at each step. The room below is waited for, not raced; the
  // reader must stay where they scrolled the whole time (13h-pre, 1024 dark: 83 px read at a step boundary).
  // Bounded: 20 s of readings, every 100 ms (expect.poll, no sleep); a failure lists what it saw (room below and scroll
  // position).
  const seen: string[] = [];
  try {
    await expect
      .poll(
        async () => {
          const { room, top } = await region.evaluate((element) => ({
            room:
              element.scrollHeight - element.clientHeight - element.scrollTop,
            top: element.scrollTop,
          }));
          seen.push(`${room}/${top}`);
          return room;
        },
        { timeout: 20_000, intervals: [100] },
      )
      .toBeGreaterThan(100);
  } catch {
    throw new Error(
      `follow: a paused reader never had more than 100 px of new output below them in 20 s; room/scrollTop seen: ${seen.join(" ")}`,
    );
  }
  expect(
    await region.evaluate((element) => element.scrollTop),
    "follow: output must not drag a paused reader, also once more has arrived",
  ).toBe(0);
  await jump.click();
  await phase(page, "following");
  await expect(
    page.getByRole("button", { name: /Jump to latest/ }),
  ).toHaveCount(0);
  await expect
    .poll(
      () =>
        region.evaluate(
          (element) =>
            element.scrollHeight - element.clientHeight - element.scrollTop,
        ),
      { message: "follow: jump resumes following" },
    )
    .toBeLessThanOrEqual(2);
}

export async function markdown(page: Page) {
  const answers = page.locator(".markdown-answer");
  await expect(
    answers.locator("h2").first(),
    "markdown: semantic heading",
  ).toBeAttached();
  await expect(
    answers.locator("table").first(),
    "markdown: semantic table",
  ).toBeAttached();
  await expect(
    answers.locator("pre code").first(),
    "markdown: fenced code becomes code",
  ).toBeAttached();
  const raw = await answers.evaluateAll((elements) =>
    elements.flatMap((element) => {
      const walker = document.createTreeWalker(element, NodeFilter.SHOW_TEXT);
      const failures: string[] = [];
      for (let node = walker.nextNode(); node; node = walker.nextNode()) {
        // Source code may legitimately contain comments and fence examples.
        if (node.parentElement?.closest("code, pre")) continue;
        const text = node.textContent ?? "";
        if (/\|\s*---|^\s*#{1,6}\s|```/m.test(text))
          failures.push(text.slice(0, 120));
      }
      return failures;
    }),
  );
  expect
    .soft(
      raw,
      "markdown: no visible raw table dividers, heading hashes or fences",
    )
    .toEqual([]);
}

const summaries: Record<string, RegExp> = {
  read_file: /^Read .+(?: line \d+| lines \d+–\d+)?$/,
  write_file: /^(Created|Updated) .+ \(\+\d+ −\d+\)$|^Write .+/,
  edit_file: /^Edited .+ \(\+\d+ −\d+\)$|^Edit .+/,
  grep: /^Looked for (“.+”|a pattern) in .+ · [\d\u202f]+ (match|matches|file|files)( \(partial\))?$/,
  glob: /^Found (no|[\d\u202f]+) (\S+ )?files?( \(partial\))?$/,
  shell: /^Ran a command: .+/,
};

/** Check each displayed summary against its own recorded tool card, independently of the presenter. */
export async function activities(page: Page, scripted: boolean) {
  const group = page.locator(".activity-heading");
  await expect(
    group,
    "activity: a completed turn folds even after a recovered tool error",
  ).toHaveAttribute("aria-expanded", "false");
  await expect(
    page.locator(".markdown-answer"),
    "narration: only the final answer sits outside work",
  ).toHaveCount(1);
  const narration = page.locator(".activity-group .activity-narration");
  if (scripted)
    await expect(
      narration,
      "narration: retain all six work-step texts",
    ).toHaveCount(6);
  for (const paragraph of await narration.all())
    await expect(paragraph, "narration: folds with work").not.toBeVisible();
  await group.click();
  for (const paragraph of await narration.all())
    await expect(
      paragraph,
      "narration: available inside expanded work",
    ).toBeVisible();
  // WEB-E2E-018 (the 13f screens, smaller items; WEB-U-157 on the fake front): Thinking is a disclosure, never a bare
  // label. Each one shows its chevron, starts closed, and opens to the reasoning's own words; a turn without reasoning
  // has none, since the web renders one only for reasoning text.
  for (const thinking of await page
    .locator(".activity-group details.reasoning")
    .all()) {
    await expect
      .soft(thinking.locator("summary svg"), "thinking: shows its chevron")
      .toBeVisible();
    expect
      .soft(await thinking.getAttribute("open"), "thinking: starts closed")
      .toBeNull();
    await thinking.locator("summary").click();
    expect
      .soft(
        (await thinking.innerText()).replace(/^\s*Thinking\s*/, "").trim()
          .length,
        "thinking: opens to its reasoning's words",
      )
      .toBeGreaterThan(0);
    await thinking.locator("summary").click();
  }
  const lines = page.locator(".activity-line");
  expect(
    await lines.count(),
    "activity: exercise several real tools",
  ).toBeGreaterThanOrEqual(scripted ? 12 : 3);
  const kinds = new Set<string>();
  for (const line of await lines.all()) {
    const label = await line.locator(".activity-label").innerText();
    expect
      .soft(label, "plurals: one result takes the singular")
      .not.toMatch(/\b1 (matches|files|commands|lines)\b/);
    expect
      .soft(label, "plurals: a single read line is not a range")
      .not.toMatch(/lines (\d+)–\1\b/);
    // Open it to read its card, unless it is open already (the turn's latest step starts open, finding T2).
    const toggle = line.locator("button").first();
    const opened = (await toggle.getAttribute("aria-expanded")) !== "true";
    if (opened) await toggle.click();
    // Finding T-a: the card in a step shows no internal tool name; its accessible label still names the tool.
    const name = (
      (await line.locator(".tool-card").getAttribute("aria-label")) ?? ""
    ).replace(/ tool call$/, "");
    kinds.add(name);
    expect
      .soft(label, `activity: ${name} must be a human summary`)
      .not.toBe(name);
    expect
      .soft(label, `activity: ${name} summary pattern`)
      .toMatch(
        summaries[name] ??
          new RegExp(`^Used ${name.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}$`),
      );
    expect
      .soft(Array.from(label).length, "activity: bounded summary")
      .toBeLessThanOrEqual(180);
    // Left as it was found.
    if (opened) await toggle.click();
  }
  if (scripted)
    expect.soft([...kinds].sort()).toEqual(Object.keys(summaries).sort());
  await group.click();
}

export async function fixedShell(page: Page) {
  await phase(page, "inspecting");
  // No global header (UI-GUIDELINES §6): the one-line conversation bar and the sidebar stay put.
  const bar = await page.locator(".conversation-bar").boundingBox();
  const sidebar = await page.locator(".app-sidebar").boundingBox();
  const region = page.getByRole("region", { name: "Transcript", exact: true });
  for (const position of [0, 1]) {
    await region.evaluate((element, position) => {
      element.scrollTop = position * element.scrollHeight;
    }, position);
    expect
      .soft(
        await page.locator(".conversation-bar").boundingBox(),
        "layout: fixed conversation bar",
      )
      .toEqual(bar);
    expect
      .soft(
        await page.locator(".app-sidebar").boundingBox(),
        "layout: fixed sidebar",
      )
      .toEqual(sidebar);
    expect
      .soft(
        await page.evaluate(() => ({
          width: document.documentElement.scrollWidth,
          height: document.documentElement.scrollHeight,
        })),
        "layout: the document must fit its viewport",
      )
      .toEqual(page.viewportSize());
  }
}

/**
 * The transcript's markup for replay comparisons, taken once nothing in it is still loading: a lazily loaded part (a
 * tool card, formatted Markdown) shows a skeleton for a moment, and a snapshot in that moment differs from one taken a
 * frame later (task 093, WEB-U-037).
 */
export async function snapshot(page: Page) {
  const transcript = page.getByTestId("transcript");
  await expect(transcript.locator(".skeleton")).toHaveCount(0);
  return transcript.evaluate((element) => {
    const copy = element.cloneNode(true) as HTMLElement;
    for (const node of copy.querySelectorAll("[id], [aria-controls]")) {
      node.removeAttribute("id");
      node.removeAttribute("aria-controls");
    }
    return copy.innerHTML.replace(/\s+/g, " ").trim();
  });
}

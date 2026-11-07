import { appendFileSync } from "node:fs";

import AxeBuilder from "@axe-core/playwright";
import { expect, type Locator, type Page } from "@playwright/test";

import { settled } from "./motion";

/** The most Tab stops one walk follows; a page with more is a finding in itself (UI-GUIDELINES §9). */
export const MAX_TAB_STOPS = 80;

/** One keyboard stop: what it is, whether a reader can see it and whether it shows that it has the focus. */
export type Stop = {
  label: string;
  visible: boolean;
  /** Another element covers the stop's centre, such as a pinned composer over the transcript (WCAG 2.4.11). */
  covered: boolean;
  indicated: boolean;
  named: boolean;
};

/** Every WCAG A/AA rule axe knows, contrast included, for the page as the test left it. */
export async function axeViolations(page: Page): Promise<string[]> {
  await settled(page);
  const { violations } = await new AxeBuilder({ page })
    .withTags(["wcag2a", "wcag2aa", "wcag21a", "wcag21aa", "wcag22aa"])
    .analyze();
  return violations.map(
    (violation) =>
      `${violation.id} (${violation.impact}): ${violation.nodes
        .slice(0, 3)
        .map((node) => node.target.join(" "))
        .join(" | ")}`,
  );
}

/**
 * Walks the page with Tab from the top: each stop's accessible label, whether it is on screen with a size, whether
 * its focus is drawn (an outline or a box shadow that differs from the unfocused element), and whether it has a name.
 * Stops when focus returns to the first stop, leaves the document, or after MAX_TAB_STOPS.
 */
export async function tabWalk(
  page: Page,
  stops = MAX_TAB_STOPS,
): Promise<Stop[]> {
  // Blurring keeps the browser's Tab starting point where focus was, so the walk starts from a probe at the very top.
  await page.evaluate(() => {
    window.scrollTo(0, 0);
    const start = document.createElement("span");
    start.tabIndex = -1;
    document.body.prepend(start);
    start.focus();
    start.remove();
  });
  const walked: Stop[] = [];
  let first: string | null = null;
  for (let index = 0; index < stops; index++) {
    await page.keyboard.press("Tab");
    const stop = await page.evaluate(() => {
      const element = document.activeElement as HTMLElement | null;
      if (!element || element === document.body) return null;
      const box = element.getBoundingClientRect();
      const style = getComputedStyle(element);
      // The ring may be the element's own or its field's (a composer card draws it with :focus-within).
      const ringed = (node: Element | null, depth: number): boolean => {
        if (!node || depth > 3) return false;
        const own = getComputedStyle(node);
        return (
          (own.outlineStyle !== "none" && parseFloat(own.outlineWidth) > 0) ||
          ringed(node.parentElement, depth + 1)
        );
      };
      const outline = ringed(element, 0);
      const shadow = style.boxShadow !== "none";
      const name =
        element.getAttribute("aria-label") ??
        (element.getAttribute("aria-labelledby")
          ? document.getElementById(
              element.getAttribute("aria-labelledby") ?? "",
            )?.textContent
          : null) ??
        (element instanceof HTMLInputElement ||
        element instanceof HTMLSelectElement ||
        element instanceof HTMLTextAreaElement
          ? (element.labels?.[0]?.textContent ??
            element.getAttribute("placeholder"))
          : element.textContent) ??
        "";
      const tag = element.tagName.toLowerCase();
      const role = element.getAttribute("role") ?? tag;
      return {
        label: `${role} "${name.trim().replace(/\s+/g, " ").slice(0, 50)}"`,
        visible:
          box.width > 0 &&
          box.height > 0 &&
          box.bottom > 0 &&
          box.right > 0 &&
          box.left < innerWidth &&
          style.visibility !== "hidden",
        indicated: outline || shadow,
        covered: (() => {
          const x = Math.min(
            Math.max(box.left + box.width / 2, 0),
            innerWidth - 1,
          );
          const y = Math.min(
            Math.max(box.top + box.height / 2, 0),
            innerHeight - 1,
          );
          const top = document.elementFromPoint(x, y);
          return !!top && !element.contains(top) && !top.contains(element);
        })(),
        named: name.trim().length > 0,
      };
    });
    if (!stop) break;
    if (first === stop.label && walked.length > 1) break;
    first ??= stop.label;
    walked.push(stop);
  }
  return walked;
}

/** The problems a walk shows: stops a reader cannot see, stops without a drawn focus, stops without a name. */
export function walkProblems(walk: Stop[]): string[] {
  return walk.flatMap((stop, index) => [
    ...(stop.visible
      ? []
      : [`stop ${index + 1} ${stop.label} is focused but not visible`]),
    ...(stop.indicated
      ? []
      : [`stop ${index + 1} ${stop.label} shows no focus`]),
    ...(stop.named ? [] : [`stop ${index + 1} ${stop.label} has no name`]),
    ...(stop.visible && stop.covered
      ? [`stop ${index + 1} ${stop.label} is covered by another element`]
      : []),
  ]);
}

/**
 * With reduced motion, nothing moves: every running animation whose keyframes change position, size or rotation, and
 * every transition of such a property. A colour or ring that fades is allowed (UI-GUIDELINES §8).
 */
export async function motionProblems(page: Page): Promise<string[]> {
  return page.evaluate(() => {
    const moving =
      /^(transform|translate|rotate|scale|top|left|right|bottom|width|height|margin|inset)/i;
    const describe = (target: Element | null) =>
      target
        ? `${target.tagName.toLowerCase()}.${[...target.classList].join(".")}`
        : "?";
    const animations = document.getAnimations().flatMap((animation) => {
      const effect = animation.effect as KeyframeEffect | null;
      const properties = (effect?.getKeyframes() ?? []).flatMap((frame) =>
        Object.keys(frame).filter((key) => moving.test(key)),
      );
      const name =
        animation instanceof CSSAnimation
          ? animation.animationName
          : "transition";
      return properties.length
        ? [
            `${describe(effect?.target ?? null)}: ${name} (${[...new Set(properties)].join(", ")})`,
          ]
        : [];
    });
    const transitions = [
      ...document.querySelectorAll<HTMLElement>("*"),
    ].flatMap((element) => {
      const style = getComputedStyle(element);
      return parseFloat(style.transitionDuration) > 0.01 &&
        style.transitionProperty
          .split(",")
          .some(
            (property) =>
              moving.test(property.trim()) || property.trim() === "all",
          )
        ? [`${describe(element)}: transition ${style.transitionProperty}`]
        : [];
    });
    return [...animations, ...transitions];
  });
}

/** One page's audit in the given theme, as findings (empty when clean). */
export async function audit(page: Page, where: string): Promise<string[]> {
  const axe = await axeViolations(page);
  const walk = await tabWalk(page);
  const findings = [
    `${where} summary: ${walk.length} tab stops, ${axe.length} axe violations`,
    ...axe.map((finding) => `${where} axe ${finding}`),
    ...walkProblems(walk).map((finding) => `${where} keys ${finding}`),
  ];
  // WCAG 1.4.10: the page reflows; nothing scrolls it sideways, at any width the test set.
  const overflow = await page.evaluate(
    () =>
      document.documentElement.scrollWidth -
      document.documentElement.clientWidth,
  );
  if (overflow > 0)
    findings.push(
      `${where} reflow: the page scrolls sideways by ${overflow} px`,
    );
  await page.emulateMedia({ reducedMotion: "reduce" });
  findings.push(
    ...(await motionProblems(page)).map(
      (finding) => `${where} motion ${finding}`,
    ),
  );
  await page.emulateMedia({ reducedMotion: "no-preference" });
  return findings;
}

/**
 * Fails on any finding (WEB-U-007): every page and dialog stays clean. With A11Y_FINDINGS set, the findings and each
 * page's summary are also appended to that file, as the report's evidence.
 */
export function record(findings: string[]) {
  const file = process.env.A11Y_FINDINGS;
  if (file)
    appendFileSync(file, findings.map((finding) => `${finding}\n`).join(""));
  expect(findings.filter((finding) => !finding.includes(" summary: "))).toEqual(
    [],
  );
}

/**
 * One dialog or menu, opened from its trigger by keyboard: focus moves inside; Tab stays inside a modal dialog; axe
 * finds nothing; Escape closes it and returns focus to the trigger, or to `returnsTo` when the trigger is a menu item
 * that closed with its menu (UI-GUIDELINES §7, §9).
 */
export async function auditDialog(
  page: Page,
  trigger: Locator,
  where: string,
  {
    role = "dialog",
    modal = true,
    returnsTo = trigger,
  }: { role?: "dialog" | "menu"; modal?: boolean; returnsTo?: Locator } = {},
): Promise<string[]> {
  const findings: string[] = [];
  await trigger.focus();
  await page.keyboard.press("Enter");
  const surface = page.getByRole(role).last();
  await expect(surface).toBeVisible();
  const inside = () =>
    surface.evaluate((node) => node.contains(document.activeElement));
  if (!(await inside()))
    findings.push(`${where} focus did not move into the ${role}`);
  if (modal)
    for (let index = 0; index < 25; index++) {
      await page.keyboard.press("Tab");
      if (!(await inside())) {
        findings.push(
          `${where} Tab left the open ${role} after ${index + 1} stops`,
        );
        break;
      }
    }
  findings.push(
    ...(await axeViolations(page)).map((finding) => `${where} axe ${finding}`),
  );
  await page.keyboard.press("Escape");
  if (await surface.isVisible())
    findings.push(`${where} Escape did not close the ${role}`);
  else if (
    !(await returnsTo.evaluate((node) => node === document.activeElement))
  )
    findings.push(`${where} focus did not return to the trigger after Escape`);
  return findings;
}

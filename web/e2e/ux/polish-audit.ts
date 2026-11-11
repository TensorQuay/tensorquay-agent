import type { Page } from "@playwright/test";

/** One finding of the polish audit (UI-GUIDELINES §5b, WEB-U-021), bounded and human-readable. */
export type Finding = {
  view: string;
  rule: string;
  where: string;
  value: string;
};

// UI-GUIDELINES §5 spacing scale, plus the two §5b role values: 2 px between sidebar items, 20 px large-button padding.
const SPACING = [0, 2, 4, 8, 12, 16, 20, 24, 32, 48, 64];
// §5 radii: controls 6, dialogs 10, composer 16, pills and round icon buttons (999, or 50 % of a square).
// The UI standard: 8 px corners, the composer 16 px (pills are exempt).
const RADII = [0, 8, 16];
const MIN_CONTROL_HEIGHT = 32;
const MAX_FINDINGS = 200;

type Raw = Omit<Finding, "view">;

/** Computes §5b findings in the page; it reads computed styles only and never changes the document. */
function measure(input: {
  spacing: number[];
  radii: number[];
  minHeight: number;
  limit: number;
  root: string;
}): Raw[] {
  const found: Raw[] = [];
  const seen = new Set<string>();
  const add = (rule: string, element: Element, value: string) => {
    const where = describe(element);
    const key = `${rule}|${where}|${value}`;
    if (seen.has(key) || found.length >= input.limit) return;
    seen.add(key);
    found.push({ rule, where, value });
  };
  function describe(element: Element): string {
    const parts: string[] = [];
    let node: Element | null = element;
    while (node && parts.length < 3 && node !== document.body) {
      const label = node.getAttribute("aria-label");
      const classes = [...node.classList].slice(0, 2).join(".");
      parts.unshift(
        `${node.tagName.toLowerCase()}${classes ? `.${classes}` : ""}${label ? `[${label.slice(0, 32)}]` : ""}`,
      );
      node = node.parentElement;
    }
    return parts.join(" > ");
  }
  const visible = (element: Element) =>
    element.checkVisibility({
      opacityProperty: false,
      visibilityProperty: true,
    });
  const hidden = (element: Element) => {
    const style = getComputedStyle(element);
    return (
      style.clip !== "auto" ||
      style.clipPath !== "none" ||
      element.closest("svg") !== null
    );
  };
  const px = (element: Element, property: string): number | null => {
    const value = element.computedStyleMap().get(property);
    return value instanceof CSSUnitValue && value.unit === "px"
      ? value.value
      : null;
  };
  // §5: a control set inside a rounded box at an even inset takes the box's radius less the inset, so their corners are
  // concentric (a task row's ⋯, §7 Conversation list item): its nearest rounded ancestor's radius is on the scale, and
  // the control sits that radius difference from the ancestor's top and its end.
  const concentric = (element: Element, radius: number, box: DOMRect) => {
    for (
      let node = element.parentElement;
      node && node !== document.body;
      node = node.parentElement
    ) {
      const outer = px(node, "border-top-right-radius");
      if (!outer) continue;
      const frame = node.getBoundingClientRect();
      const top = box.top - frame.top;
      const end = frame.right - box.right;
      return (
        input.radii.includes(outer) &&
        Math.abs(top - end) < 0.6 &&
        Math.abs(outer - radius - top) < 0.6
      );
    }
    return false;
  };
  const root = document.querySelector(input.root) ?? document.body;
  for (const element of root.querySelectorAll("*")) {
    if (!visible(element) || hidden(element)) continue;
    const style = getComputedStyle(element);
    for (const side of ["top", "right", "bottom", "left"])
      for (const box of ["padding", "margin"]) {
        const value = px(element, `${box}-${side}`);
        if (
          value !== null &&
          !input.spacing.includes(Math.abs(Math.round(value * 100) / 100))
        )
          add(`${box} off the scale`, element, `${box}-${side}: ${value}px`);
      }
    if (["flex", "inline-flex", "grid", "inline-grid"].includes(style.display))
      for (const property of ["row-gap", "column-gap"]) {
        const value = px(element, property);
        if (value !== null && !input.spacing.includes(value))
          add("gap off the scale", element, `${property}: ${value}px`);
      }
    const radius = px(element, "border-top-left-radius");
    const box = element.getBoundingClientRect();
    if (
      radius !== null &&
      !input.radii.includes(radius) &&
      radius < 999 &&
      !(Math.abs(box.width - box.height) < 1 && radius * 2 >= box.height - 1) &&
      !concentric(element, radius, box)
    )
      add("radius off the scale", element, `${radius}px`);
    if (
      element.scrollWidth > element.clientWidth + 1 &&
      !["auto", "scroll"].includes(style.overflowX) &&
      style.textOverflow !== "ellipsis" &&
      element.clientWidth > 0 &&
      !(
        element instanceof HTMLTextAreaElement ||
        element instanceof HTMLInputElement
      )
    )
      add(
        "text overflows its box",
        element,
        `${element.scrollWidth}px in ${element.clientWidth}px`,
      );
  }
  // Words for screen readers only take no room on screen (WEB-U-461). Tailwind's `sr-only` sits in its utilities
  // layer, so an unlayered rule of ours on the same element wins over it and could put the words back.
  for (const element of root.querySelectorAll(".sr-only")) {
    const box = element.getBoundingClientRect();
    if (box.width > 1.5 || box.height > 1.5 || !hidden(element))
      add(
        "screen-reader words on screen",
        element,
        `${Math.round(box.width)}x${Math.round(box.height)}`,
      );
  }
  const interactive = document.querySelectorAll(
    'button, a[href], summary, select, input:not([type="hidden"]), [role="button"], [role="menuitem"], [role="menuitemradio"], [role="tab"]',
  );
  for (const element of interactive) {
    if (
      !visible(element) ||
      hidden(element) ||
      element.closest(".markdown, .skip-link")
    )
      continue;
    const box = element.getBoundingClientRect();
    if (box.height < input.minHeight - 0.5)
      add("control lower than 32 px", element, `${Math.round(box.height)}px`);
    const disabled = element.matches(":disabled, [aria-disabled='true']");
    const cursor = getComputedStyle(element).cursor;
    const textual = element.matches(
      "input:not([type=checkbox]):not([type=radio]), textarea, select",
    );
    if (!textual && !disabled && cursor !== "pointer")
      add("control without a pointer cursor", element, cursor);
    if (disabled && cursor !== "not-allowed")
      add("disabled control without not-allowed", element, cursor);
  }
  const icons = new Map<string, Set<string>>();
  for (const icon of document.querySelectorAll("svg.lucide")) {
    if (!visible(icon)) continue;
    // §5: 16 px in text and controls, 20 px in the header and in the composer bar's round buttons (§7 Composer).
    const context = icon.closest("header.app-header")
      ? "header"
      : icon.closest(".composer-row")
        ? "composer bar"
        : icon.closest("button, a, [role=menuitem], [role=menuitemradio]")
          ? "control"
          : "text";
    const box = icon.getBoundingClientRect();
    const size = `${Math.round(box.width)}x${Math.round(box.height)}`;
    icons.set(context, (icons.get(context) ?? new Set()).add(size));
  }
  for (const [context, sizes] of icons)
    if (sizes.size > 1)
      add(
        "more than one icon size",
        document.body,
        `${context}: ${[...sizes].join(", ")}`,
      );
  // An open modal makes the page behind it inert: the view is the modal, so only its buttons count.
  const modal = document.querySelector("dialog:modal");
  const primaries = [
    ...(modal ?? document).querySelectorAll('[data-variant="primary"]'),
  ].filter(visible);
  if (primaries.length > 1)
    add(
      "more than one primary button",
      document.body,
      `${primaries.length} primary buttons`,
    );
  return found;
}

/** Runs the audit on the current view; a copy check rejects the internal word "conversation" in visible copy. */
/** Measures `view`'s elements under `root` (the whole page by default; a spec may audit only the part it tests). The
 * wording check always reads the whole page. */
export async function auditView(
  page: Page,
  view: string,
  root = "body",
): Promise<Finding[]> {
  const raw = await page.evaluate(measure, {
    spacing: SPACING,
    radii: RADII,
    minHeight: MIN_CONTROL_HEIGHT,
    limit: MAX_FINDINGS,
    root,
  });
  const copy = await page.evaluate(() => {
    const words: string[] = [document.title, document.body.innerText];
    for (const element of document.querySelectorAll(
      "[aria-label], [title], [placeholder], [alt]",
    ))
      for (const name of ["aria-label", "title", "placeholder", "alt"]) {
        const value = element.getAttribute(name);
        if (value) words.push(value);
      }
    return words.join("\n");
  });
  const wording = [
    ...copy.matchAll(/[^\n]{0,40}\bconversations?\b[^\n]{0,40}/gi),
  ].map((match) => ({
    rule: 'user-visible "conversation"',
    where: "copy",
    value: match[0].trim(),
  }));
  return [...raw, ...wording].map((finding) => ({ view, ...finding }));
}

const MAX_FOCUS_STOPS = 40;
const MAX_HOVERED = 30;

/** Tabs through the view; every focus stop must show the §5 focus ring (outline or ring shadow). */
export async function auditFocus(page: Page, view: string): Promise<Finding[]> {
  const found: Finding[] = [];
  await page.locator("body").focus();
  for (let stop = 0; stop < MAX_FOCUS_STOPS; stop += 1) {
    await page.keyboard.press("Tab");
    const state = await page.evaluate(() => {
      const active = document.activeElement;
      if (!active || active === document.body) return null;
      // UI-GUIDELINES §7: the composer shows its ring on the card while its text has focus.
      const ringed = active.closest(".composer-card") ?? active;
      const style = getComputedStyle(ringed);
      const label =
        active.getAttribute("aria-label") ??
        active.textContent?.trim().slice(0, 32) ??
        "";
      return {
        where: `${active.tagName.toLowerCase()}[${label}]`,
        ring:
          (style.outlineStyle !== "none" &&
            parseFloat(style.outlineWidth) >= 2) ||
          style.boxShadow !== "none",
      };
    });
    if (!state) break;
    if (!state.ring)
      found.push({
        view,
        rule: "focus without a visible ring",
        where: state.where,
        value: "none",
      });
  }
  return found;
}

/**
 * Hovers each enabled control; its background, colour, border or underline must change, or its row's backdrop (run
 * with reduced motion). The active item of a navigation list (`aria-current="page"`) is the exception: it keeps its
 * selected fill under the pointer, never darker and with no underline (UI-GUIDELINES §5c, 17.3).
 */
export async function auditHover(page: Page, view: string): Promise<Finding[]> {
  const found: Finding[] = [];
  const controls = page.locator(
    'button:visible:not(:disabled), a[href]:visible, [role="menuitem"]:visible, [role="menuitemradio"]:visible',
  );
  const count = Math.min(await controls.count(), MAX_HOVERED);
  // A navigation row's link (a sidebar task) shows its hover in the backdrop of the row it fills, the list item, with
  // no underline of its own (§5c, §7 Conversation list item): that row's change counts as the link's.
  const look = (element: Element) => {
    const style = getComputedStyle(element);
    const row = element.closest("li");
    return [
      style.backgroundColor,
      style.color,
      style.borderColor,
      style.textDecorationLine,
      style.opacity,
      row ? getComputedStyle(row).backgroundColor : "",
    ].join("|");
  };
  for (let index = 0; index < count; index += 1) {
    const control = controls.nth(index);
    // Markdown links keep the prose style; the skip link is off-screen until focused; an open modal makes
    // everything outside it inert; the active item keeps its look (above, §5c): the current page's link, or a chosen
    // tab, which says so the tabs' way (§7 Tabs). The other tabs still change.
    if (
      await control.evaluate((element) => {
        const modal = document.querySelector("dialog:modal");
        return (
          element.closest(".markdown, .skip-link") !== null ||
          (modal !== null && !modal.contains(element)) ||
          element.getAttribute("aria-current") === "page" ||
          (element.getAttribute("role") === "tab" &&
            element.getAttribute("aria-selected") === "true")
        );
      })
    )
      continue;
    await page.mouse.move(0, 0);
    const rest = await control.evaluate(look);
    await control.hover({ timeout: 2000 }).catch(() => undefined);
    const hovered = await control.evaluate(look);
    if (rest === hovered) {
      const label = await control.evaluate(
        (element) =>
          element.getAttribute("aria-label") ??
          element.textContent?.trim().slice(0, 32) ??
          "",
      );
      found.push({
        view,
        rule: "hover without a visible change",
        where: label,
        value: "unchanged",
      });
    }
  }
  return found;
}

import type { Page } from "@playwright/test";

/**
 * The runtime style audit (18.2, UI guardrails): in the page, every visible element's computed font size and weight,
 * and every rounded corner, must be one of the tokens' values, read from the page itself, so a token change needs no
 * edit here. It catches what the source check (`pnpm tokens`) can't see: a library's or the browser's own styles, and
 * values that only arrive at run time. Returns one line per problem, "selector: what".
 */
export async function styleAudit(page: Page, root = "body"): Promise<string[]> {
  return page.evaluate(
    ({ root, sizeTokens, radiusTokens, exempt }) => {
      const probe = document.createElement("div");
      probe.style.position = "absolute";
      probe.style.visibility = "hidden";
      document.body.append(probe);
      const resolve = (
        property: "fontSize" | "borderTopLeftRadius",
        token: string,
      ) => {
        probe.style[property] = `var(--${token})`;
        return getComputedStyle(probe)[property];
      };
      const sizes = new Set(
        sizeTokens.map((token) => resolve("fontSize", token)),
      );
      const radii = new Set([
        "0px",
        ...radiusTokens.map((token) => resolve("borderTopLeftRadius", token)),
      ]);
      probe.remove();
      const describe = (element: Element) => {
        const classes = [...element.classList].slice(0, 2).join(".");
        return `${element.tagName.toLowerCase()}${element.id ? `#${element.id}` : ""}${classes ? `.${classes}` : ""}`;
      };
      const found = new Set<string>();
      const scope = document.querySelector(root);
      if (!scope) return [`${root}: not on the page`];
      for (const element of scope.querySelectorAll("*")) {
        if (element.closest(`svg, iframe, script, style, template, ${exempt}`))
          continue;
        const style = getComputedStyle(element);
        if (style.display === "none" || style.visibility === "hidden") continue;
        const box = element.getBoundingClientRect();
        if (box.width === 0 || box.height === 0) continue;
        const text = [...element.childNodes].some(
          (node) =>
            node.nodeType === Node.TEXT_NODE && (node.textContent ?? "").trim(),
        );
        if (text || element.matches("input, textarea, select, button")) {
          if (!sizes.has(style.fontSize))
            found.add(`${describe(element)}: font size ${style.fontSize}`);
          if (style.fontWeight !== "400" && style.fontWeight !== "700")
            found.add(`${describe(element)}: font weight ${style.fontWeight}`);
        }
        for (const corner of [
          style.borderTopLeftRadius,
          style.borderTopRightRadius,
          style.borderBottomRightRadius,
          style.borderBottomLeftRadius,
        ])
          if (!radii.has(corner.split(" ")[0] ?? corner))
            found.add(`${describe(element)}: radius ${corner}`);
      }
      return [...found];
    },
    {
      root,
      sizeTokens: [
        "display-size",
        "heading-size",
        "body-size",
        "small-size",
        "label-size",
        "nav-font-size",
      ],
      radiusTokens: [
        "control-radius",
        "dialog-radius",
        "composer-radius",
        "pill-radius",
        "row-action-radius",
        "thumbnail-radius",
      ],
      // The same exceptions as web/scripts/token-allowances.txt, by what they style: the PDF text layer follows
      // pdf.js's geometry, and a chart bar is a data mark.
      exempt: ".textLayer, .usage-day",
    },
  );
}

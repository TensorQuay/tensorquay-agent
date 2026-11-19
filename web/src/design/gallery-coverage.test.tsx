import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";

import { render } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { EXEMPT, GALLERY } from "./gallery";

const UI = join(__dirname, "..", "ui");

/** Every component `web/src/ui` exports: a PascalCase function or class in a .tsx file that isn't a test. */
function exportedComponents(): string[] {
  return readdirSync(UI)
    .filter((file) => file.endsWith(".tsx") && !file.includes(".test."))
    .flatMap((file) =>
      [
        ...readFileSync(join(UI, file), "utf8").matchAll(
          /^export (?:function|class) ([A-Z][a-z][A-Za-z0-9]*)/gm,
        ),
      ].map((match) => match[1] as string),
    )
    .sort();
}

// WEB-U-510 (18.2, UI guardrails): /design is the components' source of truth. Each component in web/src/ui has a
// gallery entry with its states, or an exemption that says why; a new component without one fails here.
describe("WEB-U-510 the component gallery", () => {
  const components = exportedComponents();

  it("has an entry or a reasoned exemption for every component web/src/ui exports", () => {
    const missing = components.filter(
      (name) => !(name in GALLERY) && !(name in EXEMPT),
    );
    expect(missing, `no gallery entry: ${missing.join(", ")}`).toEqual([]);
  });

  it("names only components that exist, and never one twice", () => {
    const named = [...Object.keys(GALLERY), ...Object.keys(EXEMPT)];
    expect(named.filter((name) => !components.includes(name))).toEqual([]);
    expect(Object.keys(GALLERY).filter((name) => name in EXEMPT)).toEqual([]);
  });

  it("gives every exemption a reason of a few words", () => {
    for (const [name, reason] of Object.entries(EXEMPT))
      expect(reason.trim().split(/\s+/).length, name).toBeGreaterThanOrEqual(4);
  });

  it("draws every state of every entry", () => {
    for (const [name, states] of Object.entries(GALLERY)) {
      expect(Object.keys(states).length, name).toBeGreaterThan(0);
      for (const [state, draw] of Object.entries(states)) {
        const { container, unmount } = render(<>{draw()}</>);
        expect(
          container.childElementCount,
          `${name}: ${state}`,
        ).toBeGreaterThan(0);
        unmount();
      }
    }
  });
});

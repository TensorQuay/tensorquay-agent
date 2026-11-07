import { describe, expect, it } from "vitest";

import { MOVED, movedTo, SECTIONS } from "./sections";

// WEB-U-075: the settings menu in three groups (plan §1 in UI-GUIDELINES §7), and the old addresses kept working.
describe("WEB-U-075 settings sections", () => {
  it("names every section once, in its group, with one address each", () => {
    const hrefs = SECTIONS.map((section) => section.href);
    expect(new Set(hrefs).size).toBe(hrefs.length);
    expect(new Set(SECTIONS.map((section) => section.id)).size).toBe(
      SECTIONS.length,
    );
    // Only Your account is for everyone.
    expect(
      SECTIONS.filter((section) => !section.admin).map(
        (section) => section.group,
      ),
    ).toEqual(["account", "account", "account", "account"]);
  });

  it("sends each old address to its new place, and nothing else", () => {
    expect(MOVED).toEqual({
      "/settings/git": "/settings/keys",
      "/settings/connector-credentials": "/settings/keys",
      "/settings/secrets": "/settings/keys/shared",
      "/settings/users": "/settings/people",
      "/settings/egress": "/settings/internet-access",
      "/settings/profiles": "/settings/agent-profiles",
    });
    expect(movedTo("/settings/models")).toBeNull();
    // Each lands on a section, or on a tab of one (the shared keys are Keys › Shared, one home for every key).
    for (const target of new Set(Object.values(MOVED)))
      expect(
        SECTIONS.some(
          (section) =>
            target === section.href || target.startsWith(`${section.href}/`),
        ),
      ).toBe(true);
    expect(SECTIONS.map((section): string => section.href)).not.toContain(
      "/settings/keys/shared",
    );
  });
});

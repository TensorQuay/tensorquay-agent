import { render, screen, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import type {
  SkillSkipReason,
  SkippedSkill,
} from "../protocol/generated/protocol";
import protocolSchema from "../protocol/generated/protocol.schema.json";
import { contextLoaded } from "../test/context";

import type { ContextReport } from "./context-state";
import { ContextView } from "./ContextView";

const report: ContextReport = {
  instructions: contextLoaded.instructions,
  skills: contextLoaded.skills,
  skipped: contextLoaded.skipped,
};
const show = (value: ContextReport, showReasons = true) =>
  render(
    <ContextView
      state={{
        kind: "report",
        startedAt: "2026-01-01T00:00:00Z",
        report: value,
      }}
      showReasons={showReasons}
    />,
  );
const section = (name: RegExp) => screen.getByRole("region", { name });

// What a run loaded, in plain words: instruction files, skills and the folders left out (WEB-U-046, FR-11).
describe("WEB-U-046 the context view", () => {
  it("lists the instruction files in order, marking one that was shortened", () => {
    show(report);
    expect(screen.getByText(/Loaded by the run started/)).toBeVisible();
    const files = within(section(/^Instructions · 2$/)).getAllByRole(
      "listitem",
    );
    expect(files.map((item) => item.textContent)).toEqual([
      "AGENTS.md",
      expect.stringContaining("services/billing/AGENTS.md"),
    ]);
    expect(files[1]).toHaveTextContent("Shortened");
    expect(files[1]).toHaveTextContent(
      "Only the start of this file fits; the rest was not given to the agent.",
    );
  });

  it("names each skill with its description and whose it is, and each folder left out with why", () => {
    show(report);
    const skills = within(section(/^Skills · 2$/)).getAllByRole("listitem");
    expect(skills[0]).toHaveTextContent("release-notes");
    expect(skills[0]).toHaveTextContent(
      "Drafts release notes from the merged changes.",
    );
    expect(skills[0]).toHaveTextContent("For everyone");
    expect(skills[1]).toHaveTextContent("Yours");
    const skipped = within(section(/^Not loaded · 2$/)).getAllByRole(
      "listitem",
    );
    expect(skipped[0]).toHaveTextContent("pdf-report");
    expect(skipped[0]).toHaveTextContent("For everyone");
    expect(skipped[0]).toHaveTextContent(
      "A skill with the same name in your own skills is used instead.",
    );
    expect(skipped[1]).toHaveTextContent("No SKILL.md in the folder.");
  });

  it("explains every reason the protocol defines, and an unknown one plainly", () => {
    const reasons = protocolSchema.definitions.SkillSkipReason.oneOf.map(
      (entry) => entry.const as SkillSkipReason,
    );
    const skipped: SkippedSkill[] = [...reasons, "renamed_later"].map(
      (reason, index) =>
        ({ folder: `f${index}`, tier: "user", reason }) as SkippedSkill,
    );
    show({ instructions: [], skills: [], skipped });
    const texts = within(section(/^Not loaded/))
      .getAllByRole("listitem")
      .map((item) => item.lastElementChild?.textContent);
    expect(new Set(texts).size).toBe(reasons.length + 1);
    expect(texts.at(-1)).toBe("This skill was not loaded.");
    expect(texts.every((text) => text && /\.$/.test(text))).toBe(true);
  });

  it("shows file contents' words as plain text only, and leaves out empty sections", () => {
    const description =
      "<img src=x onerror=alert(1)> **bold** https://example.com";
    const { container } = show({
      instructions: [],
      skills: [{ name: "odd", tier: "user", description }],
      skipped: [],
    });
    expect(screen.getByText(description)).toBeVisible();
    expect(container.querySelector("img, a, strong")).toBeNull();
    expect(
      screen.queryByRole("region", { name: /Instructions|Not loaded/ }),
    ).toBeNull();
  });

  // The general-purpose walk-through (finding F6): why a skill folder wasn't loaded is for administrators, who fix it.
  it("names a folder left out without its reason to anyone who isn't an administrator", () => {
    show(report, false);
    const skipped = within(section(/^Not loaded · 2$/)).getAllByRole(
      "listitem",
    );
    expect(skipped[0]).toHaveTextContent("pdf-report");
    expect(section(/^Not loaded · 2$/)).not.toHaveTextContent(
      /SKILL\.md|same name/,
    );
  });

  it("explains the panel before any run, a run that loaded nothing, and a report on its way", () => {
    const { rerender } = render(<ContextView state={{ kind: "none" }} />);
    expect(
      screen.getByText(
        "The instruction files and skills the agent loads appear here when a run starts.",
      ),
    ).toBeVisible();
    rerender(
      <ContextView
        state={{ kind: "nothing", startedAt: "2026-01-01T00:00:00Z" }}
      />,
    );
    expect(
      screen.getByText("This run loaded no instruction files or skills."),
    ).toBeVisible();
    rerender(<ContextView state={{ kind: "loading" }} />);
    expect(screen.getByLabelText("Loading the context")).toBeVisible();
  });
});

import { describe, expect, it } from "vitest";

import { purposeBeyond, suggestTitle } from "./suggest-title";

describe("WEB-U-062 a channel's suggested title", () => {
  it("takes the first sentence or line, without its full stop", () => {
    expect(
      suggestTitle("Get the 1.0 release out by Friday. Notes are drafted."),
    ).toBe("Get the 1.0 release out by Friday");
    expect(suggestTitle("  Plan the release\nand the notes")).toBe(
      "Plan the release",
    );
    expect(suggestTitle("Is it ready? Let's see.")).toBe("Is it ready?");
    expect(suggestTitle("发布 1.0。然后写说明。")).toBe("发布 1.0");
    expect(suggestTitle("")).toBe("");
  });
  it("keeps a long first sentence to sixty characters, ending at a word", () => {
    const title = suggestTitle(
      "Coordinate the migration of every service to the new cluster before the end of the quarter",
    );
    expect(Array.from(title).length).toBeLessThanOrEqual(60);
    expect(title).toBe("Coordinate the migration of every service to the new…");
  });
});

// A pass over the pages before 17.0: a channel made from its purpose alone showed that text twice, as its title and in
// full under it.
describe("WEB-U-250 a channel's purpose under its title", () => {
  const long =
    "Plan the spring newsletter: agree the topics and who drafts each part.";

  it("carries on from a cut title, never repeating its words", () => {
    const title = suggestTitle(long);
    expect(title).toBe(
      "Plan the spring newsletter: agree the topics and who drafts…",
    );
    expect(purposeBeyond(long, title)).toBe("…each part.");
  });

  it("gives the sentences after a whole first sentence, or nothing", () => {
    const purpose = "Get the 1.0 release out by Friday. The notes are drafted.";
    expect(purposeBeyond(purpose, suggestTitle(purpose))).toBe(
      "The notes are drafted.",
    );
    expect(purposeBeyond("Get the release out.", "Get the release out")).toBe(
      null,
    );
    expect(purposeBeyond("发布 1.0。然后写说明。", "发布 1.0")).toBe(
      "然后写说明。",
    );
    expect(purposeBeyond("", "release")).toBeNull();
  });

  it("shows the whole purpose under a title of the person's own", () => {
    expect(purposeBeyond(long, "Newsletter")).toBe(long);
    // A title that only starts the same is still the person's own.
    expect(purposeBeyond("Get the release out.", "Get the")).toBe(
      "Get the release out.",
    );
  });
});

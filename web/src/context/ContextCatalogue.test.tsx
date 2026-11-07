import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import changes from "../i18n/generated/en.changes.json";
import context from "../i18n/generated/en.context.json";
import core from "../i18n/generated/en.core.json";

import { ContextCatalogue } from "./ContextCatalogue";

// The Context tab's copy ships in its own catalogue, loaded when the tab first opens (WEB-U-046, WEB-U-014 budget).
describe("WEB-U-046 the context copy catalogue", () => {
  it("keeps the tab's own copy out of the core catalogue, but its label and loading state in it", () => {
    expect(
      Object.keys(context.messages).filter((key) => key in core.messages),
    ).toEqual([]);
    expect(Object.keys(context.messages)).toEqual(
      expect.arrayContaining(["context.skills", "context.reasonShadowed"]),
    );
    expect(Object.keys(core.messages)).toEqual(
      expect.arrayContaining(["context.loading"]),
    );
    // TEST: WEB-U-178
    // The tab's label travels with the files panel's tabs, which load with the panel and register its copy (WEB-U-178).
    expect(Object.keys(changes.messages)).toEqual(
      expect.arrayContaining(["files.tabContext"]),
    );
  });

  it("shows the view once its copy has loaded, and retries a failed download", async () => {
    const load = vi
      .fn<() => Promise<void>>()
      .mockRejectedValueOnce(new Error("offline"))
      .mockResolvedValueOnce(undefined);
    render(
      <ContextCatalogue load={load}>
        <p>Context view</p>
      </ContextCatalogue>,
    );
    expect(screen.getByLabelText("Loading the context")).toBeVisible();
    expect(
      await screen.findByText("This panel could not be displayed."),
    ).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "Try again" }));
    expect(await screen.findByText("Context view")).toBeVisible();
    expect(load).toHaveBeenCalledTimes(2);
  });
});

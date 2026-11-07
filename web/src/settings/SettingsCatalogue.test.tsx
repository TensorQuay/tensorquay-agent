import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import core from "../i18n/generated/en.core.json";
import deferred from "../i18n/generated/en.deferred.json";
import settings from "../i18n/generated/en.settings.json";
import { english } from "../i18n/translate";

import { SettingsCatalogue } from "./SettingsCatalogue";

// The settings copy ships in its own catalogue, loaded by the settings pages (WEB-U-014).
describe("WEB-U-014 settings copy catalogue", () => {
  it("keeps settings copy out of the core catalogue, and the transcript's egress messages in it", () => {
    const shared = Object.keys(settings.messages).filter(
      (key) => key in core.messages,
    );
    expect(shared).toEqual([]);
    expect(Object.keys(settings.messages)).toEqual(
      expect.arrayContaining([
        "egress.presetCustomAdd",
        "models.add",
        "secrets.add",
      ]),
    );
    expect(Object.keys(core.messages)).toEqual(
      expect.arrayContaining(["egress.blocked"]),
    );
    // A tool card's status words travel with the status pill, which registers the deferred catalogue (WEB-U-178).
    expect(Object.keys(deferred.messages)).toEqual(
      expect.arrayContaining(["status.running"]),
    );
    // The test setup loads the settings catalogue, as the settings pages do.
    expect(english("egress.presetCustomAdd")).toBe("Add a site");
  });

  it("shows the settings page once its copy has loaded, and retries a failed download", async () => {
    const load = vi
      .fn<() => Promise<void>>()
      .mockRejectedValueOnce(new Error("offline"))
      .mockResolvedValueOnce(undefined);
    render(
      <SettingsCatalogue load={load}>
        <p>Settings page</p>
      </SettingsCatalogue>,
    );
    expect(
      await screen.findByText("This panel could not be displayed."),
    ).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "Try again" }));
    expect(await screen.findByText("Settings page")).toBeVisible();
    expect(load).toHaveBeenCalledTimes(2);
  });
});

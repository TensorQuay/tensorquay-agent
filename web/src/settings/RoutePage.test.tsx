import { fireEvent, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { showModels } from "../test/models-app";
import { fakeRoutes, route } from "../test/routes";

/** Replaced by the harness once it renders (showModels). */
const router = vi.hoisted(() => ({ push: vi.fn<(href: string) => void>() }));
vi.mock("next/navigation", () => ({ useRouter: () => router }));
vi.mock("next/link", async () => {
  const { createElement } = await import("react");
  return {
    default: ({ href, children, ...rest }: Record<string, unknown>) =>
      createElement("a", { href, ...rest }, children as never),
  };
});

const LOCAL = route("local", {
  display_name: "Local model",
  key_ref: "secrets/local",
});
const page = () => {
  const form = screen
    .getByRole("group", { name: "Save or cancel" })
    .closest("form");
  if (!form) throw new Error("The page is a form");
  return form;
};

// WEB-U-115 (a first-hour check): adding or editing a model route is a page of its own, like an environment's.
describe("WEB-U-115 the model route page", () => {
  it("opens Add and Edit as pages of their own from the list", async () => {
    const at = showModels(
      fakeRoutes({ routes: [LOCAL], secrets: ["secrets/local"] }).api,
      router,
    );
    expect(
      await screen.findByRole("link", { name: "Add route" }),
    ).toHaveAttribute("href", "/settings/models/new");
    expect(
      screen.getByRole("link", { name: "Edit route Local model" }),
    ).toHaveAttribute("href", "/settings/models/local");
    fireEvent.click(
      screen.getByRole("link", { name: "Edit route Local model" }),
    );
    expect(
      await screen.findByRole("heading", {
        level: 2,
        name: "Edit route Local model",
      }),
    ).toBeVisible();
    expect(at.path).toBe("/settings/models/local");
    expect(screen.getByRole("link", { name: "Models" })).toHaveAttribute(
      "href",
      "/settings/models",
    );
    expect(screen.queryByRole("dialog")).toBeNull();
    for (const section of ["Route", "Key"])
      expect(screen.getByRole("region", { name: section })).toBeVisible();
    expect(within(page()).getByRole("button", { name: "Save" })).toBeVisible();
  });

  it("saves on the page and goes back to the list, which says so", async () => {
    const fake = fakeRoutes({ routes: [LOCAL], secrets: ["secrets/local"] });
    const at = showModels(fake.api, router, "/settings/models/local");
    await screen.findByRole("heading", { name: "Edit route Local model" });
    fireEvent.change(within(page()).getByLabelText("Max output"), {
      target: { value: "8192" },
    });
    expect(within(page()).getByText("Unsaved changes")).toBeVisible();
    fireEvent.click(within(page()).getByRole("button", { name: "Save" }));
    expect(await screen.findByText("Saved route Local model.")).toBeVisible();
    expect(at.path).toBe("/settings/models");
    expect(fake.writes.at(-1)?.fields).toMatchObject({ max_output: 8192 });
  });

  it("opens Add by its own address, asking where the model runs first", async () => {
    showModels(
      fakeRoutes({ routes: [LOCAL] }).api,
      router,
      "/settings/models/new",
    );
    expect(
      await screen.findByRole("heading", { level: 2, name: "Add a route" }),
    ).toBeVisible();
    expect(
      screen.getByRole("radiogroup", { name: "Where does the model run?" }),
    ).toBeVisible();
  });

  // F9: the key's actions read as buttons, not as plain text beside it.
  it("shows the key's actions as buttons", async () => {
    showModels(
      fakeRoutes({ routes: [LOCAL], secrets: ["secrets/local"] }).api,
      router,
      "/settings/models/local",
    );
    await screen.findByRole("heading", { name: "Edit route Local model" });
    for (const name of ["Replace key", "Use another shared key"])
      expect(within(page()).getByRole("button", { name })).toHaveAttribute(
        "data-variant",
        "secondary",
      );
  });

  it("says when the route no longer exists", async () => {
    showModels(
      fakeRoutes({ routes: [LOCAL] }).api,
      router,
      "/settings/models/gone",
    );
    expect(
      await screen.findByText("This route no longer exists."),
    ).toBeVisible();
    expect(screen.getByRole("link", { name: "Models" })).toHaveAttribute(
      "href",
      "/settings/models",
    );
  });
});

// 17.3 (C4): the list keeps a route's key to two lines; its page says the key's state where its key is set.
describe("WEB-U-446 a route's page says its key's state", () => {
  it("says a missing key, with where to add it", async () => {
    showModels(
      fakeRoutes({
        routes: [
          route("flash", {
            display_name: "DeepSeek Flash",
            key_missing: true,
            key_ref: "secrets/deepseek",
          }),
        ],
        secrets: [],
      }).api,
      router,
      "/settings/models/flash",
    );
    const key = await screen.findByRole("region", { name: "Key" });
    expect(within(key).getByText("Key missing")).toBeVisible();
    expect(
      within(key).getByRole("link", { name: "Add it under Keys › Shared" }),
    ).toHaveAttribute("href", "/settings/keys/shared");
  });

  it("says a key it needs", async () => {
    showModels(
      fakeRoutes({
        routes: [
          route("flash", {
            display_name: "DeepSeek Flash",
            key_ref_invalid: true,
          }),
          LOCAL,
        ],
        secrets: ["secrets/local"],
      }).api,
      router,
      "/settings/models/flash",
    );
    const key = await screen.findByRole("region", { name: "Key" });
    expect(within(key).getByText("Key needed")).toBeVisible();
  });
});

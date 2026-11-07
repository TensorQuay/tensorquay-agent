import { fireEvent, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { showModels } from "../test/models-app";
import { choose } from "../test/picker";
import { fakeRoutes } from "../test/routes";

import { NEW_ROUTE, type RouteFields } from "./admin/routes";
import { fromDraft, gatewayProblems, toDraft } from "./route-copy";

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

const local: RouteFields = {
  ...NEW_ROUTE,
  display_name: "Local model",
  base_url: "http://127.0.0.1:11434/v1",
  model: "qwen3:4b",
};

// WEB-U-099: a route's 'First response within', under Advanced: how long the gateway waits for the model's first byte.
/** The route's page (WEB-U-115): its form, from the title down to Save and Cancel. */
const page = () => {
  const form = screen
    .getByRole("group", { name: "Save or cancel" })
    .closest("form");
  if (!form) throw new Error("The page is a form");
  return form;
};

describe("WEB-U-099 the route's first-response bound", () => {
  it("round-trips a set bound, sends none when empty, and refuses one out of range", () => {
    const set = { ...local, first_response_secs: 120 };
    expect(fromDraft(toDraft("local", set))).toEqual({
      fields: set,
      problems: {},
    });
    // Empty: the gateway's default for where the model runs.
    const empty = fromDraft(toDraft("local", local));
    expect(empty.fields).not.toHaveProperty("first_response_secs");
    for (const text of ["4", "901", "1.5", "soon"]) {
      const { problems } = fromDraft({
        ...toDraft("local", local),
        first_response_secs: text,
      });
      expect(problems.first_response_secs).toBe("models.firstResponseRange");
    }
    expect(
      gatewayProblems([
        { field: "first_response_secs", reason: "first_response_invalid" },
      ]),
    ).toEqual({ first_response_secs: "models.firstResponseRange" });
  });

  it("shows the default for where the model runs, and saves a value", async () => {
    const fake = fakeRoutes({ routes: [], secrets: ["secrets/hosted"] });
    showModels(fake.api, router);
    fireEvent.click(await screen.findByRole("link", { name: "Add route" }));
    await screen.findByRole("group", { name: "Save or cancel" });
    const dialog = page();
    const field = (label: string, value: string) => {
      const box = within(dialog).queryByLabelText(label);
      if (box) fireEvent.change(box, { target: { value } });
      else choose(label, value, dialog);
    };
    const bound = () => within(dialog).getByLabelText("First response within");
    field("Address", "http://127.0.0.1:11434/v1");
    field("Send the key", "No key");
    expect(bound()).toHaveAttribute(
      "placeholder",
      "300 s (a model on this computer or your network)",
    );
    field("Address", "https://models.example.invalid/v1");
    field("Send the key", "As a bearer token");
    expect(bound()).toHaveAttribute("placeholder", "60 s");
    field("Send the key", "No key");
    field("Route name", "local");
    field("Display name", "Local model");
    field("Model", "qwen3:4b");
    field("First response within", "600");
    fireEvent.click(within(dialog).getByRole("button", { name: "Save" }));
    expect(await screen.findByText(/^Saved route Local model\./)).toBeVisible();
    expect(fake.writes.at(-1)?.fields).toMatchObject({
      first_response_secs: 600,
    });
  });
});

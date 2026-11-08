import { fireEvent, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { ApiError } from "../api/error";
import { showEnvironments } from "../test/environment-app";
import {
  BUILT_IN_IMAGES,
  BUILT_INS,
  environment,
  fakeTemplates,
  LISTED,
} from "../test/templates-admin";

/** Replaced by the harness once it renders (showEnvironments). */
const router = vi.hoisted(() => ({ push: vi.fn<(href: string) => void>() }));
vi.mock("next/navigation", () => ({ useRouter: () => router }));
vi.mock("next/link", async () => {
  const { createElement } = await import("react");
  return {
    default: ({ href, children, ...rest }: Record<string, unknown>) =>
      createElement("a", { href, ...rest }, children as never),
  };
});

const id = (hex: string) => `sha256:${hex.repeat(64)}`;
const page = () => {
  const form = screen
    .getByRole("group", { name: "Save or cancel" })
    .closest("form");
  if (!form) throw new Error("The page is a form");
  return form;
};
const set = (label: string, value: string) =>
  fireEvent.change(within(page()).getByLabelText(label), {
    target: { value },
  });
const images = () => within(page()).getByRole("region", { name: "Image" });
const picker = () =>
  within(images()).getByRole("button", {
    name: /^Or pick one of your images: /,
  });

// WEB-U-097 (the UI standard's Page): adding or editing an environment is a page of its own, and its image is picked
// from what the host has, not typed; the image ID waits under Advanced.
describe("WEB-U-097 the Environment page", () => {
  it("opens Add and Edit as pages of their own from the list", async () => {
    showEnvironments(fakeTemplates({ environments: LISTED }).api, router);
    expect(
      await screen.findByRole("link", { name: "Add environment" }),
    ).toHaveAttribute("href", "/settings/environments/new");
    expect(
      screen.getByRole("link", { name: "Edit environment Data science" }),
    ).toHaveAttribute("href", "/settings/environments/data");
    fireEvent.click(
      screen.getByRole("link", { name: "Edit environment Data science" }),
    );
    expect(
      await screen.findByRole("heading", {
        level: 2,
        name: "Edit environment Data science",
      }),
    ).toBeVisible();
    expect(screen.getByRole("link", { name: "Environments" })).toHaveAttribute(
      "href",
      "/settings/environments",
    );
    expect(screen.queryByRole("dialog")).toBeNull();
    for (const section of [
      "Environment",
      "Image",
      "Capabilities",
      "Resources",
      "Availability",
    ])
      expect(screen.getByRole("region", { name: section })).toBeVisible();
  });

  it("adds an environment from a built-in card, with the built-in's tools", async () => {
    const fake = fakeTemplates({
      environments: BUILT_INS,
      images: BUILT_IN_IMAGES,
    });
    const at = showEnvironments(fake.api, router, "/settings/environments/new");
    expect(
      await screen.findByRole("heading", {
        level: 2,
        name: "Add an environment",
      }),
    ).toBeVisible();
    const cards = await within(images()).findByRole("radiogroup", {
      name: "Start from a built-in",
    });
    const minimal = within(cards).getByRole("radio", { name: /^Minimal/ });
    expect(within(cards).getAllByRole("radio")).toHaveLength(2);
    // Each card says what it is: its description and its tools, by name.
    expect(minimal.closest("label")).toHaveTextContent("A shell and Python");
    expect(minimal.closest("label")).toHaveTextContent("Shell");
    // WEB-U-184 (a screen review before 17.0): a new environment starts from the default built-in, chosen and
    // seen to be; its tools are the built-in's, said once, not a second list of boxes.
    const workstation = within(cards).getByRole("radio", {
      name: /^Workstation/,
    });
    expect(workstation).toBeChecked();
    expect(minimal).not.toBeChecked();
    fireEvent.click(minimal);
    expect(minimal).toBeChecked();
    expect(workstation).not.toBeChecked();
    expect(within(page()).getByLabelText("Image ID")).toHaveValue(id("2"));
    const tools = within(page()).getByRole("region", { name: "Capabilities" });
    expect(tools).toHaveTextContent("Set by Minimal: Shell, Python");
    expect(within(tools).queryByRole("checkbox")).toBeNull();
    set("Display name", "Tables");
    fireEvent.click(within(page()).getByRole("button", { name: "Save" }));
    expect(
      await screen.findByText(
        "Saved environment Tables. The gateway checks it next.",
      ),
    ).toBeVisible();
    expect(at.path).toBe("/settings/environments");
    expect(fake.writes[0]?.fields).toMatchObject({
      image: id("2"),
      capabilities: ["shell", "python"],
    });
  });

  it("picks one of the host's own images, saying what each is and why one can't be used", async () => {
    const fake = fakeTemplates({
      environments: [
        ...BUILT_INS,
        environment("data", {
          display_name: "Data science",
          image: id("3"),
          image_id: id("3"),
        }),
      ],
      images: BUILT_IN_IMAGES.map((item) =>
        item.id === id("3") ? { ...item, used_by: ["data"] } : item,
      ),
    });
    showEnvironments(fake.api, router, "/settings/environments/new");
    await screen.findByRole("heading", { name: "Add an environment" });
    expect(
      await within(images()).findByRole("button", {
        name: /^Or pick one of your images: /,
      }),
    ).toHaveAccessibleName("Or pick one of your images: Choose an image");
    fireEvent.click(picker());
    const menu = screen.getByRole("menu", {
      name: "Or pick one of your images",
    });
    const choices = within(menu).getAllByRole("menuitemradio");
    // Only the host's own images: the built-ins are the cards above.
    // Each choice's name, then its line, read as two phrases.
    expect(choices.map((choice) => choice.textContent)).toEqual([
      "team/notebooks:1 Based on Workstation · Built Sep 29, 2026 · 1.4 GB · Used by Data science",
      "Untagged image Based on Minimal · Built Sep 28, 2026 · 1.4 GB",
      "team/old-tools:2025 Too old for this version: rebuild it from a current Workstation or Minimal.",
      "postgres:16 It has no agent runtime, so it can't be an environment.",
    ]);
    expect(choices[2]).toHaveAttribute("aria-disabled", "true");
    expect(choices[3]).toHaveAttribute("aria-disabled", "true");
    fireEvent.click(choices[2] as HTMLElement);
    expect(screen.getByRole("menu")).toBeVisible();
    fireEvent.click(choices[0] as HTMLElement);
    expect(screen.queryByRole("menu")).toBeNull();
    expect(picker()).toHaveAccessibleName(
      "Or pick one of your images: team/notebooks:1",
    );
    expect(within(page()).getByLabelText("Image ID")).toHaveValue(id("3"));
    // The image declares its tools, so they are ticked for it; no built-in card is chosen.
    for (const tool of ["Shell", "Python", "Node.js", "Build tools", "Browser"])
      expect(
        within(page()).getByRole("checkbox", { name: tool }),
      ).toBeChecked();
    for (const card of within(images()).getAllByRole("radio"))
      expect(card).not.toBeChecked();
    // WEB-U-184: for your own image the boxes are the choice, and what they mean comes first.
    const tools = within(page()).getByRole("region", { name: "Capabilities" });
    const hint = within(tools).getByText(/^Declared, not detected/);
    const first = within(tools).getAllByRole("checkbox")[0] as HTMLElement;
    expect(
      hint.compareDocumentPosition(first) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
  });

  it("says how to get images when the host has none of its own", async () => {
    showEnvironments(
      fakeTemplates({
        environments: BUILT_INS,
        images: BUILT_IN_IMAGES.filter((item) => item.built_in !== null),
      }).api,
      router,
      "/settings/environments/new",
    );
    await screen.findByRole("heading", { name: "Add an environment" });
    expect(
      await within(images()).findByText(
        "No environment images on this host yet. Build them on the host; the admin guide's Environments section says how.",
      ),
    ).toBeVisible();
    expect(
      within(images()).queryByRole("button", {
        name: /^Or pick one of your images/,
      }),
    ).toBeNull();
    expect(within(images()).getAllByRole("radio")).toHaveLength(2);
  });

  it("keeps the image ID under Advanced, open when the list failed or the image is none of the choices", async () => {
    showEnvironments(
      fakeTemplates({ environments: BUILT_INS, images: BUILT_IN_IMAGES }).api,
      router,
      "/settings/environments/new",
    );
    await screen.findByRole("heading", { name: "Add an environment" });
    await within(images()).findByRole("radiogroup");
    const advanced = within(images())
      .getByText("Advanced", { selector: "summary" })
      .closest("details");
    expect(advanced).not.toHaveAttribute("open");
    expect(advanced).toContainElement(
      within(page()).getByLabelText("Image ID"),
    );
    expect(advanced).toHaveTextContent("How to build one");
  });

  it("opens Advanced when the host's images can't be listed", async () => {
    showEnvironments(
      fakeTemplates({
        environments: BUILT_INS,
        images: new ApiError(503, "unavailable", "x"),
      }).api,
      router,
      "/settings/environments/new",
    );
    await screen.findByRole("heading", { name: "Add an environment" });
    expect(
      await within(images()).findByText(
        "The host's images couldn't be listed. You can still enter an image ID under Advanced.",
      ),
    ).toBeVisible();
    expect(
      within(images())
        .getByText("Advanced", { selector: "summary" })
        .closest("details"),
    ).toHaveAttribute("open");
  });

  it("opens Advanced on an environment pinned to an image the host doesn't list", async () => {
    showEnvironments(
      fakeTemplates({ environments: LISTED, images: BUILT_IN_IMAGES }).api,
      router,
      "/settings/environments/data",
    );
    await screen.findByRole("heading", {
      name: "Edit environment Data science",
    });
    await within(images()).findByRole("radiogroup");
    const field = within(page()).getByLabelText("Image ID");
    expect(field).toHaveValue(`tqa-runtime:data@sha256:${"a".repeat(64)}`);
    expect(field.closest("details")).toHaveAttribute("open");
  });

  it("puts the display name first, and makes the ID from it on a quiet line", async () => {
    const fake = fakeTemplates({ environments: LISTED });
    showEnvironments(fake.api, router, "/settings/environments/new");
    await screen.findByRole("heading", { name: "Add an environment" });
    const basics = within(page()).getByRole("region", { name: "Environment" });
    expect(within(basics).getAllByRole("textbox")[0]).toHaveAccessibleName(
      "Display name",
    );
    // Nothing to show until there is a display name, and no ID field to fill.
    expect(within(page()).queryByLabelText("ID")).toBeNull();
    expect(basics).not.toHaveTextContent("ID:");
    set("Display name", "Data Science Tools");
    expect(basics).toHaveTextContent("ID: data-science-tools · Change");
    // An ID already taken gets a number.
    set("Display name", "Minimal");
    expect(basics).toHaveTextContent("ID: minimal-2 · Change");
    set("Display name", "Data science");
    set("Image ID", `sha256:${"c".repeat(64)}`);
    fireEvent.click(within(page()).getByRole("button", { name: "Save" }));
    expect(
      await screen.findByText(
        "Saved environment Data science. The gateway checks it next.",
      ),
    ).toBeVisible();
    expect(fake.writes[0]?.name).toBe("data-science");
  });

  it("opens the ID on Change, keeps it as typed, and refuses new before sending", async () => {
    const fake = fakeTemplates({ environments: LISTED });
    showEnvironments(fake.api, router, "/settings/environments/new");
    await screen.findByRole("heading", { name: "Add an environment" });
    set("Display name", "Data science");
    fireEvent.click(
      within(page()).getByRole("button", { name: "Change the ID" }),
    );
    const id = within(page()).getByLabelText("ID");
    expect(id).toHaveValue("data-science");
    expect(id).toHaveFocus();
    expect(id).toHaveAccessibleDescription(
      "Lower-case letters, digits, - and _, up to 64 characters, starting with a letter. It cannot change later.",
    );
    // Once changed by hand, the display name no longer moves it.
    set("ID", "new");
    set("Display name", "Tables");
    expect(id).toHaveValue("new");
    fireEvent.click(within(page()).getByRole("button", { name: "Save" }));
    expect(
      await within(page()).findByText(
        "This ID is reserved for the Add page. Choose another.",
      ),
    ).toBeVisible();
    expect(fake.writes).toEqual([]);
  });

  it("opens the ID field by itself when the display name gives none", async () => {
    showEnvironments(
      fakeTemplates({ environments: LISTED }).api,
      router,
      "/settings/environments/new",
    );
    await screen.findByRole("heading", { name: "Add an environment" });
    set("Display name", "数据");
    expect(within(page()).getByLabelText("ID")).toHaveValue("");
  });

  it("opens the ID field when the gateway says the ID is taken", async () => {
    const fake = fakeTemplates({ environments: LISTED });
    // Someone else added it after the page loaded its list.
    fake.api.putEnvironment = () =>
      Promise.reject(new ApiError(409, "name_taken", "x"));
    showEnvironments(fake.api, router, "/settings/environments/new");
    await screen.findByRole("heading", { name: "Add an environment" });
    set("Display name", "Tables");
    set("Image ID", `sha256:${"c".repeat(64)}`);
    fireEvent.click(within(page()).getByRole("button", { name: "Save" }));
    expect(
      await within(page()).findByText("An environment already uses this name."),
    ).toBeVisible();
    expect(within(page()).getByLabelText("ID")).toHaveValue("tables");
  });

  it("shows no ID to change when editing: it is fixed once created", async () => {
    showEnvironments(
      fakeTemplates({ environments: LISTED }).api,
      router,
      "/settings/environments/minimal",
    );
    await screen.findByRole("heading", { name: "Edit environment Minimal" });
    expect(within(page()).queryByLabelText("ID")).toBeNull();
    expect(
      within(page()).queryByRole("button", { name: "Change the ID" }),
    ).toBeNull();
  });

  it("asks before leaving the page with unsaved changes", async () => {
    showEnvironments(
      fakeTemplates({ environments: LISTED }).api,
      router,
      "/settings/environments/minimal",
    );
    await screen.findByRole("heading", { name: "Edit environment Minimal" });
    set("Display name", "Small");
    fireEvent.click(screen.getByRole("link", { name: "Environments" }));
    expect(
      screen.getByRole("dialog", { name: "Leave without saving?" }),
    ).toBeVisible();
  });

  it("says when the environment no longer exists", async () => {
    showEnvironments(
      fakeTemplates({ environments: LISTED }).api,
      router,
      "/settings/environments/gone",
    );
    expect(
      await screen.findByText("This environment no longer exists."),
    ).toBeVisible();
    expect(screen.getByRole("link", { name: "Environments" })).toHaveAttribute(
      "href",
      "/settings/environments",
    );
  });
});

// WEB-U-098: a built-in the gateway keeps up to date until an administrator edits it; then it is theirs, and says so,
// with a way back.
describe("WEB-U-098 a customised built-in", () => {
  const notDefault = BUILT_INS.map((item) => ({
    ...item,
    is_default: item.name === "minimal",
  }));

  it("says an unedited built-in follows its rebuilds, and offers no reset", async () => {
    showEnvironments(
      fakeTemplates({ environments: BUILT_INS, images: BUILT_IN_IMAGES }).api,
      router,
      "/settings/environments/minimal",
    );
    await screen.findByRole("heading", { name: "Edit environment Minimal" });
    expect(page()).toHaveTextContent(
      "Built in: new builds of its image update it. Saving a change here makes it yours, and rebuilds stop updating it.",
    );
    expect(
      within(page()).queryByRole("button", { name: "Reset to built-in" }),
    ).toBeNull();
    expect(page()).not.toHaveTextContent("Customised");
  });

  it("marks a customised built-in in the list and resets it from the row after confirming", async () => {
    const fake = fakeTemplates({
      environments: notDefault,
      images: BUILT_IN_IMAGES,
    });
    showEnvironments(fake.api, router);
    const row = await screen.findByRole("row", { name: /Workstation/ });
    expect(row).toHaveTextContent("Customised: rebuilds no longer update it");
    expect(screen.getByRole("row", { name: /Minimal/ })).not.toHaveTextContent(
      "Customised",
    );
    fireEvent.click(
      within(row).getByRole("button", {
        name: "More actions for environment Workstation",
      }),
    );
    fireEvent.click(
      screen.getByRole("menuitem", { name: "Reset Workstation to built-in" }),
    );
    const ask = screen.getByRole("dialog", {
      name: "Reset Workstation to built-in?",
    });
    expect(ask).toHaveTextContent(
      "It takes the image of the host's current Workstation build, with that build's name and tools, and the gateway checks it again. Its size, description and Enabled stay.",
    );
    fireEvent.click(within(ask).getByRole("button", { name: "Cancel" }));
    expect(fake.resets).toEqual([]);
    fireEvent.click(
      within(row).getByRole("button", {
        name: "More actions for environment Workstation",
      }),
    );
    fireEvent.click(
      screen.getByRole("menuitem", { name: "Reset Workstation to built-in" }),
    );
    fireEvent.click(
      within(screen.getByRole("dialog")).getByRole("button", {
        name: "Reset to built-in",
      }),
    );
    expect(
      await screen.findByText(
        "Reset Workstation to built-in. The gateway checks it next.",
      ),
    ).toBeVisible();
    expect(fake.resets).toEqual([{ name: "workstation", version: 3 }]);
    expect(
      await screen.findByRole("row", { name: /Workstation/ }),
    ).not.toHaveTextContent("Customised");
  });

  it("resets from its page too, and goes back to the list", async () => {
    const fake = fakeTemplates({
      environments: notDefault,
      images: BUILT_IN_IMAGES,
    });
    const at = showEnvironments(
      fake.api,
      router,
      "/settings/environments/workstation",
    );
    await screen.findByRole("heading", {
      name: "Edit environment Workstation",
    });
    expect(page()).toHaveTextContent(
      "Customised: rebuilds no longer update it",
    );
    fireEvent.click(
      within(page()).getByRole("button", { name: "Reset to built-in" }),
    );
    fireEvent.click(
      within(
        screen.getByRole("dialog", { name: "Reset Workstation to built-in?" }),
      ).getByRole("button", { name: "Reset to built-in" }),
    );
    expect(
      await screen.findByText(
        "Reset Workstation to built-in. The gateway checks it next.",
      ),
    ).toBeVisible();
    expect(at.path).toBe("/settings/environments");
  });

  it("says why the default can't be reset, and when someone changed it first", async () => {
    const fake = fakeTemplates({
      environments: BUILT_INS,
      images: BUILT_IN_IMAGES,
    });
    showEnvironments(fake.api, router, "/settings/environments/workstation");
    await screen.findByRole("heading", {
      name: "Edit environment Workstation",
    });
    fireEvent.click(
      within(page()).getByRole("button", { name: "Reset to built-in" }),
    );
    const ask = screen.getByRole("dialog", {
      name: "Reset Workstation to built-in?",
    });
    fireEvent.click(
      within(ask).getByRole("button", { name: "Reset to built-in" }),
    );
    expect(
      await within(ask).findByText(
        "It is the default, and the reset would change its image or tools: make another environment the default first.",
      ),
    ).toBeVisible();
    fake.touch("workstation");
    fireEvent.click(
      within(ask).getByRole("button", { name: "Reset to built-in" }),
    );
    expect(
      await within(ask).findByText(
        "Someone changed this environment since you opened it. Close this and try again.",
      ),
    ).toBeVisible();
  });
});

// 17.3 (C4): the list keeps an environment's check to two lines (its words whole in the line's tooltip); its own page
// says the check in full.
describe("WEB-U-445 an environment's page says its check in full", () => {
  it("shows the check's state, why, and what it means for people", async () => {
    const broken = environment("broken", {
      display_name: "Broken tools",
      seed: null,
      last_set_by: "admin",
      check: {
        state: "refused",
        reason: "not_derived",
        checked_version: 1,
        secret_files: null,
        scan_limit: null,
      },
    });
    showEnvironments(
      fakeTemplates({ environments: [broken] }).api,
      router,
      "/settings/environments/broken",
    );
    const status = await screen.findByRole("region", { name: "Status" });
    expect(within(status).getByText("Refused")).toBeVisible();
    expect(
      within(status).getByText(
        "The image does not start from Workstation or Minimal. Build it FROM one of them.",
      ),
    ).toBeVisible();
    expect(
      within(status).getByText(
        "People cannot pick it until it passes the check.",
      ),
    ).toBeVisible();
  });

  it("shows a check that passed as its state alone", async () => {
    showEnvironments(
      fakeTemplates({ environments: [environment("ok")] }).api,
      router,
      "/settings/environments/ok",
    );
    const status = await screen.findByRole("region", { name: "Status" });
    expect(within(status).getByText("Verified")).toBeVisible();
  });
});

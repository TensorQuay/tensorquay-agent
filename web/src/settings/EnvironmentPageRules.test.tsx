import { fireEvent, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { ApiError } from "../api/error";
import { cellNote } from "../test/cell-note";
import { showEnvironments } from "../test/environment-app";
import {
  DIGEST,
  environment,
  fakeTemplates,
  LISTED,
} from "../test/templates-admin";

import type { TemplatesApi } from "./admin/templates";

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

function show(api: TemplatesApi) {
  showEnvironments(api, router);
}
/** The open confirmation (Make default, Delete). */
const dialog = () => screen.getByRole("dialog");
const click = (name: string) =>
  fireEvent.click(within(dialog()).getByRole("button", { name }));
/** The environment's page (WEB-U-097): its form, from the title down to Save and Cancel. */
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
const press = (name: string) =>
  fireEvent.click(within(page()).getByRole("button", { name }));
/** Follows a link on the list to an environment's page, and waits for it. */
async function open(link: string, title: string) {
  fireEvent.click(await screen.findByRole("link", { name: link }));
  await screen.findByRole("heading", { level: 2, name: title });
}

// Adding, editing, making default, deleting and checking an environment (WEB-U-047, ADR-0032). The row's menu holds
// all but Edit, and every name the page shows is the display name.
/** Opens an environment's "…" menu and chooses one of its items (UI-GUIDELINES §7). */
async function menuItem(display: string, item: string) {
  fireEvent.click(
    await screen.findByRole("button", {
      name: `More actions for environment ${display}`,
    }),
  );
  fireEvent.click(screen.getByRole("menuitem", { name: item }));
}
describe("WEB-U-047 environment sizes", () => {
  // The Product Owner's direction: sizes belong to each environment, never above the configuration file's ceiling.
  it("names an environment's own size under its name, and a size the ceiling lowered", async () => {
    show(
      fakeTemplates({
        environments: [
          ...LISTED,
          // Lowered at start: stored at the ceiling now, and what it was until an administrator saves it.
          environment("big", {
            display_name: "Big data",
            cpus: 8,
            memory_mib: 4096,
            clamped: ["cpus"],
            clamped_from: { cpus: 100 },
          }),
        ],
      }).api,
    );
    const row = await screen.findByRole("row", { name: /Big data/ });
    // Lowered at start: the stored size (what tasks get), marked, with what it was, in the row's muted line (17.3).
    expect(cellNote(row, "8 CPUs, lowered from 100 CPUs · 4 GB")).toBeVisible();
    fireEvent.click(
      within(row).getByRole("link", { name: "Edit environment Big data" }),
    );
    await screen.findByRole("heading", { name: "Edit environment Big data" });
    expect(within(page()).getByLabelText("CPUs")).toHaveAccessibleDescription(
      /Lowered from 100 CPUs to the installation's ceiling of 8 CPUs\./,
    );
  });

  // The list shows the stored size, which is what tasks get, never the ceiling itself; they
  // differ once the ceiling is raised after a start lowered a size.
  it("shows the stored size of a lowered environment, not the current ceiling", async () => {
    show(
      fakeTemplates({
        environments: [
          environment("small", {
            display_name: "Small",
            cpus: 6,
            clamped: ["cpus"],
            clamped_from: { cpus: 100 },
          }),
        ],
      }).api,
    );
    const row = await screen.findByRole("row", { name: /Small/ });
    expect(cellNote(row, "6 CPUs, lowered from 100 CPUs")).toBeVisible();
  });
});

describe("WEB-U-047 a lowered default environment", () => {
  // Its CPUs are locked (it is the default) and above the ceiling; it still saves.
  it("saves a default whose locked size the ceiling has since lowered", async () => {
    const fake = fakeTemplates({
      environments: [
        environment("workstation", {
          display_name: "Workstation",
          is_default: true,
          cpus: 12,
          clamped: ["cpus"],
        }),
      ],
    });
    show(fake.api);
    await open("Edit environment Workstation", "Edit environment Workstation");
    expect(within(page()).getByLabelText("CPUs")).toBeDisabled();
    set("Processes", "512");
    press("Save");
    expect(
      await screen.findByText(/^Saved environment Workstation\./),
    ).toBeVisible();
    expect(fake.writes[0]?.fields).toMatchObject({ cpus: 12, pids: 512 });
  });
});

describe("WEB-U-047 the environment page and dialogs", () => {
  // How to build an image from Workstation or Minimal, and what the Image field wants, in the
  // form, closed until asked for.
  it("explains how to build an environment's image, on request, when adding one", async () => {
    show(fakeTemplates({ environments: LISTED }).api);
    await open("Add environment", "Add an environment");
    const help = within(page())
      .getByText("How to build one", { selector: "summary" })
      .closest("details");
    if (!help) throw new Error("The help must be a disclosure");
    expect(help).not.toHaveAttribute("open");
    expect(help).toHaveTextContent("FROM tqa-runtime:workstation");
    expect(help).toHaveTextContent("USER 1000:1000");
    expect(help).toHaveTextContent(
      "docker image inspect --format '{{.Id}}' my-environment",
    );
    expect(help).toHaveTextContent(
      "A tag such as my-environment:latest can point at another build later, so the Image field takes the ID, which is this build and no other.",
    );
  });

  // What an environment is for, in one line, shown where people pick it.
  it("keeps an environment's one-line description, checked before sending, and shows it in the list", async () => {
    const fake = fakeTemplates({ environments: LISTED });
    show(fake.api);
    await open("Edit environment Minimal", "Edit environment Minimal");
    expect(
      within(page()).getByLabelText("Description"),
    ).toHaveAccessibleDescription(
      "One line, shown where people pick the environment. Optional.",
    );
    set("Description", "x".repeat(161));
    press("Save");
    expect(
      await within(page()).findByText("Use one line of up to 160 characters."),
    ).toBeVisible();
    expect(fake.writes).toEqual([]);
    set("Description", "Documents, data and web pages; no build tools");
    press("Save");
    expect(await screen.findByText(/^Saved environment Minimal/)).toBeVisible();
    expect(fake.writes.at(-1)).toMatchObject({
      fields: { description: "Documents, data and web pages; no build tools" },
    });
    // On the list again: the description leads the row's muted line (17.3).
    const row = await screen.findByRole("row", { name: /^Minimal/ });
    expect(
      cellNote(row, "Documents, data and web pages; no build tools"),
    ).toHaveTextContent(/^Documents, data and web pages; no build tools · /);
  });

  it("leaves the build help out when editing", async () => {
    show(fakeTemplates({ environments: LISTED }).api);
    await open("Edit environment Minimal", "Edit environment Minimal");
    expect(
      within(page()).queryByText("How to build one", { selector: "summary" }),
    ).toBeNull();
  });

  it("adds an environment pinned by digest, refusing a tag first", async () => {
    const fake = fakeTemplates({ environments: LISTED });
    show(fake.api);
    await open("Add environment", "Add an environment");
    set("Display name", "Notebooks");
    set("Image ID", "tqa-notebooks:latest");
    fireEvent.click(within(page()).getByRole("checkbox", { name: "Python" }));
    press("Save");
    expect(within(page()).getByText(/not by a tag/)).toBeVisible();
    expect(fake.writes).toEqual([]);
    set("Image ID", `tqa-notebooks@${DIGEST}`);
    // Sizes in the units people think in, each hint naming the range up to the ceiling and the default.
    expect(page()).toHaveTextContent(
      "From 0.5 to 8, in halves; empty uses the installation's 1.",
    );
    expect(page()).toHaveTextContent(
      "From 0.25 to 16; empty uses the installation's 0.5.",
    );
    expect(
      within(page()).getByLabelText("Processes"),
    ).toHaveAccessibleDescription(
      "From 64 to 4\u202f096; empty uses the installation's 256.",
    );
    expect(
      within(page()).getByLabelText("Disk for new workspaces (GB)"),
    ).toHaveAccessibleDescription(
      "A workspace made for this environment gets this much space; an existing workspace keeps its size. Up to 100; empty uses the installation's 10.",
    );
    set("CPUs", "1.5");
    set("Memory (GB)", "4");
    set("Disk for new workspaces (GB)", "20");
    press("Save");
    expect(
      await screen.findByText(
        "Saved environment Notebooks. The gateway checks it next.",
      ),
    ).toBeVisible();
    expect(fake.writes[0]?.fields).toEqual({
      display_name: "Notebooks",
      description: null,
      image: `tqa-notebooks@${DIGEST}`,
      capabilities: ["python"],
      cpus: 1.5,
      memory_mib: 4096,
      pids: null,
      workspace_mib: 20480,
      enabled: true,
    });
    expect(
      within(screen.getByRole("row", { name: /Notebooks/ })).getByText(
        "Checking…",
      ),
    ).toBeVisible();
  });

  it("keeps the default enabled, notes an image change, and keeps edits on a conflict", async () => {
    const fake = fakeTemplates({ environments: LISTED });
    show(fake.api);
    await open("Edit environment Workstation", "Edit environment Workstation");
    expect(
      within(page()).getByRole("checkbox", { name: "Enabled" }),
    ).toBeDisabled();
    expect(
      within(page()).getByText("The default environment stays enabled."),
    ).toBeVisible();
    // The gateway refuses changing what the default runs (template_is_default): those fields are locked, and say why.
    expect(within(page()).getByLabelText("Image ID")).toBeDisabled();
    for (const card of within(page()).queryAllByRole("radio"))
      expect(card).toBeDisabled();
    expect(
      within(page()).getByRole("checkbox", { name: "Python" }),
    ).toBeDisabled();
    expect(within(page()).getByLabelText("CPUs")).toBeDisabled();
    expect(within(page()).getByLabelText("Memory (GB)")).toBeDisabled();
    // Processes and disk don't change what the default runs, so they stay editable.
    expect(within(page()).getByLabelText("Processes")).toBeEnabled();
    expect(
      within(page()).getByLabelText("Disk for new workspaces (GB)"),
    ).toBeEnabled();
    expect(
      within(page()).getByText(/make another environment the default first/),
    ).toBeVisible();
    expect(within(page()).getByLabelText("Display name")).toBeEnabled();
    press("Cancel");
    await open(
      "Edit environment Data science",
      "Edit environment Data science",
    );
    set("Image ID", `data@sha256:${"b".repeat(64)}`);
    expect(
      within(page()).getByText(
        "Tasks on this environment move to the new image at their next session.",
      ),
    ).toBeVisible();
    fake.touch("data");
    press("Save");
    expect(
      await within(page()).findByText(/^Someone changed this environment/),
    ).toBeVisible();
    press("Save");
    expect(
      await screen.findByText(/^Saved environment Data science/),
    ).toBeVisible();
  });

  it("opens a seeded default that has not passed its check read-only, since any change is refused", async () => {
    const seededUnchecked = LISTED.map((item) =>
      item.name === "workstation"
        ? environment("workstation", {
            ...item,
            check: {
              state: "unchecked",
              reason: null,
              checked_version: null,
              secret_files: null,
              scan_limit: null,
            },
          })
        : item,
    );
    const fake = fakeTemplates({ environments: seededUnchecked });
    show(fake.api);
    await open("Edit environment Workstation", "Edit environment Workstation");
    expect(
      within(page()).getByText(
        /comes from the configuration file and has not passed its check yet/,
      ),
    ).toBeVisible();
    for (const label of [
      "Display name",
      "Image ID",
      "CPUs",
      "Memory (GB)",
      "Processes",
      "Disk for new workspaces (GB)",
    ])
      expect(within(page()).getByLabelText(label)).toBeDisabled();
    expect(
      within(page()).getByRole("checkbox", { name: "Shell" }),
    ).toBeDisabled();
    expect(within(page()).queryByRole("button", { name: "Save" })).toBeNull();
    press("Close");
    expect(fake.writes).toEqual([]);
  });

  // One page each: a second page mounted beside the first made the raced case wait on two lists' renders (a flake
  // under load), and the first page's button is gone once Minimal is the default.
  it("makes an offered environment the default after confirming", async () => {
    const fake = fakeTemplates({ environments: LISTED });
    show(fake.api);
    await menuItem("Minimal", "Make Minimal the default");
    expect(dialog()).toHaveTextContent("New tasks will start in Minimal.");
    click("Make default");
    expect(
      await screen.findByText("Minimal is now the default."),
    ).toBeVisible();
  });

  it("says why a raced environment cannot be the default", async () => {
    const raced = fakeTemplates({ environments: LISTED });
    raced.api.makeDefault = () =>
      Promise.reject(new ApiError(409, "template_unavailable", "x"));
    show(raced.api);
    await menuItem("Minimal", "Make Minimal the default");
    click("Make default");
    expect(
      await within(dialog()).findByText(
        "People cannot pick this environment now, so it cannot be the default.",
      ),
    ).toBeVisible();
  });

  it("deletes after confirming, says each thing that still uses one, and asks for a check", async () => {
    const fake = fakeTemplates({
      environments: LISTED,
      mcpServers: { data: ["files"] },
    });
    show(fake.api);
    await menuItem("Data science", "Delete environment Data science");
    expect(dialog()).toHaveTextContent("Nothing is removed from the engine");
    click("Delete environment");
    expect(
      await within(dialog()).findByText("Tool servers list it: files."),
    ).toBeVisible();
    click("Cancel");
    await menuItem("Old", "Delete environment Old");
    expect(dialog()).toHaveTextContent(
      "It comes from the configuration file and stays deleted until that entry changes.",
    );
    click("Delete environment");
    expect(await screen.findByText("Deleted environment Old.")).toBeVisible();
    await menuItem("Minimal", "Check the image of Minimal now");
    expect(
      await screen.findByText("Asked the gateway to check Minimal."),
    ).toBeVisible();
    expect(fake.checks).toEqual(["minimal"]);
  });
});

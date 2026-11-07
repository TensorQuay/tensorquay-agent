import { fireEvent, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { showEnvironments } from "../test/environment-app";
import {
  BUILT_IN_IMAGES,
  BUILT_INS,
  DELETED_DESKTOP,
  DESKTOP_TOOLS,
  fakeTemplates,
  image,
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

const page = () => {
  const form = screen
    .getByRole("group", { name: "Save or cancel" })
    .closest("form");
  if (!form) throw new Error("The page is a form");
  return form;
};
const region = (name: string) => within(page()).getByRole("region", { name });
const field = (label: string) => within(page()).getByLabelText(label);

/** Add an environment on a host where Desktop was deleted, its build still there; the cards once listed. */
async function addWith(images = [...BUILT_IN_IMAGES, DELETED_DESKTOP]) {
  const fake = fakeTemplates({ environments: BUILT_INS, images });
  showEnvironments(fake.api, router, "/settings/environments/new");
  await screen.findByRole("heading", { level: 2, name: "Add an environment" });
  const cards = await within(region("Image")).findByRole("radiogroup", {
    name: "Start from a built-in",
  });
  return { fake, cards };
}

const save = async (name: string) => {
  fireEvent.change(field("Display name"), { target: { value: name } });
  fireEvent.click(within(page()).getByRole("button", { name: "Save" }));
  await screen.findByText(
    `Saved environment ${name}. The gateway checks it next.`,
  );
};

// WEB-U-385 (the lead's 16j hand check, 15 Oct): Desktop added again from its card after a delete was a Workstation
// named Desktop, since the page kept only the five tools it knew. A card and an image keep every tool the label
// declares, Data and Desktop included; a Desktop box is offered only for an image that declares one.
describe("WEB-U-385 a built-in's card and an image's boxes keep every declared tool", () => {
  it("adds Desktop again from its card with all its tools, Desktop included", async () => {
    const { fake, cards } = await addWith();
    const desktop = within(cards).getByRole("radio", { name: /^Desktop/ });
    expect(desktop.closest("label")).toHaveTextContent("Data");
    fireEvent.click(desktop);
    expect(field("Image ID")).toHaveValue(DELETED_DESKTOP.id);
    expect(region("Capabilities")).toHaveTextContent(
      "Set by Desktop: Shell, Python, Node.js, Build tools, Browser, Data, Desktop",
    );
    await save("Desktop");
    expect(fake.writes[0]?.fields).toMatchObject({
      image: DELETED_DESKTOP.id,
      capabilities: DESKTOP_TOOLS,
    });
  });

  it("keeps Data when Workstation's label declares it", async () => {
    const { fake, cards } = await addWith(
      BUILT_IN_IMAGES.map((each) =>
        each.environment === "workstation" && each.built_in
          ? { ...each, capabilities: DESKTOP_TOOLS.slice(0, 6) }
          : each,
      ),
    );
    fireEvent.click(within(cards).getByRole("radio", { name: /^Workstation/ }));
    await save("Tools");
    expect(fake.writes[0]?.fields).toMatchObject({
      capabilities: ["shell", "python", "node", "build", "browser", "data"],
    });
  });

  it("offers and keeps Desktop's box for an image built FROM Desktop, and Data's for any image", async () => {
    const own = image("e", {
      tags: ["team/desktop-tools:1"],
      environment: "desktop",
      display_name: "Desktop",
      capabilities: DESKTOP_TOOLS,
      based_on: "desktop",
    });
    const { fake } = await addWith([...BUILT_IN_IMAGES, own]);
    fireEvent.click(
      within(region("Image")).getByRole("button", {
        name: /^Or pick one of your images: /,
      }),
    );
    fireEvent.click(
      within(screen.getByRole("menu")).getByRole("menuitemradio", {
        name: /^team\/desktop-tools:1/,
      }),
    );
    const tools = region("Capabilities");
    for (const tool of ["Data", "Desktop"])
      expect(within(tools).getByRole("checkbox", { name: tool })).toBeChecked();
    await save("Desktop tools");
    expect(fake.writes[0]?.fields).toMatchObject({
      image: own.id,
      capabilities: DESKTOP_TOOLS,
    });
  });

  it("offers no Desktop box for an image that doesn't declare one", async () => {
    await addWith();
    fireEvent.click(
      within(region("Image")).getByRole("button", {
        name: /^Or pick one of your images: /,
      }),
    );
    fireEvent.click(
      within(screen.getByRole("menu")).getByRole("menuitemradio", {
        name: /^team\/notebooks:1/,
      }),
    );
    const tools = region("Capabilities");
    expect(within(tools).getByRole("checkbox", { name: "Data" })).toBeVisible();
    expect(
      within(tools).queryByRole("checkbox", { name: "Desktop" }),
    ).toBeNull();
  });
});

// WEB-U-386: a built-in's card carries the built-in's own size (the image list's `sizes`), so Desktop added again
// has its 2 CPUs and 4 GB, not the installation's defaults; a card without one leaves the sizes blank (the defaults).
describe("WEB-U-386 a built-in's card fills a new environment's size from the built-in's own", () => {
  it("fills Desktop's 2 CPUs, 4 GB and 1024 processes, and saves them", async () => {
    const { fake, cards } = await addWith();
    fireEvent.click(within(cards).getByRole("radio", { name: /^Desktop/ }));
    expect(field("CPUs")).toHaveValue("2");
    expect(field("Memory (GB)")).toHaveValue("4");
    expect(field("Processes")).toHaveValue("1024");
    await save("Desktop");
    expect(fake.writes[0]?.fields).toMatchObject({
      cpus: 2,
      memory_mib: 4096,
      pids: 1024,
    });
  });

  it("leaves the sizes blank again for a card without its own", async () => {
    const { fake, cards } = await addWith();
    fireEvent.click(within(cards).getByRole("radio", { name: /^Desktop/ }));
    fireEvent.click(within(cards).getByRole("radio", { name: /^Workstation/ }));
    for (const label of ["CPUs", "Memory (GB)", "Processes"])
      expect(field(label)).toHaveValue("");
    await save("Tools");
    expect(fake.writes[0]?.fields).toMatchObject({
      cpus: null,
      memory_mib: null,
      pids: null,
    });
  });
});

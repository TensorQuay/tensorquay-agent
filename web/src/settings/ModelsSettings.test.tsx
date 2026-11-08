import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { ApiError } from "../api/error";
import type { User } from "../protocol/generated/api";
import { cellNote } from "../test/cell-note";
import { showModels } from "../test/models-app";
import { choose as choosePicked, picker } from "../test/picker";
import { fakeRoutes, route } from "../test/routes";
import { LIMITS } from "../test/user";

import { NEW_ROUTE, type RoutesApi } from "./admin/routes";
import { ModelsSettings } from "./ModelsSettings";

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

const admin: User = {
  id: "0195d8c0-0000-7000-8000-000000000001",
  email: "admin@example.invalid",
  role: "admin",
  internet: "none",
  limits: LIMITS,
  features: { agent_teams: false },
};
const listed = [
  route("local", {
    display_name: "Local model",
    seed: "current",
    last_set_by: "seed",
    fallback_of: ["deepseek-flash"],
  }),
  route("deepseek-flash", {
    display_name: "DeepSeek Flash",
    fallbacks: ["local"],
    key_missing: true,
    key_ref: "secrets/deepseek",
  }),
];
function show(api: RoutesApi, me: User = admin) {
  if (me.role === "admin") return showModels(api, router);
  // A member: the list alone, which says the section is for administrators.
  const cache = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  render(
    <QueryClientProvider client={cache}>
      <ModelsSettings me={me} api={api} />
    </QueryClientProvider>,
  );
}
/** The route's page (WEB-U-115): its form, from the title down to Save and Cancel. */
const page = () => {
  const form = screen
    .getByRole("group", { name: "Save or cancel" })
    .closest("form");
  if (!form) throw new Error("The page is a form");
  return form;
};
/** Follows a link on the list to a route's page, and waits for it. */
async function go(link: string) {
  fireEvent.click(await screen.findByRole("link", { name: link }));
  await screen.findByRole("group", { name: "Save or cancel" });
}
const row = (name: RegExp) => screen.getByRole("row", { name });
/** Types into the field labelled `label`, or, for a standard picker (WEB-U-134), chooses the choice so worded. */
const field = (label: string, value: string) => {
  const box = within(page()).queryByLabelText(label);
  if (box) fireEvent.change(box, { target: { value } });
  else choosePicked(label, value, page());
};
/** A field or a standard picker by its label. */
const labelled = (label: string, scope: HTMLElement) =>
  within(scope).queryByLabelText(label) ?? picker(label, scope);

// Design (models page) on the routes contract (ADR-0028): model routes for administrators (WEB-U-039).
/** Chooses one of a route's actions from its menu (one actions column: Edit, and the rest under "…"). */
function choose(route: string, action: string) {
  fireEvent.click(
    screen.getByRole("button", { name: `More actions for route ${route}` }),
  );
  fireEvent.click(screen.getByRole("menuitem", { name: action }));
}

describe("WEB-U-039 model routes", () => {
  // The 13f screens, smaller items (WEB-U-181, decision a): the Key column showed `secrets/provider-ga…`, the
  // configuration's notation, cut where keys differ. It shows the key's name, as Keys › Shared does, on one line,
  // with the full reference on hover.
  it("WEB-U-181 names a route's key without secrets/, with the full reference on hover", async () => {
    show(
      fakeRoutes({
        routes: [route("gateway", { key_ref: "secrets/provider-gateway-key" })],
        secrets: ["secrets/provider-gateway-key"],
      }).api,
    );
    await screen.findByRole("table", { name: "Model routes" });
    const key = within(row(/gateway/)).getByText("provider-gateway-key");
    expect(key).toHaveClass("secret-ref-line");
    expect(key).toHaveAttribute("title", "secrets/provider-gateway-key");
    expect(within(row(/gateway/)).queryByText(/^secrets\//)).toBeNull();
  });

  // WEB-U-301: on a page-made installation every row would say "Added here" and "Chat Completions". The source shows
  // on each route's muted line once any route comes from the configuration file (17.3: no Source column, as Connectors
  // and Environments say theirs), and the protocol once the routes differ in it.
  it("leaves out the source and the protocol when every route was added here with one protocol", async () => {
    const same = [route("office"), route("cloud")];
    show(fakeRoutes({ routes: same, secrets: [] }).api);
    await screen.findByRole("table", { name: "Model routes" });
    expect(
      screen.getAllByRole("columnheader").map((header) => header.textContent),
    ).toEqual(["Route", "Model", "Key", "Actions"]);
    expect(within(row(/office/)).getAllByRole("cell")).toHaveLength(4);
    expect(screen.queryByText("Added here")).toBeNull();
    expect(screen.queryByText("Chat Completions")).toBeNull();
  });

  // Review 7 Oct, P2-1: a file-managed list keeps its source, the only place that says so; and when every route has
  // left the file, its marker is what says to delete them. It is each route's muted line (17.3).
  it.each([
    ["current", "Configuration file"],
    ["removed", "No longer in the configuration file"],
  ] as const)(
    "names the source on each route when every route is %s in the configuration file",
    async (seed, words) => {
      const same = [
        route("office", { seed, last_set_by: "seed" }),
        route("cloud", { seed, last_set_by: "seed" }),
      ];
      show(fakeRoutes({ routes: same, secrets: [] }).api);
      await screen.findByRole("table", { name: "Model routes" });
      expect(
        screen.getAllByRole("columnheader").map((header) => header.textContent),
      ).toEqual(["Route", "Model", "Key", "Actions"]);
      expect(cellNote(row(/office/), words)).toHaveTextContent(words);
      expect(cellNote(row(/cloud/), words)).toHaveTextContent(words);
    },
  );

  it("names each route's source and protocol once the routes differ in them", async () => {
    const mixed = [
      route("office", { seed: "current", last_set_by: "seed" }),
      route("cloud", { protocol: "anthropic_messages" }),
    ];
    show(fakeRoutes({ routes: mixed, secrets: [] }).api);
    await screen.findByRole("table", { name: "Model routes" });
    expect(
      screen.getAllByRole("columnheader").map((header) => header.textContent),
    ).toEqual(["Route", "Model", "Key", "Actions"]);
    expect(cellNote(row(/office/), "Configuration file")).toBeVisible();
    expect(within(row(/office/)).getByText("Chat Completions")).toBeVisible();
    expect(cellNote(row(/cloud/), "Added here")).toBeVisible();
    expect(within(row(/cloud/)).getByText("Messages")).toBeVisible();
  });

  it("lists routes with their key, source and fallbacks, and explains a blocked delete", async () => {
    show(fakeRoutes({ routes: listed, secrets: ["secrets/local"] }).api);
    await screen.findByRole("table", { name: "Model routes" });
    // A change reaches open tasks from their next message (F12, GW-I-157), in the product's words.
    expect(
      screen.getByText(
        "Each route is one model on one server, with its key; people pick routes by their display names. A change applies to new tasks, and to open tasks from their next message; a message already running keeps its route.",
      ),
    ).toBeVisible();
    expect(
      within(row(/DeepSeek Flash/)).getByText("Key missing"),
    ).toBeVisible();
    expect(
      within(row(/DeepSeek Flash/)).getByRole("link", {
        name: "Add it under Keys › Shared",
      }),
    ).toHaveAttribute("href", "/settings/keys/shared");
    expect(cellNote(row(/Local model/), "Configuration file")).toBeVisible();
    // The display name only (UI-GUIDELINES §10), in what people see and in what a screen reader names: the route's own
    // name stays in its form and the configuration file.
    // (The Key column names its key, `local` here, which is the key's name, not the route's: WEB-U-181.)
    const [routeCell] = within(row(/Local model/)).getAllByRole("cell");
    expect(within(routeCell!).queryByText("local")).toBeNull();
    expect(within(row(/Local model/)).getAllByText("local")).toHaveLength(1);
    expect(
      within(row(/DeepSeek Flash/)).queryByText("deepseek-flash"),
    ).toBeNull();
    // The table fits the 720 px column: the protocol sits under the model and the source under the route, not in
    // columns of their own. Both routes speak Chat Completions, so no row names it (WEB-U-301).
    expect(
      screen.getAllByRole("columnheader").map((header) => header.textContent),
    ).toEqual(["Route", "Model", "Key", "Actions"]);
    expect(screen.queryByText("Chat Completions")).toBeNull();
    // One actions column (UI-GUIDELINES §7): Edit, a link to the route's page (WEB-U-115), the rest in the row's menu.
    expect(
      [...row(/Local model/).querySelectorAll("a, button")].map((control) =>
        control.getAttribute("aria-label"),
      ),
    ).toEqual(["Edit route Local model", "More actions for route Local model"]);
    expect(within(row(/Local model/)).queryByText("Check")).toBeNull();
    fireEvent.click(
      screen.getByRole("button", {
        name: "More actions for route Local model",
      }),
    );
    // A delete that isn't possible says why, and stays reachable by keyboard (UI-GUIDELINES §5b).
    const blocked = screen.getByRole("menuitem", {
      name: "Delete route Local model",
    });
    expect(blocked).toHaveAttribute("aria-disabled", "true");
    expect(blocked).toHaveAccessibleDescription(
      "Other routes fall back to it: DeepSeek Flash.",
    );
    expect(blocked).toHaveAttribute("data-danger", "true");
    fireEvent.click(blocked);
    expect(screen.queryByRole("dialog")).toBeNull();
    fireEvent.keyDown(blocked, { key: "Escape" });
    const download = screen.getByRole("link", { name: "Download models.toml" });
    expect(download).toHaveAttribute("href", "/fixture/models.toml");
    // What the download is for, read with it (WEB-U-283).
    expect(download).toHaveAccessibleDescription(
      "The download holds every route in the installation's models.toml format. It names their keys but never holds their values.",
    );
  });

  // Lead (2026-09-26): a badge only for a real probe result; a route never checked shows nothing.
  it("shows a route probe's result, and nothing for a route never checked", async () => {
    const probed = (state: "verified" | "failed" | "stale") => ({
      probe: { state, probed_at: "2026-09-26T08:00:00Z", observed: null },
    });
    show(
      fakeRoutes({
        routes: [
          route("local", { display_name: "Local model" }),
          route("fast", { display_name: "Fast", ...probed("verified") }),
          route("broken", { display_name: "Broken", ...probed("failed") }),
          route("older", { display_name: "Older", ...probed("stale") }),
        ],
        secrets: [],
      }).api,
    );
    await screen.findByRole("table", { name: "Model routes" });
    const badges = ["Verified", "Check failed", "Check out of date"];
    for (const text of badges)
      expect(within(row(/Local model/)).queryByText(text)).toBeNull();
    expect(within(row(/Fast/)).getByText("Verified")).toBeVisible();
    expect(within(row(/Broken/)).getByText("Check failed")).toBeVisible();
    expect(within(row(/Older/)).getByText("Check out of date")).toBeVisible();
    expect(within(row(/Fast/)).queryByText("Check failed")).toBeNull();
  });

  // F4, part 1: what most routes need comes first, in plain headings; the tuning waits under Advanced, closed.
  it("puts what most routes need first, and the rest under Advanced, closed", async () => {
    show(fakeRoutes({ routes: listed, secrets: ["secrets/local"] }).api);
    await screen.findByRole("table", { name: "Model routes" });
    await go("Add route");
    const dialog = page();
    const advanced = within(dialog)
      .getByText("Advanced", { selector: "summary" })
      .closest("details");
    if (!advanced) throw new Error("Advanced must be a disclosure");
    expect(advanced).not.toHaveAttribute("open");
    // The route's name is made from the display name, so it waits under Advanced too.
    expect(within(advanced).getByLabelText("Route name")).toBeInTheDocument();
    for (const label of ["Display name", "Address", "Model"])
      expect(advanced.contains(within(dialog).getByLabelText(label))).toBe(
        false,
      );
    // Adding takes the key inline (F4, part 2c).
    expect(
      within(
        within(dialog).getByRole("region", { name: "Key" }),
      ).getByLabelText("Key", { selector: "input" }),
    ).toBeInTheDocument();
    for (const label of [
      "Protocol",
      "Send the key",
      "Context window",
      "Max output",
      "Concurrency",
      "Requests per minute",
    ])
      expect(labelled(label, advanced)).toBeInTheDocument();
    // An empty limit says what it means: the server's own, or no limit here.
    for (const [label, placeholder] of [
      ["Context window", "The server's own"],
      ["Max output", "The server's own (no limit sent)"],
      ["Concurrency", "No limit"],
      ["Requests per minute", "No limit"],
    ] as const)
      expect(within(advanced).getByLabelText(label)).toHaveAttribute(
        "placeholder",
        placeholder,
      );
  });

  // WEB-U-108: an empty Max output says what it means on each protocol: no limit sent on Chat Completions and Responses
  // (the server's own), and required on Anthropic Messages, which can't go without one.
  it("WEB-U-108 says what an empty Max output means on each protocol", async () => {
    show(fakeRoutes({ routes: listed, secrets: ["secrets/local"] }).api);
    await screen.findByRole("table", { name: "Model routes" });
    await go("Add route");
    const dialog = page();
    const advanced = within(dialog)
      .getByText("Advanced", { selector: "summary" })
      .closest("details");
    if (!advanced) throw new Error("Advanced must be a disclosure");
    for (const [protocol, placeholder] of [
      ["Chat Completions", "The server's own (no limit sent)"],
      ["Responses", "The server's own (no limit sent)"],
      ["Messages", "Required for Anthropic Messages, e.g. 16384"],
    ] as const) {
      choosePicked("Protocol", protocol, advanced);
      expect(within(advanced).getByLabelText("Max output")).toHaveAttribute(
        "placeholder",
        placeholder,
      );
    }
  });

  // F4, part 2: the first question is where the model runs; the form fits the answer (no vendor presets).
  it("asks where the model runs first, and fits the form to the answer", async () => {
    const fake = fakeRoutes({ routes: listed, secrets: ["secrets/local"] });
    show(fake.api);
    await screen.findByRole("table", { name: "Model routes" });
    await go("Add route");
    const dialog = page();
    const where = within(dialog).getByRole("radiogroup", {
      name: "Where does the model run?",
    });
    expect(
      within(where)
        .getAllByRole("radio")
        .map(
          (radio) =>
            radio.getAttribute("aria-label") ??
            radio.closest("label")?.textContent,
        ),
    ).toEqual([
      "A hosted service",
      "A server on your network",
      "Something else",
    ]);
    // A hosted service: its address and a key.
    expect(
      within(where).getByRole("radio", { name: "A hosted service" }),
    ).toBeChecked();
    expect(
      within(dialog).getByLabelText("Key", { selector: "input" }),
    ).toBeInTheDocument();
    // A server on your network: a key only if it needs one.
    fireEvent.click(
      within(where).getByRole("radio", { name: "A server on your network" }),
    );
    expect(
      within(dialog).queryByLabelText("Key", { selector: "input" }),
    ).toBeNull();
    const needs = within(dialog).getByRole("checkbox", {
      name: "It needs a key",
    });
    expect(needs).not.toBeChecked();
    fireEvent.click(needs);
    expect(
      within(dialog).getByLabelText("Key", { selector: "input" }),
    ).toBeInTheDocument();
    fireEvent.click(needs);
    // Something else: every setting, Advanced open.
    fireEvent.click(
      within(where).getByRole("radio", { name: "Something else" }),
    );
    expect(
      within(dialog)
        .getByText("Advanced", { selector: "summary" })
        .closest("details"),
    ).toHaveAttribute("open");
    // Saved from a server on your network without a key: it sends none.
    fireEvent.click(
      within(where).getByRole("radio", { name: "A server on your network" }),
    );
    field("Display name", "Office model");
    field("Address", "http://gpu.office.invalid:8000/v1");
    field("Model", "qwen2.5-coder");
    fireEvent.click(within(dialog).getByRole("button", { name: "Save" }));
    expect(
      await screen.findByText(/^Saved route Office model\./),
    ).toBeVisible();
    expect(fake.writes.at(-1)).toMatchObject({
      name: "office-model",
      fields: { auth: { kind: "none" }, key_ref: null },
    });
  });

  // F4, part 2b: editing a route says in one line where it connects and which key it uses; each opens on request.
  it("sums up an existing route's connection and key, and opens each to change it", async () => {
    show(fakeRoutes({ routes: listed, secrets: ["secrets/local"] }).api);
    await screen.findByRole("table", { name: "Model routes" });
    await go("Edit route Local model");
    const dialog = page();
    expect(
      within(dialog).getByText(
        "Connects to models.example.invalid · Chat Completions",
      ),
    ).toBeVisible();
    expect(within(dialog).queryByLabelText("Address")).toBeNull();
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Change the address" }),
    );
    expect(within(dialog).getByLabelText("Address")).toHaveValue(
      "https://models.example.invalid/v1",
    );
    expect(
      within(dialog).getByText("Uses the shared key local."),
    ).toBeVisible();
    expect(
      within(dialog).queryByLabelText("Key", { selector: "input" }),
    ).toBeNull();
    // Replace opens Paste a key at once: a new key for this route, never an overwrite of a shared one.
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Replace key" }),
    );
    expect(
      within(dialog).getByRole("group", { name: "Paste a key" }),
    ).toBeVisible();
    expect(within(dialog).getByLabelText("Secret name")).toHaveValue(
      "local-key",
    );
    // Or another shared key: the Secret field, holding the current one.
    // The dialog's own Cancel, the last of two (the key panel has one).
    const cancel = within(dialog)
      .getAllByRole("button", { name: "Cancel" })
      .at(-1);
    if (!cancel) throw new Error("The dialog must have Cancel");
    fireEvent.click(cancel);
    await go("Edit route Local model");
    const again = page();
    fireEvent.click(
      within(again).getByRole("button", { name: "Use another shared key" }),
    );
    expect(within(again).getByLabelText("Shared key")).toHaveValue("local");
  });

  it("makes the route's name from its display name until the name is edited", async () => {
    show(fakeRoutes({ routes: listed }).api);
    await screen.findByRole("table", { name: "Model routes" });
    await go("Add route");
    const dialog = page();
    field("Display name", "Qwen Coder 32B");
    expect(within(dialog).getByLabelText("Route name")).toHaveValue(
      "qwen-coder-32b",
    );
    field("Route name", "qwen");
    field("Display name", "Qwen");
    expect(within(dialog).getByLabelText("Route name")).toHaveValue("qwen");
  });

  // General-purpose finding X1: an optional purpose line, shown where people pick the model, and in the list.
  it("keeps a route's one-line description, checked before sending, and shows it in the list", async () => {
    const fake = fakeRoutes({ routes: listed, secrets: ["secrets/local"] });
    show(fake.api);
    await screen.findByRole("table", { name: "Model routes" });
    await go("Edit route Local model");
    const dialog = page();
    expect(
      within(dialog).getByLabelText("Description"),
    ).toHaveAccessibleDescription(
      "One line, shown where people pick the model. Optional.",
    );
    for (const bad of ["x".repeat(161), "a\ttab"]) {
      field("Description", bad);
      fireEvent.click(within(dialog).getByRole("button", { name: "Save" }));
      expect(
        await within(dialog).findByText(
          "Use one line of up to 160 characters.",
        ),
      ).toBeVisible();
    }
    expect(fake.writes).toEqual([]);
    field("Description", "  Fast, for short everyday tasks  ");
    fireEvent.click(within(dialog).getByRole("button", { name: "Save" }));
    expect(await screen.findByText(/^Saved route Local model\./)).toBeVisible();
    expect(fake.writes.at(-1)?.fields?.description).toBe(
      "Fast, for short everyday tasks",
    );
    // It leads the route's muted line, before its source (17.3).
    expect(
      await within(row(/Local model/)).findByText(
        /^Fast, for short everyday tasks · Configuration file$/,
      ),
    ).toBeVisible();
  });

  it("asks for a display name before saving", async () => {
    const fake = fakeRoutes({ routes: listed, secrets: ["secrets/local"] });
    show(fake.api);
    await screen.findByRole("table", { name: "Model routes" });
    await go("Add route");
    field("Route name", "qwen");
    field("Address", "http://vllm.example.invalid:8000/v1");
    field("Model", "qwen2.5-coder");
    field("Send the key", "No key");
    fireEvent.click(
      within(page()).getByRole("button", {
        name: "Save",
      }),
    );
    expect(
      await within(page()).findByText(
        "Give the route a display name: it is what people pick.",
      ),
    ).toBeVisible();
    expect(fake.writes).toEqual([]);
    // WEB-U-110: the refusal shows where, whatever the dialog's scroll: the field has the focus, and Save says so.
    expect(within(page()).getByLabelText("Display name")).toHaveFocus();
    expect(within(page()).getByText("Fix 1 field above.")).toHaveRole("status");
  });

  it("adds a route with the fields it sets and the defaults it leaves", async () => {
    const fake = fakeRoutes({ routes: listed, secrets: ["secrets/local"] });
    show(fake.api);
    await screen.findByRole("table", { name: "Model routes" });
    await go("Add route");
    field("Route name", "qwen");
    field("Display name", "Qwen Coder");
    field("Address", "http://vllm.example.invalid:8000/v1");
    field("Model", "qwen2.5-coder");
    field("Send the key", "No key");
    field("Context window", "32768");
    fireEvent.click(
      within(page()).getByRole("button", {
        name: "Save",
      }),
    );
    expect(await screen.findByText(/^Saved route Qwen Coder\./)).toBeVisible();
    expect(fake.writes).toEqual([
      {
        name: "qwen",
        fields: {
          ...NEW_ROUTE,
          display_name: "Qwen Coder",
          base_url: "http://vllm.example.invalid:8000/v1",
          model: "qwen2.5-coder",
          auth: { kind: "none" },
          context_window: 32768,
        },
        key: expect.any(String) as string,
      },
    ]);
    expect(screen.getByRole("row", { name: /Qwen Coder/ })).toBeVisible();
    // WEB-U-288: a route just added is offered its check, beside the notice.
    expect(
      screen.getByText(
        "Saved route Qwen Coder. Check it before people pick it: it takes a minute and a few tokens.",
      ),
    ).toHaveRole("status");
    fireEvent.click(
      screen.getByRole("button", { name: "Check Qwen Coder now" }),
    );
    expect(
      screen.getByRole("dialog", { name: "Check route Qwen Coder" }),
    ).toBeVisible();
  });

  it("edits with the version read, orders fallbacks and recovers from another admin's change", async () => {
    const fake = fakeRoutes({
      routes: [...listed, route("spare", { display_name: "Spare model" })],
      secrets: ["secrets/local"],
    });
    show(fake.api);
    await screen.findByRole("table", { name: "Model routes" });
    await go("Edit route DeepSeek Flash");
    const dialog = page();
    expect(within(dialog).getByLabelText("Model")).toHaveValue(
      "deepseek-flash-model",
    );
    // WEB-U-284: the fallbacks by their display names, as the rest of the page names routes (UI-GUIDELINES §10).
    field("Add a fallback", "Spare model");
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Move Spare model up" }),
    );
    expect(
      [...dialog.querySelectorAll(".fallback-row > span")].map(
        (each) => each.textContent,
      ),
    ).toEqual(["Spare model", "Local model"]);
    fake.touch("deepseek-flash");
    fireEvent.click(within(dialog).getByRole("button", { name: "Save" }));
    expect(
      await within(dialog).findByText(
        "Someone changed this route since you opened it. Your edits are kept: save again to replace their change with yours.",
      ),
    ).toBeVisible();
    fireEvent.click(within(dialog).getByRole("button", { name: "Save" }));
    expect(
      await screen.findByText(/^Saved route DeepSeek Flash\./),
    ).toBeVisible();
    // An edited route's notice is the plain one, with no offer to check (WEB-U-288).
    expect(screen.getByText("Saved route DeepSeek Flash.")).toBeVisible();
    expect(screen.queryByRole("button", { name: /now$/ })).toBeNull();
    expect(
      fake.writes.map((write) => [write.version, write.fields?.fallbacks]),
    ).toEqual([
      [1, ["spare", "local"]],
      [2, ["spare", "local"]],
    ]);
  });

  it("names the gateway's field problems at their fields, and refuses bad JSON before sending", async () => {
    const fake = fakeRoutes({ routes: listed });
    show(fake.api);
    await screen.findByRole("table", { name: "Model routes" });
    await go("Add route");
    field("Display name", "Bad name");
    field("Route name", "bad name");
    field("Extra body (JSON object)", "[1, 2]");
    fireEvent.click(
      within(page()).getByRole("button", {
        name: "Save",
      }),
    );
    expect(
      await screen.findByText(
        'Enter a JSON object, like {"temperature": 0.2}.',
      ),
    ).toBeVisible();
    expect(fake.writes).toEqual([]);
    field("Extra body (JSON object)", "{}");
    // The model is named here before anything is sent (F4, part 2c: no key is stored for a refused route).
    fireEvent.click(
      within(page()).getByRole("button", {
        name: "Save",
      }),
    );
    expect(
      await screen.findByText("Enter the model name the server expects."),
    ).toBeVisible();
    expect(fake.writes).toEqual([]);
    field("Model", "qwen2.5-coder");
    fireEvent.click(
      within(page()).getByRole("button", {
        name: "Save",
      }),
    );
    expect(
      await screen.findByText(
        "Use letters, digits, '.', '-' and '_', up to 64 characters.",
      ),
    ).toBeVisible();
    expect(
      screen.getByText(
        "Use the server's http:// or https:// address, without a name or password.",
      ),
    ).toBeVisible();
    // Typed inline, the missing key names what is on screen (WEB-U-286).
    expect(
      screen.getByText(
        "Paste the service's key, or use an existing shared key.",
      ),
    ).toBeVisible();
  });

  it("deletes an unused route after a named confirmation, and is for administrators only", async () => {
    const fake = fakeRoutes({ routes: listed });
    show(fake.api);
    await screen.findByRole("table", { name: "Model routes" });
    choose("DeepSeek Flash", "Delete route DeepSeek Flash");
    const confirm = screen.getByRole("dialog", {
      name: "Delete route DeepSeek Flash?",
    });
    expect(
      within(confirm).getByText(
        "Tasks on this route will fail until they switch to another model.",
      ),
    ).toBeVisible();
    fireEvent.click(
      within(confirm).getByRole("button", { name: "Delete route" }),
    );
    expect(
      await screen.findByText("Deleted route DeepSeek Flash."),
    ).toBeVisible();
    expect(fake.writes).toEqual([
      { name: "deepseek-flash", version: 1, key: expect.any(String) as string },
    ]);
  });

  it("says plainly when routes cannot be loaded, and refuses members", async () => {
    const routes = vi.fn<RoutesApi["routes"]>(() =>
      Promise.reject(new ApiError(503, "store_unavailable", "x")),
    );
    show({ ...fakeRoutes().api, routes });
    expect(
      await screen.findByText("The model routes could not be loaded."),
    ).toBeVisible();
    routes.mockClear();
    show({ ...fakeRoutes().api, routes }, { ...admin, role: "member" });
    expect(
      screen.getByText("Only administrators can manage model routes."),
    ).toBeVisible();
    expect(routes).not.toHaveBeenCalled();
  });
});

// 099b-2: each route can be checked from the list; the check runs in its own dialog (WEB-U-039).
// The route form lists the saved route's models to pick Model from (WEB-U-067).
describe("WEB-U-067 List models in the route form", () => {
  it("offers it when editing, fills Model with the pick, and waits for an unsaved address to be saved", async () => {
    show(fakeRoutes({ routes: listed, secrets: ["secrets/local"] }).api);
    await screen.findByRole("table", { name: "Model routes" });
    await go("Edit route Local model");
    const dialog = page();
    fireEvent.click(
      within(dialog).getByRole("button", { name: "List models" }),
    );
    fireEvent.click(
      await within(dialog).findByRole("button", {
        name: "local-model-mini",
      }),
    );
    expect(within(dialog).getByLabelText("Model")).toHaveValue(
      "local-model-mini",
    );
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Change the address" }),
    );
    fireEvent.change(within(dialog).getByLabelText("Address"), {
      target: { value: "https://other.example.invalid/v1" },
    });
    expect(
      within(dialog).getByRole("button", { name: "List models" }),
    ).toBeDisabled();
  });

  it("is not offered for a new route", async () => {
    show(fakeRoutes({ routes: listed, secrets: ["secrets/local"] }).api);
    await screen.findByRole("table", { name: "Model routes" });
    await go("Add route");
    expect(
      within(page()).queryByRole("button", { name: "List models" }),
    ).toBeNull();
  });
});

describe("WEB-U-039 checking a route from the list", () => {
  it("opens the route's check, which runs only when asked, and closes back to the list", async () => {
    const fake = fakeRoutes({ routes: listed, secrets: ["secrets/local"] });
    show(fake.api);
    await screen.findByRole("table", { name: "Model routes" });
    choose("Local model", "Check route Local model");
    const dialog = screen.getByRole("dialog", {
      name: "Check route Local model",
    });
    expect(fake.writes).toEqual([]);
    fireEvent.click(within(dialog).getByRole("button", { name: "Run check" }));
    expect(
      await within(dialog).findByText("Verified: this route works for tasks."),
    ).toBeVisible();
    fireEvent.click(within(dialog).getByRole("button", { name: "Done" }));
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(screen.getByRole("table", { name: "Model routes" })).toBeVisible();
    // The list reads the stored check again, so the route shows it without a reload (GW-I-140).
    expect(
      await within(row(/Local model/)).findByText("Verified"),
    ).toBeVisible();
  });
});

// WEB-U-111 (the lead's first hour, F14): the README's quick-start route has no display name. It is shown by its
// model's name, as the model picker shows it, and editing any other field saves; a display name is asked for only when
// adding a route.
describe("WEB-U-111 a route without a display name", () => {
  const quick = route("quick-start", {
    display_name: null,
    model: "qwen3:4b",
    auth: { kind: "none" },
    key_ref: null,
  });

  it("is listed by its model's name, never its route name", async () => {
    show(fakeRoutes({ routes: [quick] }).api);
    await screen.findByRole("table", { name: "Model routes" });
    expect(row(/^Qwen3:4b/)).toBeVisible();
    expect(
      screen.getByRole("link", { name: "Edit route Qwen3:4b" }),
    ).toBeVisible();
    expect(screen.queryByText("quick-start", { exact: true })).toBeNull();
  });

  it("saves a change to another field, leaving the display name unset", async () => {
    const fake = fakeRoutes({ routes: [quick] });
    show(fake.api);
    await go("Edit route Qwen3:4b");
    expect(page()).toBeVisible();
    expect(within(page()).getByLabelText("Display name")).toHaveValue("");
    field("Max output", "8192");
    fireEvent.click(
      within(page()).getByRole("button", {
        name: "Save",
      }),
    );
    expect(await screen.findByText(/^Saved route Qwen3:4b\./)).toBeVisible();
    expect(fake.writes.at(-1)?.fields).toMatchObject({
      display_name: null,
      max_output: 8192,
    });
  });
});

// WEB-U-283 (the Models page review, P2 #7): with no route nobody can start a task, so the empty list says what to do.
describe("WEB-U-283 an installation without routes", () => {
  it("says to add one so people can start tasks", async () => {
    show(fakeRoutes({ routes: [] }).api);
    expect(
      await screen.findByText(
        "No model routes yet. Add one so people can start tasks.",
      ),
    ).toBeVisible();
    expect(screen.getByRole("link", { name: "Add route" })).toBeVisible();
  });
});

// WEB-U-285 (the Models page review, P2 #2 and #4): Address gives an example that fits where the model runs, and, while
// adding, Model says List models comes once the route is saved.
describe("WEB-U-285 the route form's hints", () => {
  it("fits the Address example to where the model runs, and says when List models comes", async () => {
    show(fakeRoutes({ routes: listed }).api);
    await screen.findByRole("table", { name: "Model routes" });
    await go("Add route");
    const address = within(page()).getByLabelText("Address");
    expect(address).toHaveAccessibleDescription(
      "The provider's API address, such as https://api.openai.com/v1.",
    );
    expect(within(page()).getByLabelText("Model")).toHaveAccessibleDescription(
      "The model's name as the server knows it. Once the route is saved, List models shows what the server offers.",
    );
    fireEvent.click(
      within(page()).getByRole("radio", { name: "A server on your network" }),
    );
    expect(address).toHaveAccessibleDescription(
      /^The server's address with its port, such as http:\/\/ollama:11434\/v1, .* Not localhost: here that is the gateway's own container\.$/,
    );
    fireEvent.click(
      within(page()).getByRole("radio", { name: "Something else" }),
    );
    expect(address).toHaveAccessibleDescription(
      "The server's API address, usually ending in /v1.",
    );
  });

  it("gives the general Address hint when editing, and no List models note", async () => {
    show(fakeRoutes({ routes: listed }).api);
    await go("Edit route Local model");
    fireEvent.click(
      within(page()).getByRole("button", { name: "Change the address" }),
    );
    expect(
      within(page()).getByLabelText("Address"),
    ).toHaveAccessibleDescription(
      "The server's API address, usually ending in /v1.",
    );
    expect(
      within(page()).getByLabelText("Model"),
    ).not.toHaveAccessibleDescription(/List models/);
  });
});

// WEB-U-286 (the Models page review, P2 #5): adding a hosted route with the Key left empty, the gateway's refusal
// names what is on screen, not a secret to choose or a setting under Advanced.
describe("WEB-U-286 adding a route without its key", () => {
  it("says to paste the key or use a shared one", async () => {
    const fake = fakeRoutes({ routes: listed });
    show(fake.api);
    await screen.findByRole("table", { name: "Model routes" });
    await go("Add route");
    field("Display name", "Hosted");
    field("Address", "https://models.example.invalid/v1");
    field("Model", "big-model");
    fireEvent.click(within(page()).getByRole("button", { name: "Save" }));
    expect(
      await within(page()).findByText(
        "Paste the service's key, or use an existing shared key.",
      ),
    ).toBeVisible();
    expect(
      within(page()).queryByText(
        "Choose the secret that holds the key, or send no key.",
      ),
    ).toBeNull();
  });
});

// WEB-U-293 (the Models page review, P3 #18): a route from the configuration file says it stays deleted after a
// restart, unless its entry in the file changes; one added here doesn't.
describe("WEB-U-293 deleting a route from the configuration file", () => {
  it("says it stays deleted after a restart", async () => {
    show(
      fakeRoutes({
        routes: [
          route("cfg", { display_name: "Config model", seed: "current" }),
          route("mine", { display_name: "My model" }),
        ],
      }).api,
    );
    await screen.findByRole("table", { name: "Model routes" });
    const seeded =
      "It comes from the configuration file: deleted here, it stays deleted after a restart unless its entry in the file changes.";
    choose("Config model", "Delete route Config model");
    expect(
      within(
        screen.getByRole("dialog", { name: "Delete route Config model?" }),
      ).getByText(seeded),
    ).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    choose("My model", "Delete route My model");
    expect(
      within(
        screen.getByRole("dialog", { name: "Delete route My model?" }),
      ).queryByText(seeded),
    ).toBeNull();
  });
});

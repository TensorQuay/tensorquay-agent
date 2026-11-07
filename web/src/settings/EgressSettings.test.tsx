import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { User } from "../protocol/generated/api";
import { fakeEgress, PRESET_RULES, rule } from "../test/egress";
import { LIMITS } from "../test/user";

import type { EgressApi } from "./admin/egress";
import { EgressSettings } from "./EgressSettings";

/**
 * The address as Next's router gives it (useSearchParams), which on a client navigation is the new page's before
 * window.location catches up (WEB-U-150); replace records where the page sends the address.
 */
const navigation = vi.hoisted(() => ({
  search: "",
  replace: vi.fn<(href: string) => void>(),
}));
vi.mock("next/navigation", () => ({
  useRouter: () => ({ replace: navigation.replace, push: vi.fn() }),
  usePathname: () => "/settings/internet-access",
  useSearchParams: () => new URLSearchParams(navigation.search),
}));

const admin: User = {
  id: "0195d8c0-0000-7000-8000-000000000001",
  email: "admin@example.invalid",
  role: "admin",
  internet: "none",
  limits: LIMITS,
  features: { agent_teams: false },
};
const listed = [
  rule("api.example.com", {
    ports: [443, 8443],
    seed: "current",
    last_set_by: "seed",
  }),
  rule("pypi.org"),
];
function show(api: EgressApi, me: User = admin) {
  const cache = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(
    <QueryClientProvider client={cache}>
      <EgressSettings me={me} api={api} />
    </QueryClientProvider>,
  );
}
const row = (host: string) =>
  screen.getByRole("row", { name: new RegExp(host) });
const findRow = (host: string) =>
  screen.findByRole("row", { name: new RegExp(host) });
const dialog = () => screen.getByRole("dialog");
const field = (label: string, value: string) =>
  fireEvent.change(within(dialog()).getByLabelText(label), {
    target: { value },
  });
const PORTS =
  "Enter 1 to 16 different ports from 1 to 65535, separated by commas.";

// Slice 4 (ADR-0028, decision B): the egress allow-list for administrators; no rule means no destination (WEB-U-040).
describe("WEB-U-040 egress rules", () => {
  it("lists hosts with their ports and source, and says what an empty list means", async () => {
    show(fakeEgress({ rules: listed }).api);
    await screen.findByRole("table", { name: "Allowed sites" });
    expect(within(row("api.example.com")).getByText("443, 8443")).toBeVisible();
    expect(
      within(row("api.example.com")).getByText("Configuration file"),
    ).toBeVisible();
    expect(within(row("pypi.org")).getByText("Added here")).toBeVisible();
    // WEB-U-162 (the 13f screens, #10): the rules have a heading, and Delete is under each row's ⋯, never bare.
    expect(
      screen.getByRole("heading", { name: "Allowed sites" }),
    ).toBeVisible();
    // No second landmark by that name around the table's own scroll region (axe: landmark-unique; the e2e's axe
    // checks it in a browser, where the scroll region exists).
    expect(
      screen
        .queryAllByRole("region", { name: "Allowed sites" })
        .filter((region) => !region.classList.contains("data-table-scroll")),
    ).toHaveLength(0);
    expect(
      screen.getByRole("button", { name: "More actions for rule pypi.org" }),
    ).toBeVisible();
    expect(
      screen.queryByRole("button", { name: "Delete rule pypi.org" }),
    ).toBeNull();
    expect(
      screen.getAllByRole("columnheader").map((header) => header.textContent),
    ).toEqual(["Host", "Ports", "Source", "Actions"]);
  });

  it("says an empty list reaches nothing, and that a disabled egress takes no rules", async () => {
    show(fakeEgress().api);
    expect(
      await screen.findByText(
        "No sites allowed yet: environments can reach nothing outside.",
      ),
    ).toBeVisible();
    expect(
      await screen.findByRole("button", { name: "Add a site" }),
    ).toBeEnabled();
  });

  it("shows egress turned off by configuration with its rules and no way to change them", async () => {
    show(fakeEgress({ rules: listed, disabled: true }).api);
    expect(
      await screen.findByText(
        "Internet access is turned off in the gateway configuration: environments are internal-only and no rule applies.",
      ),
    ).toBeVisible();
    expect(screen.queryByRole("button", { name: "Add a site" })).toBeNull();
    expect(screen.queryByRole("button", { name: /^Edit rule/ })).toBeNull();
    expect(
      screen.queryByRole("button", { name: /^More actions for rule/ }),
    ).toBeNull();
  });

  // WEB-U-421 (c16k real-world p02): Add a site opened with Ports empty, so the usual site needed 443 typed in.
  it("starts a new site at port 443", async () => {
    const fake = fakeEgress({ rules: listed });
    show(fake.api);
    await screen.findByRole("table", { name: "Allowed sites" });
    fireEvent.click(await screen.findByRole("button", { name: "Add a site" }));
    expect(within(dialog()).getByLabelText("Ports")).toHaveValue("443");
    field("Host", "new.example.com");
    fireEvent.click(
      within(dialog()).getByRole("button", { name: "Save rule" }),
    );
    expect(
      await screen.findByText("Saved rule new.example.com."),
    ).toBeVisible();
    expect(fake.writes).toEqual([
      { host: "new.example.com", ports: [443], internal: false },
    ]);
  });

  it("adds a rule with the ports as typed, after checking them before anything is sent", async () => {
    const fake = fakeEgress({ rules: listed });
    show(fake.api);
    await screen.findByRole("table", { name: "Allowed sites" });
    fireEvent.click(await screen.findByRole("button", { name: "Add a site" }));
    field("Host", "Files.Example.com");
    const save = within(dialog()).getByRole("button", { name: "Save rule" });
    for (const ports of [
      "",
      "0",
      "70000",
      "443, 443",
      "https",
      "1,".repeat(17),
    ]) {
      field("Ports", ports);
      fireEvent.click(save);
      expect(await within(dialog()).findByText(PORTS)).toBeVisible();
    }
    expect(fake.writes).toEqual([]);
    field("Ports", "8443, 443");
    fireEvent.click(save);
    expect(
      await screen.findByText("Saved rule files.example.com."),
    ).toBeVisible();
    expect(fake.writes).toEqual([
      { host: "Files.Example.com", ports: [8443, 443], internal: false },
    ]);
    expect(
      within(row("files.example.com")).getByText("443, 8443"),
    ).toBeVisible();
  });

  it("names the gateway's host problem and a full list", async () => {
    const full = fakeEgress({ rules: listed, limit: 2 });
    show(full.api);
    await screen.findByRole("table", { name: "Allowed sites" });
    fireEvent.click(await screen.findByRole("button", { name: "Add a site" }));
    field("Host", "bad host");
    field("Ports", "443");
    fireEvent.click(
      within(dialog()).getByRole("button", { name: "Save rule" }),
    );
    expect(
      await within(dialog()).findByText(
        "Enter a host name or address, like api.example.com or 10.0.0.5.",
      ),
    ).toBeVisible();
    field("Host", "new.example.com");
    fireEvent.click(
      within(dialog()).getByRole("button", { name: "Save rule" }),
    );
    expect(
      await within(dialog()).findByText(
        "The allow-list is full (2 rules). Delete a rule to add another.",
      ),
    ).toBeVisible();
  });

  it("edits the ports with the version read and keeps the edits after another admin's change", async () => {
    const fake = fakeEgress({ rules: listed });
    show(fake.api);
    await screen.findByRole("table", { name: "Allowed sites" });
    fireEvent.click(screen.getByRole("button", { name: "Edit rule pypi.org" }));
    expect(
      screen.getByRole("dialog", { name: "Edit rule pypi.org" }),
    ).toBeVisible();
    expect(within(dialog()).queryByLabelText("Host")).toBeNull();
    field("Ports", "443, 80");
    fake.touch("pypi.org");
    fireEvent.click(
      within(dialog()).getByRole("button", { name: "Save rule" }),
    );
    expect(
      await within(dialog()).findByText(
        "Someone changed this rule since you opened it. Your edits are kept: save again to replace their change with yours.",
      ),
    ).toBeVisible();
    expect(within(dialog()).getByLabelText("Ports")).toHaveValue("443, 80");
    fireEvent.click(
      within(dialog()).getByRole("button", { name: "Save rule" }),
    );
    expect(await screen.findByText("Saved rule pypi.org.")).toBeVisible();
    expect(fake.writes.map((write) => write.version)).toEqual([1, 2]);
  });

  it("deletes a rule after a named confirmation", async () => {
    const deleteRule = vi.fn(() => Promise.resolve());
    show({ ...fakeEgress({ rules: listed }).api, deleteRule });
    await screen.findByRole("table", { name: "Allowed sites" });
    fireEvent.click(
      screen.getByRole("button", { name: "More actions for rule pypi.org" }),
    );
    // Shown as "Delete…", named for what it deletes, as in every settings table's menu.
    const item = screen.getByRole("menuitem", { name: "Delete rule pypi.org" });
    expect(item).toHaveTextContent("Delete…");
    expect(item).toHaveAttribute("data-danger", "true");
    fireEvent.click(item);
    const confirm = screen.getByRole("dialog", {
      name: "Delete rule pypi.org?",
    });
    expect(confirm).toHaveTextContent(
      "Environments' next connections to this site will be refused.",
    );
    fireEvent.click(
      within(confirm).getByRole("button", { name: "Delete rule" }),
    );
    expect(await screen.findByText("Deleted rule pypi.org.")).toBeVisible();
    expect(deleteRule).toHaveBeenCalledWith("pypi.org", 1, expect.any(String));
  });

  it("says plainly when the list cannot be loaded, and is for administrators only", async () => {
    show({
      ...fakeEgress().api,
      rules: () => Promise.reject(new Error("offline")),
    });
    expect(
      await screen.findByText("The allowed sites could not be loaded."),
    ).toBeVisible();
  });

  it("is for administrators only", () => {
    show(fakeEgress().api, { ...admin, role: "member" });
    expect(
      screen.getByText("Only administrators can manage internet access."),
    ).toBeVisible();
  });
});

// WEB-U-118 (the first hour's F20): a fresh install reaches nothing; ready-made lists let an administrator allow what
// tasks usually need in one step, each saying what it allows, with Open web asked first and never called read-only.
describe("WEB-U-118 ready-made lists", () => {
  const lists = () => screen.getByRole("region", { name: "Ready-made lists" });
  const card = (name: RegExp) => within(lists()).getByRole("group", { name });

  it("offers each list with what it allows, in order, and Custom for any other site", async () => {
    show(fakeEgress().api);
    await screen.findByRole("region", { name: "Ready-made lists" });
    expect(
      within(lists())
        .getAllByRole("group")
        .map((group) => group.getAttribute("aria-label")),
    ).toEqual([
      "Package registries",
      "Code hosting",
      "Open web: any website (ports 80 and 443)",
      "Custom",
    ]);
    expect(card(/^Package registries/)).toHaveTextContent(
      "Install Python, Node.js, Rust and Go packages. 8 sites.",
    );
    expect(card(/^Code hosting/)).toHaveTextContent(
      "Download code from GitHub and GitLab. 5 sites.",
    );
    const web = card(/^Open web/);
    expect(web).toHaveTextContent(
      "The agent can reach any public website, and so can send data to any site. Private and internal addresses stay blocked.",
    );
    expect(document.body).not.toHaveTextContent(/read-only/i);
    // One way to add any other site: Custom's, not a second Add rule above the lists.
    expect(screen.queryByRole("button", { name: "Add rule" })).toBeNull();
    // Custom: any other site, by the same dialog as Add rule.
    expect(card(/^Custom/)).toHaveTextContent(
      "Any other site, with the ports you choose.",
    );
    fireEvent.click(
      within(card(/^Custom/)).getByRole("button", { name: "Add a site" }),
    );
    expect(screen.getByRole("dialog", { name: "Add a rule" })).toBeVisible();
  });

  it("adds a list, marks its rules with its name, and removes them again", async () => {
    const fake = fakeEgress({ rules: [rule("pypi.org")] });
    show(fake.api);
    await screen.findByRole("region", { name: "Ready-made lists" });
    fireEvent.click(
      within(card(/^Package registries/)).getByRole("button", {
        name: "Add Package registries",
      }),
    );
    expect(
      await screen.findByText(
        "Added Package registries: 7 sites. Already allowed and kept as they were: pypi.org.",
      ),
    ).toBeVisible();
    expect(fake.applies).toEqual(["package_registries"]);
    expect(
      within(row("^crates\\.io")).getByText("Package registries"),
    ).toBeVisible();
    expect(within(row("pypi.org")).getByText("Added here")).toBeVisible();
    // Your own pypi.org rule stays yours: the list is only partly in the allow-list.
    expect(
      await within(card(/^Package registries/)).findByText(
        "Some of its sites were changed or removed.",
      ),
    ).toBeVisible();
    fireEvent.click(
      within(card(/^Package registries/)).getByRole("button", {
        name: "Remove Package registries",
      }),
    );
    expect(
      await screen.findByText(
        "Removed Package registries. Changed by you, so kept: pypi.org.",
      ),
    ).toBeVisible();
    expect(fake.removals).toEqual(["package_registries"]);
    expect(screen.queryByRole("row", { name: /crates\.io/ })).toBeNull();
  });

  it("says so when a list can't be added, and adds none of its sites", async () => {
    const fake = fakeEgress({ rules: [] });
    vi.spyOn(fake.api, "applyPreset").mockRejectedValueOnce(
      new Error("unreachable"),
    );
    show(fake.api);
    await screen.findByRole("region", { name: "Ready-made lists" });
    fireEvent.click(
      within(card(/^Package registries/)).getByRole("button", {
        name: "Add Package registries",
      }),
    );
    expect(
      await screen.findByText("The list could not be changed. Try again."),
    ).toBeVisible();
    expect(fake.applies).toEqual([]);
    expect(screen.queryByRole("row", { name: /crates\.io/ })).toBeNull();
    expect(
      within(card(/^Package registries/)).getByRole("button", {
        name: "Add Package registries",
      }),
    ).toBeEnabled();
  });

  it("asks before allowing any website, saying what that means", async () => {
    const fake = fakeEgress();
    show(fake.api);
    await screen.findByRole("region", { name: "Ready-made lists" });
    fireEvent.click(
      within(card(/^Open web/)).getByRole("button", {
        name: "Add Open web: any website (ports 80 and 443)",
      }),
    );
    const ask = screen.getByRole("dialog", { name: "Allow any website?" });
    expect(ask).toHaveTextContent(
      "Tasks' agents can then reach any public website on ports 80 and 443, and can send data to it. Private and internal addresses stay blocked.",
    );
    fireEvent.click(within(ask).getByRole("button", { name: "Cancel" }));
    expect(fake.applies).toEqual([]);
    fireEvent.click(
      within(card(/^Open web/)).getByRole("button", {
        name: "Add Open web: any website (ports 80 and 443)",
      }),
    );
    fireEvent.click(
      within(screen.getByRole("dialog")).getByRole("button", {
        name: "Allow any website",
      }),
    );
    expect(
      await screen.findByText(
        "Added Open web: any website (ports 80 and 443).",
      ),
    ).toBeVisible();
    expect(await within(card(/^Open web/)).findByText("On")).toBeVisible();
  });

  it("offers no lists while internet access is turned off by configuration", async () => {
    show(fakeEgress({ disabled: true }).api);
    await screen.findByText(/turned off/i);
    expect(
      screen.queryByRole("region", { name: "Ready-made lists" }),
    ).toBeNull();
  });
});

// WEB-U-150: a key's "Allow {host}" link (Keys › Mine) opens Internet access with ?host= (and ?port=), so the
// administrator lands on the one thing to do, not a page to search. The router's address carries the parameters; the
// window's doesn't yet (a click from Keys, before Next updates the history).
describe("WEB-U-150 opened for a host", () => {
  const at = (search: string) => {
    navigation.search = search;
    navigation.replace.mockClear();
  };
  afterEach(() => at(""));

  it("opens Add a site with the host and port 443, then drops the parameters", async () => {
    at("?host=api.github.com");
    show(fakeEgress({ rules: [rule("pypi.org")] }).api);
    const add = await screen.findByRole("dialog", { name: "Add a rule" });
    expect(within(add).getByLabelText("Host")).toHaveValue("api.github.com");
    expect(within(add).getByLabelText("Ports")).toHaveValue("443");
    expect(navigation.replace).toHaveBeenCalledWith(
      "/settings/internet-access",
    );
  });

  it("takes the port it is given", async () => {
    at("?host=git.acme.lan&port=8443");
    show(fakeEgress().api);
    const add = await screen.findByRole("dialog", { name: "Add a rule" });
    expect(within(add).getByLabelText("Ports")).toHaveValue("8443");
  });

  // ADR-0037 Amendment 2: a key for an exact address needs that address's rule, IPv6 too.
  it("opens Add a site for an exact address", async () => {
    at("?host=fd00%3A%3A5&port=8443");
    show(fakeEgress().api);
    const add = await screen.findByRole("dialog", { name: "Add a rule" });
    expect(within(add).getByLabelText("Host")).toHaveValue("fd00::5");
    expect(within(add).getByLabelText("Ports")).toHaveValue("8443");
  });

  it("opens the host's own rule with the asked port added, as a change to save", async () => {
    at("?host=git.acme.lan&port=8443");
    const fake = fakeEgress({ rules: [rule("git.acme.lan")] });
    show(fake.api);
    const edit = await screen.findByRole("dialog", { name: /git\.acme\.lan/ });
    expect(within(edit).getByLabelText("Ports")).toHaveValue("443, 8443");
    fireEvent.click(within(edit).getByRole("button", { name: "Save rule" }));
    expect(await screen.findByText("Saved rule git.acme.lan.")).toBeVisible();
    expect(fake.writes.at(-1)).toMatchObject({
      host: "git.acme.lan",
      ports: [443, 8443],
    });
  });

  it("opens nothing when the host's rule already has the port", async () => {
    at("?host=pypi.org");
    show(fakeEgress({ rules: [rule("pypi.org", { ports: [443, 8443] })] }).api);
    await screen.findByRole("table", { name: "Allowed sites" });
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("stays closed once closed", async () => {
    at("?host=api.github.com");
    show(fakeEgress().api);
    const add = await screen.findByRole("dialog", { name: "Add a rule" });
    fireEvent.click(within(add).getByRole("button", { name: "Cancel" }));
    await screen.findByRole("region", { name: "Ready-made lists" });
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("opens nothing for an invalid host, with egress turned off, or for a member", async () => {
    for (const [search, api, who] of [
      ["?host=not%20a%20host", fakeEgress().api, admin],
      // An IPv6 address only in its one spelling, as the key's link writes it.
      ["?host=FD00%3A0%3A%3A5", fakeEgress().api, admin],
      ["?host=api.github.com", fakeEgress({ disabled: true }).api, admin],
      [
        "?host=api.github.com",
        fakeEgress().api,
        { ...admin, role: "member" as const },
      ],
    ] as const) {
      at(search);
      const view = show(api, who);
      // Wait for the page to settle, whichever way it opens.
      await screen.findAllByText(
        /Only administrators|turned off|No sites allowed yet/,
      );
      expect(screen.queryByRole("dialog")).toBeNull();
      view.unmount();
    }
  });
});

// The 13f screen review, #10: a ready-made list's card didn't say whether it was on (only an Add button did). Each
// card says On, Partly on or Off.
describe("WEB-U-162 each ready-made list says whether it is on", () => {
  it("says On, Partly on or Off", async () => {
    show(
      fakeEgress({
        rules: [
          ...PRESET_RULES.code_hosting.map(({ host }) =>
            rule(host, { preset: "code_hosting" }),
          ),
          rule("pypi.org", { preset: "package_registries" }),
        ],
      }).api,
    );
    const lists = await screen.findByRole("region", {
      name: "Ready-made lists",
    });
    const state = (name: RegExp) =>
      within(within(lists).getByRole("group", { name })).getByRole("status");
    expect(state(/^Code hosting/)).toHaveTextContent("On");
    expect(state(/^Package registries/)).toHaveTextContent("Partly on");
    expect(state(/^Open web/)).toHaveTextContent("Off");
  });
});

// WEB-U-188 (ADR-0035's amendment, 1 Oct 2026): a rule for one exact host name may be marked an internal server, so
// that name reaches an address on the installation's own network; never a wildcard or an address, and an internal
// rule the gateway can't use yet says why.
describe("WEB-U-188 an internal server's rule", () => {
  const internalBox = () =>
    within(dialog()).getByRole("checkbox", { name: "Internal server" });

  it("adds a rule for an internal server, and shows it in the list", async () => {
    const fake = fakeEgress({ rules: listed });
    show(fake.api);
    fireEvent.click(await screen.findByRole("button", { name: "Add a site" }));
    field("Host", "git.corp.example");
    field("Ports", "443");
    expect(internalBox()).not.toBeChecked();
    expect(
      within(dialog()).getByText(
        "Lets tasks reach this name at an address on your own network, such as 10.1.2.3. Never this machine, the container engine or cloud metadata.",
      ),
    ).toBeVisible();
    fireEvent.click(internalBox());
    fireEvent.click(
      within(dialog()).getByRole("button", { name: "Save rule" }),
    );
    expect(
      await screen.findByText("Saved rule git.corp.example."),
    ).toBeVisible();
    expect(fake.writes).toEqual([
      { host: "git.corp.example", ports: [443], internal: true },
    ]);
    expect(
      within(row("git.corp.example")).getByText("Internal server"),
    ).toBeVisible();
    expect(within(row("pypi.org")).queryByText("Internal server")).toBeNull();
  });

  it("offers no internal choice for a wildcard or an address, saying why", async () => {
    show(fakeEgress().api);
    fireEvent.click(await screen.findByRole("button", { name: "Add a site" }));
    for (const host of ["*.corp.example", "10.1.2.3", "fd00::5"]) {
      field("Host", host);
      expect(internalBox()).toBeDisabled();
      expect(internalBox()).not.toBeChecked();
      expect(
        within(dialog()).getByText(
          "Only for one exact host name. A rule for an address already reaches it.",
        ),
      ).toBeVisible();
    }
  });

  it("sends a name typed back from a wildcard as not internal", async () => {
    const fake = fakeEgress();
    show(fake.api);
    fireEvent.click(await screen.findByRole("button", { name: "Add a site" }));
    field("Host", "git.corp.example");
    fireEvent.click(internalBox());
    field("Host", "*.corp.example");
    field("Ports", "443");
    fireEvent.click(
      within(dialog()).getByRole("button", { name: "Save rule" }),
    );
    expect(await screen.findByText("Saved rule *.corp.example.")).toBeVisible();
    expect(fake.writes).toEqual([
      { host: "*.corp.example", ports: [443], internal: false },
    ]);
  });

  it("names the gateway's refusal at the choice", async () => {
    const fake = fakeEgress({ refuseInternal: true });
    show(fake.api);
    fireEvent.click(await screen.findByRole("button", { name: "Add a site" }));
    field("Host", "git.corp.example");
    field("Ports", "443");
    fireEvent.click(internalBox());
    fireEvent.click(
      within(dialog()).getByRole("button", { name: "Save rule" }),
    );
    expect(
      await within(dialog()).findByText(
        "Only for one exact host name. A rule for an address already reaches it.",
      ),
    ).toBeVisible();
  });

  it("turns a rule's internal choice off when it is edited", async () => {
    const fake = fakeEgress({
      rules: [rule("git.corp.example", { internal: true })],
    });
    show(fake.api);
    fireEvent.click(
      await screen.findByRole("button", { name: "Edit rule git.corp.example" }),
    );
    expect(internalBox()).toBeChecked();
    fireEvent.click(internalBox());
    fireEvent.click(
      within(dialog()).getByRole("button", { name: "Save rule" }),
    );
    expect(
      await screen.findByText("Saved rule git.corp.example."),
    ).toBeVisible();
    expect(fake.writes).toEqual([
      { host: "git.corp.example", ports: [443], internal: false, version: 1 },
    ]);
  });

  it("says why an internal rule can't be used yet", async () => {
    show(
      fakeEgress({
        rules: [
          rule("git.corp.example", {
            internal: true,
            internal_state: "internal_never_missing",
          }),
          rule("wiki.corp.example", {
            internal: true,
            internal_state: "engine_unknown",
          }),
        ],
      }).api,
    );
    expect(
      await within(await findRow("git.corp.example")).findByText(
        "Not in use yet: name this machine's own addresses in gateway.toml ([egress] internal_never).",
      ),
    ).toBeVisible();
    expect(
      within(row("wiki.corp.example")).getByText(
        "Not in use for now: the gateway can't read the container engine's networks. It tries again every minute.",
      ),
    ).toBeVisible();
    expect(document.body).not.toHaveTextContent(
      /internal_never_missing|engine_unknown/,
    );
  });
});

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { ApiError } from "../api/error";
import type { User } from "../protocol/generated/api";
import { fakeCredentials, userConnector } from "../test/connector-credentials";
import { fakeHostKeys, hostKey } from "../test/host-keys";
import { LIMITS } from "../test/user";

import type { CredentialsApi } from "./connector-credentials";
import type { HostKeysApi } from "./host-keys/contract";
import { KeysSettings } from "./KeysSettings";

vi.mock("next/navigation", () => ({ usePathname: () => "/settings/keys" }));

const member: User = {
  id: "0195d8c0-0000-7000-8000-000000000002",
  email: "member@example.invalid",
  role: "member",
  internet: "none",
  limits: LIMITS,
  features: { agent_teams: false },
};
const admin: User = {
  ...member,
  role: "admin",
  email: "admin@example.invalid",
};
// Synthetic markers, never real keys: the page must never show them back.
const SECRET = "tok-9f8e7d6c5b4a";
/** Every path the page asks the gateway for directly (WEB-U-149: never a Git credential's). */
const requested: string[] = [];

beforeEach(() => {
  requested.length = 0;
  vi.stubGlobal(
    "fetch",
    vi.fn<typeof globalThis.fetch>((input) => {
      requested.push(new URL(input as string, "http://localhost").pathname);
      return Promise.resolve(Response.json({}, { status: 404 }));
    }),
  );
});
afterEach(() => vi.unstubAllGlobals());

const listed = [
  userConnector("tracker", { display_name: "Issue tracker", required: true }),
  userConnector("wiki", {
    scheme: "header",
    has_credential: true,
    credential_updated_at: "2026-09-20T10:00:00Z",
  }),
  userConnector("files", { scheme: "none" }),
  userConnector("mail"),
];
function show(
  me: User = member,
  connectors: CredentialsApi = fakeCredentials(listed).api,
) {
  const cache = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  cache.setQueryData(["session"], me);
  render(
    <QueryClientProvider client={cache}>
      <KeysSettings me={me} connectors={connectors} />
    </QueryClientProvider>,
  );
}
function showWithHostKeys(hostKeys: HostKeysApi) {
  const cache = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  cache.setQueryData(["session"], member);
  render(
    <QueryClientProvider client={cache}>
      <KeysSettings
        me={member}
        connectors={fakeCredentials(listed).api}
        hostKeys={hostKeys}
      />
    </QueryClientProvider>,
  );
}
const table = () => screen.findByRole("table", { name: "Your keys" });
const row = (name: RegExp) => screen.getByRole("row", { name });
const dialog = () => screen.getByRole("dialog");

// The owner's direction (c8): people never need to know git. One page for a person's keys, listed by the service each
// is for, one main action, and an administrator's shared keys one switch away (WEB-U-076, UI-GUIDELINES §7).
describe("WEB-U-076 keys, by service", () => {
  it("lists every key in one table by its service, with no git words", async () => {
    show();
    // WEB-U-262 (a first-hour check before 17.0): the connectors' keys have their own heading and say what they are
    // for.
    expect(
      screen.getByRole("heading", { level: 2, name: "Keys for connectors" }),
    ).toBeVisible();
    expect(
      screen.getByText(
        "For a connector your administrator set up that works as you, such as a team's issue tracker: it needs your own key. The gateway sends each key only to its connector; nobody can read it back.",
      ),
    ).toBeInTheDocument();
    const keys = await table();
    await within(keys).findByText("Issue tracker");
    // Sorted by service; a connector that takes no key isn't listed.
    expect(
      within(keys)
        .getAllByRole("row")
        .slice(1)
        .map((line) => line.querySelector(".data-name span")?.textContent),
    ).toEqual(["Issue tracker", "mail", "wiki"]);
    expect(
      within(row(/Issue tracker/)).getByText("For a connector"),
    ).toBeVisible();
    expect(
      screen.queryByRole("heading", { name: /Git|Connectors/ }),
    ).toBeNull();
    expect(document.body.textContent).not.toMatch(
      /\bgit\b|repositor|credential|branch|push|remote/i,
    );
    // One main action; a member switches to the shared keys they may use, which they read (WEB-U-146).
    const primary = screen
      .getAllByRole("button")
      .filter((button) => button.getAttribute("data-variant") === "primary");
    expect(primary.map((button) => button.textContent)).toEqual([
      "Add a key for a connector",
    ]);
    expect(screen.getByRole("link", { name: "Shared" })).toHaveAttribute(
      "href",
      "/settings/keys/shared",
    );
  });

  // WEB-U-262: with no connector taking a person's key, the section says so in words that can't read as "no keys" beside
  // a web-service key just saved, and offers nothing to add.
  it("says no connector needs a key of yours, and offers no add, when none takes one", async () => {
    show(
      member,
      fakeCredentials([userConnector("files", { scheme: "none" })]).api,
    );
    expect(
      await screen.findByText(
        "None of the connectors here needs a key of yours.",
      ),
    ).toBeVisible();
    expect(screen.queryByRole("table")).toBeNull();
    expect(
      screen.queryByRole("button", { name: "Add a key for a connector" }),
    ).toBeNull();
  });

  it("asks which service a new key is for: the connectors that take one, the needed first", async () => {
    show();
    await table();
    await screen.findByText("Issue tracker");
    fireEvent.click(
      screen.getByRole("button", { name: "Add a key for a connector" }),
    );
    const ask = screen.getByRole("dialog", {
      name: "Add a key for a connector",
    });
    expect(within(ask).getByText("Which service is it for?")).toBeVisible();
    expect(
      within(ask)
        .getAllByRole("button")
        // The dialog's own close button is an icon, with no words.
        .filter((button) => button.textContent !== "")
        .map((button) => button.textContent),
    ).toEqual(["Issue tracker", "wiki", "mail"]);
    fireEvent.click(within(ask).getByRole("button", { name: "Issue tracker" }));
    expect(
      await screen.findByRole("dialog", {
        name: "Add your key for Issue tracker",
      }),
    ).toBeVisible();
  });

  it("gives an administrator Mine and Shared", () => {
    show(admin);
    const views = screen.getByRole("navigation", { name: "Keys" });
    expect(within(views).getByRole("link", { name: "Mine" })).toHaveAttribute(
      "aria-current",
      "page",
    );
    expect(within(views).getByRole("link", { name: "Shared" })).toHaveAttribute(
      "href",
      "/settings/keys/shared",
    );
  });
});

describe("WEB-U-044 keys for connectors", () => {
  it("says for each connector whether a key is needed, added or optional", async () => {
    show();
    await table();
    await screen.findByText("Issue tracker");
    expect(within(row(/Issue tracker/)).getByText("Needed")).toBeVisible();
    // People see the connector's name, never its internal id.
    expect(within(row(/Issue tracker/)).queryByText("tracker")).toBeNull();
    expect(
      within(row(/Issue tracker/))
        .getAllByRole("button")
        .map((button) => button.getAttribute("aria-label")),
    ).toEqual(["Add your key for Issue tracker"]);
    expect(within(row(/wiki/)).getByText("Added")).toBeVisible();
    expect(
      within(row(/wiki/))
        .getAllByRole("button")
        .map((button) => button.getAttribute("aria-label")),
    ).toEqual(["Replace your key for wiki", "More actions for your wiki key"]);
    fireEvent.click(
      within(row(/wiki/)).getByRole("button", {
        name: "More actions for your wiki key",
      }),
    );
    expect(
      screen
        .getAllByRole("menuitem")
        .map((item) => item.getAttribute("aria-label")),
    ).toEqual(["Show reads of your wiki key", "Remove your key for wiki"]);
    fireEvent.keyDown(document.activeElement ?? document.body, {
      key: "Escape",
    });
    expect(within(row(/mail/)).getByText("Optional")).toBeVisible();
  });

  it("saves a value once, checks it first, and never shows it again", async () => {
    const fake = fakeCredentials(listed);
    show(member, fake.api);
    await table();
    fireEvent.click(
      await screen.findByRole("button", {
        name: "Add your key for Issue tracker",
      }),
    );
    const key = within(dialog()).getByLabelText("Key");
    expect(key).toHaveAttribute("type", "password");
    fireEvent.change(key, { target: { value: "has space" } });
    fireEvent.click(within(dialog()).getByRole("button", { name: "Save key" }));
    expect(
      within(dialog()).getByText(
        "Use 1 to 4096 printable characters without spaces.",
      ),
    ).toBeVisible();
    expect(fake.values.size).toBe(0);
    fireEvent.change(key, { target: { value: SECRET } });
    fireEvent.click(within(dialog()).getByRole("button", { name: "Save key" }));
    expect(
      await screen.findByText("Saved your key for Issue tracker."),
    ).toBeVisible();
    expect(fake.values.get("tracker")).toBe(SECRET);
    expect(document.body.innerHTML).not.toContain(SECRET);
    expect(
      await within(row(/Issue tracker/)).findByText("Added"),
    ).toBeVisible();
    fireEvent.click(
      screen.getByRole("button", { name: "Replace your key for wiki" }),
    );
    expect(within(dialog()).getByLabelText("Header value")).toHaveValue("");
  });

  it("removes after confirmation, and explains a refusal where the person is looking", async () => {
    const fake = fakeCredentials(listed);
    show(member, fake.api);
    await table();
    fireEvent.click(
      await screen.findByRole("button", {
        name: "More actions for your wiki key",
      }),
    );
    fireEvent.click(
      screen.getByRole("menuitem", { name: "Remove your key for wiki" }),
    );
    expect(dialog()).toHaveTextContent(
      "The shared key applies again, if the administrator set one.",
    );
    fireEvent.click(
      within(dialog()).getByRole("button", { name: "Remove key" }),
    );
    expect(await screen.findByText("Removed your key for wiki.")).toBeVisible();
    vi.spyOn(fake.api, "putCredential").mockRejectedValue(
      new ApiError(403, "session_required", "x"),
    );
    fireEvent.click(
      screen.getByRole("button", { name: "Add your key for mail" }),
    );
    fireEvent.change(within(dialog()).getByLabelText("Key"), {
      target: { value: SECRET },
    });
    fireEvent.click(within(dialog()).getByRole("button", { name: "Save key" }));
    expect(
      await within(dialog()).findByText(
        "Saving a key needs a browser session. Sign in again.",
      ),
    ).toBeVisible();
  });
});

// Host keys (ADR-0037, slice 1): the Keys page lists the person's keys for web services when the gateway has them.
describe("WEB-U-141 the Keys page with host keys", () => {
  // UI-GUIDELINES §7: an empty state's one action is the primary, and a view has one primary (17.3): the web services'
  // Add, first on the page, empty or not; the connectors' below it is secondary.
  it.each([
    ["no keys yet", []],
    ["a key", [hostKey({ name: "GITHUB_WORK" })]],
  ])(
    "has one primary Add with %s for web services, theirs",
    async (_, keys) => {
      const { api } = fakeHostKeys({ keys });
      showWithHostKeys(api);
      expect(
        await screen.findByRole("link", {
          name: "Add a key for a web service",
        }),
      ).toHaveAttribute("data-variant", "primary");
      expect(
        await screen.findByRole("button", {
          name: "Add a key for a connector",
        }),
      ).toHaveAttribute("data-variant", "secondary");
      expect(
        document.querySelectorAll('[data-variant="primary"]'),
      ).toHaveLength(1);
    },
  );

  it("lists the person's keys for web services, above the rest", async () => {
    const { api } = fakeHostKeys({ keys: [hostKey({ name: "GITHUB_WORK" })] });
    showWithHostKeys(api);
    expect(
      await screen.findByRole("table", { name: "Your keys for web services" }),
    ).toHaveTextContent("GITHUB_WORK");
  });
});

// ADR-0037 §7: a key for a Git host is a key for a web service. The gateway copies each Git credential into a host key
// and keeps its old routes read-only for one release, so the page never asks for them: no project rows, no project in
// Add a key, nothing to fail.
describe("WEB-U-149 keys without project keys", () => {
  it("never asks the gateway for Git credentials, and offers no project", async () => {
    show();
    await table();
    expect(await screen.findByText("Issue tracker")).toBeVisible();
    expect(requested.filter((path) => path.includes("/git/"))).toEqual([]);
    fireEvent.click(
      screen.getByRole("button", { name: "Add a key for a connector" }),
    );
    const ask = screen.getByRole("dialog", {
      name: "Add a key for a connector",
    });
    expect(within(ask).queryByRole("button", { name: /project/i })).toBeNull();
  });

  it("says the keys couldn't be loaded when the connectors' keys can't be", async () => {
    show(member, {
      ...fakeCredentials(listed).api,
      connectors: () =>
        Promise.reject(new ApiError(503, "store_unavailable", "x")),
    });
    expect(
      await screen.findByText("Your keys could not be loaded."),
    ).toBeVisible();
  });
});

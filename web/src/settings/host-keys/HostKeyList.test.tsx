import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, within } from "@testing-library/react";
import type { ReactNode } from "react";
import { describe, expect, it } from "vitest";

import { fakeHostKeys, hostKey } from "../../test/host-keys";

import type { HostKeysApi } from "./contract";
import { HOST_KEYS_NOTICE, HostKeyList } from "./HostKeyList";

/** The reader's clock for "Today": 30 Sep 2026, 21:00 UTC. */
const now = () => new Date("2026-09-30T21:00:00Z");

function show(api: HostKeysApi, admin = false, notice?: string) {
  const cache = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  if (notice) cache.setQueryData(HOST_KEYS_NOTICE, notice);
  const wrap = (children: ReactNode) => (
    <QueryClientProvider client={cache}>{children}</QueryClientProvider>
  );
  // Times as a reader in UTC sees them; the page uses the reader's own zone.
  return render(
    wrap(<HostKeyList api={api} admin={admin} now={now} timeZone="UTC" />),
  );
}
const rows = async () =>
  within(
    await screen.findByRole("table", { name: "Your keys for web services" }),
  )
    .getAllByRole("row")
    .slice(1);

// ADR-0037 and the reviewed Keys sketch §2: a person's keys by the name their tasks read, the web address, and when
// each was last used and by which task; never a value.
describe("WEB-U-141 the Mine list of host keys", () => {
  // Found at 390 px, where the settings menu's own headings aren't on the page (axe heading-order, WEB-U-141): the
  // section is the page's second level, as on every settings list.
  it("heads its section at the page's second level", async () => {
    show(fakeHostKeys().api);
    expect(
      await screen.findByRole("heading", {
        name: "Keys for web services",
        level: 2,
      }),
    ).toBeVisible();
    // WEB-U-262 (a first-hour check before 17.0): what the section is for, beside the connectors' keys below it.
    expect(
      screen.getByText(
        "For a service your tasks call over the web, such as GitHub or a package registry. A task uses the key by its name and never sees it.",
      ),
    ).toBeVisible();
  });

  it("lists each key by name, address and last use", async () => {
    const { api } = fakeHostKeys({
      keys: [
        hostKey({
          name: "GITHUB_WORK",
          last_use: {
            at: "2026-09-30T14:02:00Z",
            count: 12,
            conversation_id: "c1",
            task_title: "Sales report",
            user: null,
            outcome: "ok",
          },
        }),
        hostKey({
          name: "NOTION_KEY",
          host: "api.notion.com",
          path_prefix: null,
          last_use: {
            at: "2026-09-28T09:00:00Z",
            count: 1,
            conversation_id: "c2",
            task_title: "Q3 planning",
            user: null,
            outcome: "ok",
          },
        }),
        hostKey({
          name: "GITHUB_TOKEN",
          host: "api.github.com",
          path_prefix: null,
        }),
      ],
    });
    show(api);
    const [first, second, third] = await rows();
    expect(first).toHaveTextContent(
      /GITHUB_WORK.*github\.com\/acme.*Today, 14:02 · Sales report/,
    );
    expect(second).toHaveTextContent(
      /NOTION_KEY.*api\.notion\.com.*Sep 28 · Q3 planning/,
    );
    expect(third).toHaveTextContent(
      /GITHUB_TOKEN.*api\.github\.com.*Never used/,
    );
    // The whole of a use on hover and focus: its line as shown (cut where its column ends, 17.3), then the details.
    expect(
      within(first as HTMLElement).getByText(/Today, 14:02/),
    ).toHaveAttribute(
      "title",
      "Today, 14:02 · Sales report\nUsed 12 times in Sales report, last on Sep 30, 2026, 14:02",
    );
  });

  it("says a refused last try in words, never its code", async () => {
    const { api } = fakeHostKeys({
      keys: [
        hostKey({
          last_use: {
            at: "2026-09-30T14:02:00Z",
            count: 1,
            conversation_id: "c1",
            task_title: "Sales report",
            user: null,
            outcome: "foreign_run",
          },
        }),
      ],
    });
    show(api);
    const [row] = await rows();
    expect(row).toHaveTextContent(
      "Last try refused: the key was used by something left over from an earlier run.",
    );
    expect(row).not.toHaveTextContent("foreign_run");
  });

  // ADR-0037, Amendment 1: the key store couldn't serve the key, so nothing was sent.
  it("says a last try the key store couldn't serve", async () => {
    const { api } = fakeHostKeys({
      keys: [
        hostKey({
          last_use: {
            at: "2026-09-30T14:02:00Z",
            count: 1,
            conversation_id: "c1",
            task_title: "Sales report",
            user: null,
            outcome: "unavailable",
          },
        }),
      ],
    });
    show(api);
    const [row] = await rows();
    expect(row).toHaveTextContent(
      "Last try refused: the key store was unavailable.",
    );
  });

  it("warns quietly when tasks can't reach a key's host, and tells an administrator where to allow it", async () => {
    const keys = [
      hostKey({ host: "api.notion.com", path_prefix: null, allowed: false }),
    ];
    const member = show(fakeHostKeys({ keys }).api);
    expect(
      await screen.findByText(
        "Tasks can't reach api.notion.com yet. An administrator can allow it.",
      ),
    ).toBeVisible();
    member.unmount();
    show(fakeHostKeys({ keys }).api, true);
    expect(
      await screen.findByRole("link", { name: "Allow api.notion.com" }),
    ).toHaveAttribute("href", "/settings/internet-access?host=api.notion.com");
  });

  // ADR-0037 Amendment 2: only a rule for exactly that address lets tasks reach it, so the line says so.
  it("says an address key needs a rule for exactly that address", async () => {
    const member = show(
      fakeHostKeys({
        keys: [
          hostKey({ host: "10.0.0.5", path_prefix: null, allowed: false }),
        ],
      }).api,
    );
    expect(
      await screen.findByText(
        "Tasks can't reach 10.0.0.5 yet. An administrator can allow that exact address.",
      ),
    ).toBeVisible();
    member.unmount();
    show(
      fakeHostKeys({
        keys: [
          hostKey({
            host: "fd00::5",
            port: 8443,
            path_prefix: null,
            allowed: false,
          }),
        ],
      }).api,
      true,
    );
    expect(
      await screen.findByText(
        "Tasks can't reach fd00::5 yet: an address needs a rule for exactly that address.",
      ),
    ).toBeVisible();
    expect(screen.getByRole("link", { name: "Allow fd00::5" })).toHaveAttribute(
      "href",
      "/settings/internet-access?host=fd00%3A%3A5&port=8443",
    );
    expect(screen.getByRole("row", { name: /GITHUB_WORK/ })).toHaveTextContent(
      "[fd00::5]:8443",
    );
  });

  // Internet access opens Add a site ready from the link (WEB-U-150); a port other than 443 goes with it, so the rule
  // matches the key's.
  it("takes a key's port other than 443 to Internet access", async () => {
    const keys = [
      hostKey({
        host: "git.acme.lan",
        port: 8443,
        path_prefix: null,
        allowed: false,
      }),
    ];
    show(fakeHostKeys({ keys }).api, true);
    expect(
      await screen.findByRole("link", { name: "Allow git.acme.lan" }),
    ).toHaveAttribute(
      "href",
      "/settings/internet-access?host=git.acme.lan&port=8443",
    );
  });

  // Each key opens its own page (WEB-U-143), where a migrated key's name is checked and saved.
  it("opens a key's page from its name", async () => {
    const item = hostKey({ name: "GITHUB_WORK" });
    show(fakeHostKeys({ keys: [item] }).api);
    expect(
      await screen.findByRole("link", { name: "GITHUB_WORK" }),
    ).toHaveAttribute("href", `/settings/keys/${item.id}`);
  });

  it("flags a migrated key's name once, without blocking it", async () => {
    show(fakeHostKeys({ keys: [hostKey({ check_name: true })] }).api);
    const [row] = await rows();
    expect(
      within(row as HTMLElement).getByText("Check the name"),
    ).toBeVisible();
  });

  it("offers Add a key, and says what to do when there are none", async () => {
    show(fakeHostKeys().api);
    expect(
      await screen.findByText(
        "No keys for web services yet. Add one for a service your tasks call, such as GitHub.",
      ),
    ).toBeVisible();
    expect(
      screen.getByRole("link", { name: "Add a key for a web service" }),
    ).toHaveAttribute("href", "/settings/keys/new");
    // An empty state's one action is the primary (UI-GUIDELINES §7, 17.3), as the other sections' Add links are; it
    // was a secondary button before 17.3.
    expect(
      screen.getByRole("link", { name: "Add a key for a web service" }),
    ).toHaveAttribute("data-variant", "primary");
  });

  it("leaves itself out on a gateway without host keys", async () => {
    const { api } = fakeHostKeys({ missing: true });
    const { container } = show(api);
    await screen.findByText((_, element) => element === container);
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(container).toBeEmptyDOMElement();
  });

  it("says plainly when the keys can't be loaded, and tries again", async () => {
    const { api } = fakeHostKeys();
    api.keys = () => Promise.reject(new Error("offline"));
    show(api);
    expect(
      await screen.findByText(
        "Your keys for web services could not be loaded.",
      ),
    ).toBeVisible();
    expect(screen.getByRole("button", { name: "Try again" })).toBeVisible();
  });

  it("says once what the Add page just saved", async () => {
    show(fakeHostKeys({ keys: [hostKey()] }).api, false, "Saved GITHUB_WORK.");
    expect(await screen.findByRole("status")).toHaveTextContent(
      "Saved GITHUB_WORK.",
    );
  });

  it("never shows a value", async () => {
    const { api } = fakeHostKeys({ keys: [hostKey()] });
    const { container } = show(api);
    await rows();
    expect(container).not.toHaveTextContent(/tqa_ph_|secret|value/i);
  });
});

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { User } from "../protocol/generated/api";
import { cellNote } from "../test/cell-note";
import { showEnvironments } from "../test/environment-app";
import { environment, fakeTemplates, LISTED } from "../test/templates-admin";
import { LIMITS } from "../test/user";

import type { TemplatesApi } from "./admin/templates";
import { EnvironmentsSettings } from "./EnvironmentsSettings";

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

const admin: User = {
  id: "0195d8c0-0000-7000-8000-000000000001",
  email: "admin@example.invalid",
  role: "admin",
  internet: "none",
  limits: LIMITS,
  features: { agent_teams: false },
};
function show(api: TemplatesApi, me: User = admin) {
  const cache = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  render(
    <QueryClientProvider client={cache}>
      <EnvironmentsSettings me={me} api={api} />
    </QueryClientProvider>,
  );
}
const row = (name: RegExp) => screen.getByRole("row", { name });
afterEach(() => vi.useRealTimers());

// Settings, Environments (ADR-0032): what tasks can run in, whether each image passed its check, and why not (WEB-U-047).
describe("WEB-U-047 the environments list", () => {
  it("calls a base that names itself built in, as the gateway lists a seeded base (WEB-U-047)", async () => {
    show(
      fakeTemplates({
        environments: [environment("minimal", { base: "minimal" })],
      }).api,
    );
    await screen.findByRole("table", { name: "Environments" });
    expect(
      within(row(/^Minimal/)).getByText("Built in · Configuration file"),
    ).toBeVisible();
  });

  // UI-GUIDELINES §10 (WEB-U-047): no internal ids, tools named in words, the task count only where it matters.
  it("shows each environment's origin, tools and check state, by its display name", async () => {
    show(fakeTemplates({ environments: LISTED }).api);
    await screen.findByRole("table", { name: "Environments" });
    // Plain words for what an environment is: no digests (UI-GUIDELINES §10).
    expect(
      screen.getByText(
        "The machines tasks run in. Each is built from Workstation or Minimal; the gateway checks it before people can pick it. A change reaches a task at its next session.",
      ),
    ).toBeVisible();
    expect(
      screen.getAllByRole("columnheader").map((cell) => cell.textContent),
    ).toEqual(["Environment", "Tools", "Status", "Actions"]);
    const workstation = row(/^Workstation/);
    for (const text of ["Built in · Configuration file", "Default", "Verified"])
      expect(within(workstation).getByText(text)).toBeVisible();
    // Each tool by name, not an unlabelled icon.
    expect(
      within(within(workstation).getByRole("list", { name: "Tools" }))
        .getAllByRole("listitem")
        .map((item) => item.textContent),
    ).toEqual(["Shell", "Python", "Node.js", "Build tools", "Browser"]);
    // The internal name and the task count are not the row's: the count is Delete's reason when it matters.
    expect(within(workstation).queryByText("workstation")).toBeNull();
    expect(within(workstation).queryByText(/Used by/)).toBeNull();
    const data = row(/Data science/);
    expect(
      within(data).getByText("Based on Workstation · Added here"),
    ).toBeVisible();
    expect(within(data).getByText("Refused")).toBeVisible();
    // Under the pills, one muted line: the reason's sentence, then what it means for people (17.3; its page says each
    // on its own line).
    expect(
      cellNote(data, "People cannot pick it until it passes the check."),
    ).toHaveTextContent(
      "The image does not start from Workstation or Minimal. Build it FROM one of them. · People cannot pick it until it passes the check.",
    );
    expect(within(row(/GPU tools/)).getByText("Checking…")).toBeVisible();
    expect(within(row(/^Old/)).getByText("Off")).toBeVisible();
  });

  // One actions column (UI-GUIDELINES §7, as on Models and Keys): Edit, and the rest in a menu named after the row.
  it("gives each environment Edit and a menu: check, make default where people can pick it, and delete", async () => {
    show(fakeTemplates({ environments: LISTED }).api);
    await screen.findByRole("table", { name: "Environments" });
    for (const name of ["Workstation", "Minimal", "Data science"])
      // Edit opens the environment's page (WEB-U-097), so it is a link; the rest are in the menu.
      expect(
        [...row(new RegExp(`^${name}`)).querySelectorAll("a, button")].map(
          (control) =>
            `${control.tagName === "A" ? "link" : "button"} ${control.getAttribute("aria-label")}`,
        ),
      ).toEqual([
        `link Edit environment ${name}`,
        `button More actions for environment ${name}`,
      ]);
    const items = (name: string) => {
      fireEvent.click(
        screen.getByRole("button", {
          name: `More actions for environment ${name}`,
        }),
      );
      const names = screen
        .getAllByRole("menuitem")
        .map((item) => item.getAttribute("aria-label"));
      fireEvent.keyDown(document.activeElement ?? document.body, {
        key: "Escape",
      });
      return names;
    };
    // Make default only where people can pick it; the default can't be deleted, and says why (§5b).
    expect(items("Minimal")).toEqual([
      "Check the image of Minimal now",
      "Make Minimal the default",
      "Delete environment Minimal",
    ]);
    expect(items("Data science")).toEqual([
      "Check the image of Data science now",
      "Delete environment Data science",
    ]);
    fireEvent.click(
      screen.getByRole("button", {
        name: "More actions for environment Workstation",
      }),
    );
    const blocked = screen.getByRole("menuitem", {
      name: "Delete environment Workstation",
    });
    expect(blocked).toHaveAttribute("aria-disabled", "true");
    expect(blocked).toHaveAccessibleDescription(
      "Make another environment the default first.",
    );
  });

  it("says a delete waits for the tasks that use it, in the menu", async () => {
    show(
      fakeTemplates({
        environments: LISTED.map((item) =>
          item.name === "data" ? { ...item, bound_conversations: 3 } : item,
        ),
      }).api,
    );
    await screen.findByRole("table", { name: "Environments" });
    fireEvent.click(
      screen.getByRole("button", {
        name: "More actions for environment Data science",
      }),
    );
    const blocked = screen.getByRole("menuitem", {
      name: "Delete environment Data science",
    });
    expect(blocked).toHaveAttribute("aria-disabled", "true");
    expect(blocked).toHaveAccessibleDescription(
      "3 tasks use it; they can switch to another first.",
    );
  });

  it("refetches while a check is pending, and stops once it settles", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    const fake = fakeTemplates({ environments: LISTED });
    show(fake.api);
    await screen.findByText("Checking…");
    fake.answer("gpu", "rootfs_missing");
    await act(() => vi.advanceTimersByTimeAsync(5_000));
    expect(cellNote(row(/GPU tools/), "No VM disk image yet.")).toBeVisible();
  });

  it("shows members nothing", () => {
    show(fakeTemplates({ environments: LISTED }).api, {
      ...admin,
      role: "member",
    });
    expect(
      screen.getByText("Only administrators can manage environments."),
    ).toBeVisible();
  });
});

// ADR-0032: a default that people cannot pick leaves new tasks without it; the page says so first, and why.
describe("WEB-U-047 a default people cannot pick", () => {
  const withDefault = (extra: Parameters<typeof environment>[1]) =>
    LISTED.map((item) =>
      item.name === "workstation" ? { ...item, ...extra } : item,
    );
  const banner = () =>
    screen.getByRole("region", {
      name: "New tasks cannot start in the default environment",
    });

  it.each([
    [
      "refused by its check",
      {
        check: {
          state: "refused",
          reason: "runtime_too_old",
          checked_version: 1,
          secret_files: null,
          scan_limit: null,
        },
      },
      "The image’s agent runtime is too old. Rebuild it FROM a current Workstation or Minimal.",
    ],
    [
      "refused for a reason the page does not know",
      {
        check: {
          state: "refused",
          reason: "new_code",
          checked_version: 1,
          secret_files: null,
          scan_limit: null,
        },
      },
      "The image was refused for a reason this page does not know yet.",
    ],
    ["turned off (by the seed file)", { enabled: false }, "It is turned off."],
    [
      "changed here and not checked yet",
      {
        last_set_by: "admin",
        check: {
          state: "unchecked",
          reason: null,
          checked_version: null,
          secret_files: null,
          scan_limit: null,
        },
      },
      "It has not passed its check yet.",
    ],
  ] as const)(
    "says so above the list when the default is %s",
    async (_case, extra, reason) => {
      show(
        fakeTemplates({
          environments: withDefault(extra),
        }).api,
      );
      await screen.findByRole("table", { name: "Environments" });
      expect(banner()).toHaveTextContent(
        "Workstation is the default, but people cannot pick it now.",
      );
      expect(banner()).toHaveTextContent(reason);
      expect(within(banner()).queryByRole("alert")).toBeNull();
    },
  );

  it("moves focus to the default's row from the notice", async () => {
    show(fakeTemplates({ environments: withDefault({ enabled: false }) }).api);
    await screen.findByRole("table", { name: "Environments" });
    fireEvent.click(
      within(banner()).getByRole("link", { name: "Go to Workstation" }),
    );
    expect(row(/^Workstation/)).toHaveFocus();
  });

  it("says nothing while the default is offered, and goes once another becomes the default", async () => {
    show(fakeTemplates({ environments: LISTED }).api);
    await screen.findByRole("table", { name: "Environments" });
    expect(
      screen.queryByRole("region", {
        name: "New tasks cannot start in the default environment",
      }),
    ).toBeNull();
    const fake = fakeTemplates({
      environments: withDefault({ enabled: false }),
    });
    document.body.innerHTML = "";
    show(fake.api);
    await screen.findByRole("table", { name: "Environments" });
    expect(banner()).toBeVisible();
    fireEvent.click(
      screen.getByRole("button", {
        name: "More actions for environment Minimal",
      }),
    );
    fireEvent.click(
      screen.getByRole("menuitem", { name: "Make Minimal the default" }),
    );
    fireEvent.click(
      within(screen.getByRole("dialog")).getByRole("button", {
        name: "Make default",
      }),
    );
    expect(
      await screen.findByText("Minimal is now the default."),
    ).toBeVisible();
    expect(
      screen.queryByRole("region", {
        name: "New tasks cannot start in the default environment",
      }),
    ).toBeNull();
  });

  it("treats a seeded environment changed here as the gateway does: not pickable until checked", async () => {
    show(
      fakeTemplates({
        environments: [
          ...LISTED,
          environment("edited", {
            display_name: "Edited",
            last_set_by: "admin",
            seed: "current",
            check: {
              state: "unchecked",
              reason: null,
              checked_version: 2,
              secret_files: null,
              scan_limit: null,
            },
          }),
        ],
      }).api,
    );
    await screen.findByRole("table", { name: "Environments" });
    expect(
      cellNote(
        row(/^Edited/),
        "People cannot pick it until it passes the check.",
      ),
    ).toBeVisible();
    fireEvent.click(
      screen.getByRole("button", {
        name: "More actions for environment Edited",
      }),
    );
    expect(
      screen.queryByRole("menuitem", { name: "Make Edited the default" }),
    ).toBeNull();
  });
});

// A pass over the pages before 17.0: a built-in the gateway registered itself, found on this host, says so; nobody
// added it here.
describe("WEB-U-235 a built-in the gateway found on this host", () => {
  it("reads Found on this host, not Added here", async () => {
    show(
      fakeTemplates({
        environments: [
          environment("minimal", { display_name: "Minimal", is_default: true }),
          environment("workstation", {
            display_name: "Workstation",
            last_set_by: "builtin",
            seed: null,
          }),
        ],
      }).api,
    );
    const line = await screen.findByRole("row", { name: /Workstation/ });
    expect(
      within(line).getByText("Built in · Found on this host"),
    ).toBeVisible();
    expect(within(line).queryByText(/Added here/)).toBeNull();
  });
});

// E2E-VM-013: on a microVM install, a built-in whose disk image isn't built can't be used, so it doesn't become the
// default; its row says why in one line, naming the build script, and its page says it in plain words with the command
// that builds it (17.3).
describe("WEB-U-163 an environment whose microVM disk image isn't built", () => {
  it("says so in plain words, with the command that builds it", async () => {
    const workstation = environment("workstation", {
      display_name: "Workstation",
      last_set_by: "builtin",
      check: {
        state: "refused",
        reason: "rootfs_missing",
        checked_version: 1,
        secret_files: null,
        scan_limit: null,
      },
    });
    showEnvironments(
      fakeTemplates({
        environments: [
          environment("minimal", { display_name: "Minimal", is_default: true }),
          workstation,
        ],
      }).api,
      router,
    );
    const line = await screen.findByRole("row", { name: /Workstation/ });
    expect(
      cellNote(
        line,
        "No VM disk image yet. Build it with images/rootfs/build_rootfs.py, then check again.",
      ),
    ).toBeVisible();
    fireEvent.click(
      within(line).getByRole("link", { name: "Edit environment Workstation" }),
    );
    const status = await screen.findByRole("region", { name: "Status" });
    expect(
      within(status).getByText(
        "Workstation's microVM disk image isn't built yet, so tasks can't use it. On the gateway's host, build it with:",
      ),
    ).toBeVisible();
    expect(
      within(status).getByText(
        `python3 images/rootfs/build_rootfs.py --image ${workstation.image_id ?? ""} --vmm-image tqa-vmm --out <storage.root>/images`,
      ),
    ).toBeVisible();
    expect(within(status).getByText("Then check it again.")).toBeVisible();
  });
});

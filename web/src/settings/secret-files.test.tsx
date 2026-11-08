import { fireEvent, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { cellNote } from "../test/cell-note";
import { showEnvironments } from "../test/environment-app";
import { environment, fakeTemplates } from "../test/templates-admin";

import type { EnvironmentItem } from "./admin/templates";

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

/** The Environments list, from which each environment's page opens (its Status section says the check in full). */
function show(environments: EnvironmentItem[]) {
  showEnvironments(fakeTemplates({ environments }).api, router);
}
const row = (name: RegExp) => screen.getByRole("row", { name });
/** Opens an environment's page from its row, and returns the page's Status section. */
async function status(display: string) {
  fireEvent.click(
    screen.getByRole("link", { name: `Edit environment ${display}` }),
  );
  return screen.findByRole("region", { name: "Status" });
}
type Check = EnvironmentItem["check"];
const refused = (check: Partial<Check>): Check => ({
  state: "refused",
  reason: "image_secret_file",
  checked_version: 1,
  secret_files: null,
  scan_limit: null,
  ...check,
});

// The image policy's secret-file scan (ADR-0032 amendment; SBX-C-014, GW-U-063): a template image that holds credential
// files is refused, naming each path and what it looks like, never its content; paths the install file acknowledges
// verify with a warning; an image too large to scan says which setting to raise (WEB-U-060). The list row says it in
// one muted line, whole in its tooltip; the environment's page in full (17.3).
describe("WEB-U-060 credential files in an environment's image", () => {
  it("names each refused path as code with what it is, marks acknowledged ones, and counts the rest", async () => {
    show([
      environment("data", {
        display_name: "Data science",
        base: "workstation",
        check: refused({
          secret_files: {
            items: [
              {
                path: "/root/.git-credentials",
                rule: "git_credentials",
                acknowledged: false,
              },
              {
                path: "/srv/keys/deploy.pem",
                rule: "private_key",
                acknowledged: false,
              },
              {
                path: "/opt/test/fixture_key",
                rule: "private_key",
                acknowledged: true,
              },
            ],
            more: 3,
          },
          scan_limit: null,
        }),
      }),
    ]);
    await screen.findByRole("table", { name: "Environments" });
    expect(
      cellNote(row(/Data science/), "Credential files found"),
    ).toHaveTextContent(
      "Its image holds credential files that every task would get. Remove them, or acknowledge a path in the install file if it must stay. · Credential files found · /root/.git-credentials: saved logins for a code host · /srv/keys/deploy.pem: a private key · /opt/test/fixture_key: a private key (acknowledged) · and 3 more · People cannot pick it until it passes the check.",
    );
    const data = await status("Data science");
    expect(
      within(data).getByText(
        "Its image holds credential files that every task would get. Remove them, or acknowledge a path in the install file if it must stay.",
      ),
    ).toBeVisible();
    const list = within(data).getByRole("list", {
      name: "Credential files found",
    });
    const items = within(list).getAllByRole("listitem");
    expect(items.map((item) => item.textContent)).toEqual([
      "/root/.git-credentials: saved logins for a code host",
      "/srv/keys/deploy.pem: a private key",
      "/opt/test/fixture_key: a private key (acknowledged)",
    ]);
    expect(
      within(items[0] as HTMLElement).getByText("/root/.git-credentials")
        .tagName,
    ).toBe("CODE");
    expect(within(data).getByText("and 3 more")).toBeVisible();
  });

  it("warns about acknowledged files on a verified environment", async () => {
    show([
      environment("fixtures", {
        display_name: "Test fixtures",
        base: "workstation",
        check: {
          state: "verified",
          reason: null,
          checked_version: 1,
          secret_files: {
            items: [
              {
                path: "/opt/test/fixture_key",
                rule: "private_key",
                acknowledged: true,
              },
            ],
            more: 0,
          },
          scan_limit: null,
        },
      }),
    ]);
    await screen.findByRole("table", { name: "Environments" });
    const line = row(/Test fixtures/);
    expect(within(line).getByText("Verified")).toBeVisible();
    expect(cellNote(line, "Acknowledged")).toHaveTextContent(
      "Acknowledged credential files, which every task gets · /opt/test/fixture_key: a private key",
    );
    const fixtures = await status("Test fixtures");
    expect(within(fixtures).getByText("Verified")).toBeVisible();
    expect(
      within(fixtures).getByRole("list", {
        name: "Acknowledged credential files, which every task gets",
      }),
    ).toHaveTextContent("/opt/test/fixture_key: a private key");
    expect(within(fixtures).queryByText(/and \d+ more/)).toBeNull();
  });

  it("says which install-file setting to raise for an image too large to scan", async () => {
    show([
      environment("huge", {
        display_name: "Huge",
        base: "workstation",
        check: refused({
          reason: "image_scan_limit",
          secret_files: null,
          scan_limit: { bound: "entries", limit: 160000 },
        }),
      }),
      environment("heavy", {
        display_name: "Heavy",
        base: "workstation",
        check: refused({
          reason: "image_scan_limit",
          secret_files: null,
          scan_limit: { bound: "bytes", limit: 6 * 1024 ** 3 },
        }),
      }),
    ]);
    await screen.findByRole("table", { name: "Environments" });
    expect(row(/^Huge/)).toHaveTextContent(
      /more than 160[,\s  ]?000 files to scan for credential files\. An administrator can raise max_scan_entries under \[template_checks\] in the install file\./,
    );
    expect(row(/^Heavy/)).toHaveTextContent(
      "more than 6 GB of files to scan for credential files. An administrator can raise max_scan_bytes under [template_checks] in the install file.",
    );
  });

  it("shows a path exactly as sent, as plain text", async () => {
    show([
      environment("odd", {
        display_name: "Odd",
        base: "workstation",
        check: refused({
          secret_files: {
            items: [
              {
                path: "/home/<b>x</b>/�key…",
                rule: "netrc",
                acknowledged: false,
              },
            ],
            more: 0,
          },
          scan_limit: null,
        }),
      }),
    ]);
    await screen.findByRole("table", { name: "Environments" });
    const line = row(/^Odd/);
    expect(
      cellNote(line, "/home/<b>x</b>/�key…: netrc credentials"),
    ).toBeVisible();
    expect(line.querySelector("b")).toBeNull();
    const odd = await status("Odd");
    expect(within(odd).getByText("/home/<b>x</b>/�key…").tagName).toBe("CODE");
    expect(odd).toHaveTextContent("/home/<b>x</b>/�key…: netrc credentials");
    expect(odd.querySelector("b")).toBeNull();
  });
});

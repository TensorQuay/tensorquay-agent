import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import type { User } from "../protocol/generated/api";
import { environment, fakeTemplates } from "../test/templates-admin";
import { LIMITS } from "../test/user";

import type { EnvironmentItem } from "./admin/templates";
import { EnvironmentsSettings } from "./EnvironmentsSettings";

const admin: User = {
  id: "0195d8c0-0000-7000-8000-000000000001",
  email: "admin@example.invalid",
  role: "admin",
  internet: "none",
  limits: LIMITS,
  features: { agent_teams: false },
};
function show(environments: EnvironmentItem[]) {
  const cache = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  render(
    <QueryClientProvider client={cache}>
      <EnvironmentsSettings
        me={admin}
        api={fakeTemplates({ environments }).api}
      />
    </QueryClientProvider>,
  );
}
const row = (name: RegExp) => screen.getByRole("row", { name });
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
// verify with a warning; an image too large to scan says which setting to raise (WEB-U-060).
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
    const data = row(/Data science/);
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
    const fixtures = row(/Test fixtures/);
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
    const odd = row(/^Odd/);
    expect(within(odd).getByText("/home/<b>x</b>/�key…").tagName).toBe("CODE");
    expect(odd).toHaveTextContent("/home/<b>x</b>/�key…: netrc credentials");
    expect(odd.querySelector("b")).toBeNull();
  });
});

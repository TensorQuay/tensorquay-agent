import { describe, expect, it } from "vitest";

import { nameProblem, suggestName } from "./names";

// ADR-0037 §1: a name suggested from the host (a built-in list for well-known hosts, else its first label), unique
// across the person's own keys and the shared keys they may use.
describe("WEB-U-140 a key's name", () => {
  it.each([
    ["github.com", "GITHUB_TOKEN"],
    ["api.github.com", "GITHUB_TOKEN"],
    ["gitlab.com", "GITLAB_TOKEN"],
    ["huggingface.co", "HF_TOKEN"],
    ["registry.npmjs.org", "NPM_TOKEN"],
    ["pypi.org", "PYPI_TOKEN"],
    ["upload.pypi.org", "PYPI_TOKEN"],
    ["api.notion.com", "API_TOKEN"],
    ["jira.acme-corp.example", "JIRA_TOKEN"],
    ["10-0-0-5.lan", "HOST_10_0_0_5_TOKEN"],
    // An exact address (ADR-0037 Amendment 2) is named whole: its first part alone says nothing.
    ["10.0.0.5", "HOST_10_0_0_5_TOKEN"],
    ["fd00::5", "HOST_FD00_5_TOKEN"],
    ["2001:db8::34", "HOST_2001_DB8_34_TOKEN"],
    ["::ffff:192.0.2.10", "HOST_FFFF_192_0_2_10_TOKEN"],
  ])("suggests a name for %s", (host, name) => {
    expect(suggestName(host, [])).toBe(name);
  });

  // A long first label is cut so the whole name fits 64 characters.
  it("keeps a suggestion within 64 characters, numbered or not", () => {
    const label = "a".repeat(63);
    const plain = suggestName(`${label}.example`, []);
    expect(plain.length).toBeLessThanOrEqual(64);
    expect(plain).toMatch(/_TOKEN$/);
    expect(nameProblem(plain, { own: [], shared: [] })).toBeNull();
    const numbered = suggestName(`${label}.example`, [plain]);
    expect(numbered.length).toBeLessThanOrEqual(64);
    expect(nameProblem(numbered, { own: [], shared: [] })).toBeNull();
    const long = suggestName("2001:db8:1234:5678:9abc:def0:1234:5678", []);
    expect(long.length).toBeLessThanOrEqual(64);
    expect(nameProblem(long, { own: [], shared: [] })).toBeNull();
  });

  it("numbers a suggestion that is taken", () => {
    expect(suggestName("github.com", ["GITHUB_TOKEN"])).toBe("GITHUB_TOKEN_2");
    expect(suggestName("github.com", ["GITHUB_TOKEN", "GITHUB_TOKEN_2"])).toBe(
      "GITHUB_TOKEN_3",
    );
  });

  it.each([
    ["GITHUB_WORK", null],
    ["A", null],
    ["", "empty"],
    ["github_work", "invalid"],
    ["1PASSWORD", "invalid"],
    ["_TOKEN", "invalid"],
    ["MY-KEY", "invalid"],
    ["A".repeat(65), "invalid"],
  ])("checks %j", (name, problem) => {
    expect(nameProblem(name, { own: [], shared: [] })).toEqual(
      problem === null ? null : { kind: problem },
    );
  });

  // The gateway's reserved names (the gateway plan's §1.2.1): the variables a sandbox and its tools rely on.
  it.each([
    "PATH",
    "HOME",
    "HTTPS_PROXY",
    "SSL_CERT_FILE",
    "NODE_OPTIONS",
    "PYTHONPATH",
    "LC_ALL",
    "LD_PRELOAD",
    "GIT_ASKPASS",
    "TQA_RUNTIME_TOKEN",
  ])("refuses the reserved name %s", (name) => {
    expect(nameProblem(name, { own: [], shared: [] })).toEqual({
      kind: "reserved",
    });
  });

  it("never suggests a reserved name", () => {
    expect(suggestName("path.example", [])).toBe("PATH_TOKEN");
    expect(suggestName("git.acme.lan", [])).not.toMatch(/^GIT_/);
  });

  it("names the key that already has the name, own or shared", () => {
    const taken = {
      own: [{ name: "GITHUB_WORK", host: "github.com", path_prefix: "/acme" }],
      shared: [{ name: "PYPI_MIRROR", host: "pypi.acme.lan" }],
    };
    expect(nameProblem("GITHUB_WORK", taken)).toEqual({
      kind: "takenOwn",
      address: "github.com/acme",
    });
    expect(nameProblem("PYPI_MIRROR", taken)).toEqual({
      kind: "takenShared",
      address: "pypi.acme.lan",
    });
  });
});

import { describe, expect, it } from "vitest";

import { formatAddress, parseAddress } from "./address";

// ADR-0037 §1: a key is for one exact host and port (443 unless given) and optionally a path. People paste addresses
// as they find them; what the design can't honour is refused in words.
describe("WEB-U-139 a key's web address", () => {
  it.each([
    [
      "a bare host",
      "api.github.com",
      { host: "api.github.com", port: 443, path_prefix: null },
    ],
    [
      "a pasted https link",
      "https://github.com/acme/",
      { host: "github.com", port: 443, path_prefix: "/acme" },
    ],
    [
      "a path",
      "github.com/acme/tools.git",
      { host: "github.com", port: 443, path_prefix: "/acme/tools.git" },
    ],
    [
      "a port",
      "git.acme.lan:8443/team",
      { host: "git.acme.lan", port: 8443, path_prefix: "/team" },
    ],
    [
      "capitals and spaces",
      "  HTTPS://API.GitHub.com  ",
      { host: "api.github.com", port: 443, path_prefix: null },
    ],
    [
      "port 443 said aloud",
      "https://api.github.com:443",
      { host: "api.github.com", port: 443, path_prefix: null },
    ],
    [
      "an exact IPv4 address (ADR-0037 Amendment 2)",
      "https://10.0.0.5/git",
      { host: "10.0.0.5", port: 443, path_prefix: "/git" },
    ],
    [
      "an IPv6 address in brackets, with a port",
      "https://[fd00::5]:8443/git",
      { host: "fd00::5", port: 8443, path_prefix: "/git" },
    ],
    [
      "an IPv6 address in its one spelling, as the gateway stores it",
      "[2001:DB8:0:0::34]",
      { host: "2001:db8::34", port: 443, path_prefix: null },
    ],
    [
      "an IPv4-mapped IPv6 address, its IPv4 part dotted",
      "[::FFFF:192.0.2.10]",
      { host: "::ffff:192.0.2.10", port: 443, path_prefix: null },
    ],
  ])("reads %s", (_, text, address) => {
    expect(parseAddress(text)).toEqual({ ok: true, address });
  });

  it.each([
    ["nothing", "", "empty"],
    ["plain http", "http://api.github.com", "notHttps"],
    ["another scheme", "ssh://git@github.com/acme", "notHttps"],
    ["a user name", "https://me@github.com", "extras"],
    ["a user and password", "https://me:secret@github.com", "extras"],
    ["a query", "https://api.example.com/v1?key=1", "extras"],
    ["a fragment", "https://example.com/#top", "extras"],
    ["a wildcard", "*.github.com", "wildcard"],
    ["a space inside", "api .github.com", "invalid"],
    ["no host", "https:///acme", "invalid"],
    ["a port out of range", "example.com:70000", "invalid"],
    ["an IPv6 address without brackets", "fd00::5", "brackets"],
    [
      "an IPv6 address without brackets, as a link",
      "https://fd00::5/git",
      "brackets",
    ],
    ["something else in brackets", "[fd00::zz]", "invalid"],
    ["an IPv4 address in brackets", "[10.0.0.5]", "invalid"],
    ["brackets never closed", "[fd00::5", "invalid"],
  ])("refuses %s", (_, text, problem) => {
    expect(parseAddress(text)).toEqual({ ok: false, problem });
  });

  it("shows an address as people read it: the port only when it isn't 443", () => {
    expect(
      formatAddress({ host: "github.com", port: 443, path_prefix: "/acme" }),
    ).toBe("github.com/acme");
    expect(
      formatAddress({ host: "git.acme.lan", port: 8443, path_prefix: null }),
    ).toBe("git.acme.lan:8443");
    expect(
      formatAddress({ host: "fd00::5", port: 8443, path_prefix: "/git" }),
    ).toBe("[fd00::5]:8443/git");
    expect(
      formatAddress({ host: "2001:db8::34", port: 443, path_prefix: null }),
    ).toBe("[2001:db8::34]");
  });
});

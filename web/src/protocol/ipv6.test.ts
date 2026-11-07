// @vitest-environment node
import { isIPv6 } from "node:net";

import { describe, expect, it } from "vitest";

import { validIpv6 } from "./ipv6";

describe("WEB-U-003 protocol destination format", () => {
  it.each([
    "::",
    "::1",
    "2001:db8::1",
    "2001:DB8:0:1:2:3:4:5",
    "::ffff:192.0.2.1",
    "2001:db8:0:1:2:3:192.0.2.1",
    "",
    "192.0.2.1",
    "2001:db8::1::2",
    "2001:db8:1:2:3:4:5",
    "2001:db8:1:2:3:4:5:6:7",
    "2001:db8:10000::1",
    "::ffff:192.0.2.01",
    "2001:db8::g",
    ":".repeat(254),
  ])("matches the independent IP parser for %s", (value) => {
    expect(validIpv6(value)).toBe(isIPv6(value));
  });

  it.each([
    "[2001:db8::1]",
    "2001:db8::1%zone",
    "2001:db8::1]/path",
    "2001:db8::1]?query",
    "2001:db8::1]@example.invalid",
    " 2001:db8::1",
    "example.invalid:443",
  ])("rejects URL syntax and scoped literals: %s", (value) => {
    expect(validIpv6(value)).toBe(false);
  });
});

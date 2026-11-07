import { describe, expect, it } from "vitest";

import { ApiError } from "../../api/error";
import { english } from "../../i18n/translate";

import { draftBody, refusalWords } from "./host-key-form";

const refused = (field: string, reason: string) =>
  refusalWords(
    new ApiError(400, "invalid_request", "Bad.", undefined, [
      { field, reason },
    ]),
    english,
    "GITHUB_WORK",
  );

// The gateway's closed InvalidReason list, `<field>_<problem>` (the gateway's contract §1.2): each reason is said in
// words at its field, never as its code; the way of sending, which has no field of its own, on the page's line.
describe("WEB-U-142 every field reason the gateway gives, in words at its field", () => {
  it.each([
    [
      "address",
      "address_not_https",
      "address",
      "Keys are sent only over https.",
    ],
    [
      "address",
      "address_not_plain",
      "address",
      "Use the service's address only, without a name, password, or anything after ? or #.",
    ],
    [
      "address",
      "host_invalid",
      "address",
      "That isn't a web address, such as github.com/acme or 10.0.0.5.",
    ],
    [
      "name",
      "name_invalid",
      "name",
      "Use capital letters, digits and _, starting with a letter.",
    ],
    [
      "name",
      "name_reserved",
      "name",
      "That name is kept for the task's own use. Pick another name.",
    ],
    [
      "name",
      "name_taken",
      "name",
      "This name is already another key's. Pick another name.",
    ],
    [
      "name",
      "name_taken_by_user",
      "name",
      "Someone who may use this key already has their own key named GITHUB_WORK. Choose another name.",
    ],
    [
      "user_name",
      "user_name_required",
      "userName",
      "Enter the user name the service expects with this key.",
    ],
    [
      "header_name",
      "header_name_required",
      "headerName",
      "Enter the header the service reads the key from, such as x-api-key.",
    ],
    ["value", "value_empty", "value", "Paste the key."],
    [
      "value",
      "value_too_long",
      "value",
      "Paste the key on one line, up to 8 kB.",
    ],
    ["value", "value_nul", "value", "Paste the key on one line, up to 8 kB."],
    [
      "value",
      "value_invalid",
      "value",
      "Paste the key on one line, up to 8 kB.",
    ],
  ])("%s %s", (field, reason, at, words) => {
    expect(refused(field, reason)).toEqual({
      problems: { [at]: words },
      failure: null,
    });
  });

  it("says a refused way of sending on the page's line", () => {
    expect(refused("scheme", "scheme_not_supported")).toEqual({
      problems: {},
      failure: "This way of sending a key isn't available yet.",
    });
  });

  // Only a stale administrator's page could change a member's own key: an administrator may delete it, never change
  // it (ADR-0037 §1).
  it("says a member's own key can't be changed, on the page's line", () => {
    expect(
      refusalWords(new ApiError(409, "personal_key", "Personal."), english),
    ).toEqual({
      problems: {},
      failure: "This is a member's own key: you can delete it, not change it.",
    });
  });

  it("never shows a reason it doesn't know", () => {
    expect(refused("value", "value_mystery")).toEqual({
      problems: {},
      failure: "The key could not be saved. Check the fields and try again.",
    });
  });
});

// WEB-U-192 (TEST-PLAN §19, found by WEB-E2E-023 on the real stack): the gateway reads a key's address as a URL and
// refuses anything that isn't https://, so a bare address, which the page itself accepts ("github.com/acme"), and every
// save from a key's own page (which shows its address without the scheme) were refused as "not https". The body carries
// the https URL the page has read.
describe("WEB-U-192 a key's address is sent as the https URL the page read", () => {
  const draft = (address: string) => ({
    name: "GITHUB_WORK",
    address,
    scheme: "bearer" as const,
    userName: "",
    headerName: "",
    value: "tok-1",
    users: "all" as const,
  });
  it.each([
    ["github.com/acme", "https://github.com/acme"],
    ["  HTTPS://API.GitHub.com/  ", "https://api.github.com"],
    ["git.acme.lan:8443/team", "https://git.acme.lan:8443/team"],
    ["api.github.com:443", "https://api.github.com"],
    ["10.0.0.5", "https://10.0.0.5"],
    ["[FD00:0::5]:8443/git", "https://[fd00::5]:8443/git"],
  ])("sends %s as %s", (typed, sent) => {
    expect(draftBody(draft(typed)).address).toBe(sent);
  });
});

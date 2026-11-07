import { describe, expect, it } from "vitest";

import { ApiError } from "../api/error";
import { connector } from "../test/connectors";

import {
  connectorFailure,
  connectorProblems,
  fromDraft,
  toDraft,
  toolsSummary,
} from "./connector-copy";

const full = connector("tracker", {
  display_name: "Issue tracker",
  url: "https://mcp.tracker.example/v1/mcp?team=a",
  auth: { scheme: "header", header: "X-Api-Key", required: true },
  secret_ref: "secrets/tracker",
  tools: { deny: ["delete_issue", "issues.purge"] },
  call_timeout_seconds: 90,
  enabled: true,
  enabled_by_default: true,
});
const refusal = (
  status: number,
  code: string,
  field?: string,
  reason?: string,
) =>
  new ApiError(
    status,
    code,
    "x",
    undefined,
    field && reason ? [{ field, reason }] : [],
  );

// The Connectors form turns a connector into editable text and back, and checks what the gateway would refuse (WEB-U-043).
describe("WEB-U-043 connector form rules", () => {
  it("round-trips every field an administrator sets", () => {
    const { fields, problems } = fromDraft(toDraft(full), true);
    expect(problems).toEqual({});
    expect(fields).toEqual({
      display_name: "Issue tracker",
      url: "https://mcp.tracker.example/v1/mcp?team=a",
      auth: { scheme: "header", header: "X-Api-Key", required: true },
      secret_ref: "secrets/tracker",
      tools: { deny: ["delete_issue", "issues.purge"] },
      call_timeout_seconds: 90,
      enabled: true,
      enabled_by_default: true,
    });
    const plain = fromDraft(toDraft(null), true);
    expect(plain.fields.auth).toEqual({ scheme: "none", required: false });
    expect(plain.fields).not.toHaveProperty("secret_ref");
    expect(plain.fields).not.toHaveProperty("tools");
  });

  it("refuses at the field what the gateway would refuse", () => {
    const check = (change: Partial<ReturnType<typeof toDraft>>) =>
      fromDraft({ ...toDraft(full), ...change }, true).problems;
    for (const name of [
      "Tracker",
      "track__er",
      "tracker_",
      "1tracker",
      "a".repeat(17),
    ])
      expect(check({ name })).toEqual({ name: "connectors.nameInvalid" });
    for (const url of [
      "http://mcp.example/mcp",
      "https://u:p@mcp.example/mcp",
      "https://mcp.example/#x",
      "mcp.example",
    ])
      expect(check({ url })).toEqual({ url: "connectors.urlInvalid" });
    for (const header of ["Cookie", "X-", "X-Bad Name", `X-${"a".repeat(63)}`])
      expect(check({ header })).toEqual({ header: "connectors.headerInvalid" });
    for (const tools of [
      "",
      "a\na",
      Array.from({ length: 257 }, (_, i) => `t${i}`).join("\n"),
      "x".repeat(129),
    ])
      expect(check({ toolMode: "allow", tools })).toEqual({
        tools: "connectors.toolsInvalid",
      });
    for (const timeout of ["0", "601", "1.5", ""])
      expect(check({ timeout })).toEqual({
        timeout: "connectors.timeoutInvalid",
      });
    expect(check({ displayName: "Two\nlines" })).toEqual({
      displayName: "connectors.displayNameInvalid",
    });
    expect(
      fromDraft({ ...toDraft(full), name: "Bad" }, false).problems,
    ).toEqual({});
  });

  it("sends no credential settings when none is sent", () => {
    const { fields } = fromDraft({ ...toDraft(full), scheme: "none" }, true);
    expect(fields.auth).toEqual({ scheme: "none", required: false });
    expect(fields).not.toHaveProperty("secret_ref");
  });

  it("names the gateway's refusals at their fields, and the rest in plain words", () => {
    expect(
      connectorProblems(
        refusal(400, "invalid_request", "url", "auth_query_invalid"),
      ),
    ).toEqual({ url: "connectors.urlCredential" });
    expect(
      connectorProblems(
        refusal(400, "invalid_request", "auth", "auth_header_invalid"),
      ),
    ).toEqual({ header: "connectors.headerInvalid" });
    expect(
      connectorProblems(
        refusal(400, "invalid_request", "tools", "renamed_later"),
      ),
    ).toEqual({ tools: "connectors.fieldInvalid" });
    expect(
      connectorFailure(
        refusal(400, "invalid_request", "url", "base_url_invalid"),
      ),
    ).toBeNull();
    expect(connectorFailure(refusal(409, "conflict"))).toBe(
      "connectors.conflict",
    );
    expect(connectorFailure(refusal(409, "name_taken"))).toBe(
      "connectors.nameTaken",
    );
    expect(connectorFailure(refusal(409, "connector_limit"))).toBe(
      "connectors.full",
    );
    expect(connectorFailure(new Error("offline"))).toBe(
      "connectors.saveFailed",
    );
  });

  it("summarises the tool filter in words", () => {
    expect(toolsSummary(undefined)).toEqual(["connectors.toolsAll", {}]);
    expect(toolsSummary({ allow: ["a", "b", "c"] })).toEqual([
      "connectors.toolsAllow",
      { count: 3 },
    ]);
    expect(toolsSummary({ deny: ["a", "b"] })).toEqual([
      "connectors.toolsDeny",
      { count: 2 },
    ]);
  });
});

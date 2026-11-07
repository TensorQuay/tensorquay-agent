import { describe, expect, it } from "vitest";

import { connectorName, connectorRefusal } from "./connector-call";
import { toolSummary } from "./tool-summary";
import type { ToolView } from "./types";

/**
 * The codes the runtime's MCP client puts before ": " on a failed connector call that the page explains
 * (ADR-0031). mcp_invalid_params is left out on purpose: see the test below.
 */
const CODES = [
  "connector_not_allowed",
  "connector_credential_missing",
  "connector_credential_rejected",
  "connector_busy",
  "rate_limited",
  "connector_unavailable",
  "connector_redirect",
  "connector_answer_too_large",
  "connector_timeout",
  "store_unavailable",
  "unauthorized",
  "mcp_method_not_found",
  "mcp_error",
  "timeout",
  "mcp_server_exited",
  // GW-I-147: the gateway uses no key it can't record the use of.
  "key_use_unrecorded",
  // The MCP proxy's own refusals (X-TQA-Gateway-Error, 2849c1c4).
  "not_found",
  "body_too_large",
  "secret_unavailable",
];
const call = (
  status: ToolView["status"],
  result: string | null = null,
): ToolView => ({
  id: "call-1",
  name: "mcp.tracker.issues.create",
  arguments: { title: "Crash" },
  status,
  output: "",
  result,
  ran: status !== "prepared",
  startedAt: null,
  durationMs: null,
  exitCode: null,
});

// A connector's tool call reads as its tool on its connector, and a failure as a sentence (WEB-U-016, ADR-0031;
// WEB-U-045's approval card is superseded by ADR-0035).
describe("WEB-U-016 connector calls", () => {
  it("splits a connector tool's name at its first two dots only", () => {
    expect(connectorName("mcp.tracker.issues.create")).toEqual({
      connector: "tracker",
      tool: "issues.create",
    });
    expect(connectorName("mcp.wiki.search")).toEqual({
      connector: "wiki",
      tool: "search",
    });
    for (const name of [
      "shell",
      "mcp.tracker",
      "mcp..search",
      "mcp.tracker.",
      "mcpx.tracker.search",
    ])
      expect(connectorName(name)).toBeNull();
  });

  it("says in words what the call is doing, did, or would have done", () => {
    expect(toolSummary(call("running"))).toBe(
      "Using issues.create on tracker…",
    );
    expect(toolSummary(call("ok"))).toBe("Used issues.create on tracker");
    expect(toolSummary(call("prepared"))).toBe("issues.create on tracker");
    expect(
      toolSummary(call("error", "connector_timeout: tracker did not answer.")),
    ).toBe("issues.create on tracker");
  });

  it("names every refusal code the runtime sends, and treats anything else as the tool's own answer", () => {
    for (const code of CODES)
      expect(connectorRefusal(`${code}: sentence`), code).not.toBeNull();
    expect(connectorRefusal("connector_credential_missing: Add one.")).toEqual({
      key: "activity.connectorCredential",
      link: true,
    });
    // WEB-U-044: a person's credential the connector refused links to the same page, to check or replace it.
    expect(
      connectorRefusal(
        "connector_credential_rejected: the gateway's MCP proxy answered 424",
      ),
    ).toEqual({ key: "activity.connectorCredentialRejected", link: true });
    expect(connectorRefusal("secret_unavailable: x")).toEqual({
      key: "activity.connectorKeyMissing",
      link: false,
    });
    expect(connectorRefusal("body_too_large: x")).toEqual({
      key: "activity.connectorTooLarge",
      link: false,
    });
    expect(connectorRefusal("key_use_unrecorded: x")).toEqual({
      key: "error.key_use_unrecorded",
      link: false,
    });
    expect(connectorRefusal("rate_limited: retry after 12 s")).toEqual({
      key: "activity.connectorBusy",
      link: false,
    });
    // -32602 is also a server's answer to bad arguments (usually the model sent wrong ones), so it reads as the tool's
    // own answer with the runtime's message kept, not as "does not offer this tool".
    expect(
      connectorRefusal(
        "mcp_invalid_params: Invalid arguments for issues.create",
      ),
    ).toBeNull();
    // ADR-0035: the connector approval's codes are gone; one in an old history reads as the tool's own answer.
    for (const text of [
      "approval_required: x",
      "approval_missing: x",
      "call_not_journaled: x",
      "tool_denied: x",
      "renamed_later: x",
      "Issue 42 not found.",
      "Timeout: x",
      "mcp_error:x",
      "",
      null,
    ])
      expect(connectorRefusal(text)).toBeNull();
  });
});

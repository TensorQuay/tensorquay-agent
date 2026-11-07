import { describe, expect, it } from "vitest";

import { ApiError } from "../api/error";
import { profile } from "../test/profiles";

import {
  duplicateDraft,
  fromDraft,
  profileFailure,
  profileProblems,
  toDraft,
} from "./profile-copy";
import { TOOL_GROUPS, TOOLS } from "./profiles";

// Settings → Profiles (ADR-0012, WEB-U-049): the form's draft, the page's own checks and the gateway's reasons.
describe("WEB-U-049 profile drafts", () => {
  it("round-trips a profile: unset fields stay unset, so the task or the install decides", () => {
    const draft = toDraft(
      profile("review", {
        model_route: "local",
        tools: ["read_file", "grep"],
        skills: ["lint", "docs"],
        connectors: ["tracker"],
        limits: { max_steps: 40 },
      }),
    );
    const { fields, problems } = fromDraft(draft, false);
    expect(problems).toEqual({});
    expect(fields).toMatchObject({
      model_route: "local",
      template: null,
      tools: ["read_file", "grep"],
      skills: ["lint", "docs"],
      connectors: ["tracker"],
      mcp_servers: [],
      limits: { max_steps: 40 },
    });
    expect(fields).not.toHaveProperty("shared");
    // PRD FR-10, superseded: a profile has no approval policy and no rules; the network allow-list is the guardrail.
    expect(fields).not.toHaveProperty("policy");
    expect(fields).not.toHaveProperty("rules");
  });

  it("sends every tool as no allow-list, and a new shared profile as shared", () => {
    const draft = { ...toDraft(null), name: "team", displayName: "Team" };
    expect(draft.tools).toEqual([...TOOLS]);
    expect(fromDraft({ ...draft, shared: true }, true).fields).toMatchObject({
      tools: null,
      skills: "all",
      shared: true,
    });
  });

  it("duplicates a shared profile as a personal one without its name", () => {
    const copy = duplicateDraft(
      profile("shared-review", { shared: true, display_name: "Review" }),
    );
    expect(copy).toMatchObject({
      name: "",
      displayName: "Review",
      shared: false,
    });
  });

  it("checks what it can before sending", () => {
    const draft = {
      ...toDraft(null),
      name: "Bad Name",
      displayName: "",
      skills: "named" as const,
      skillNames: "Lint, lint",
      tools: [],
      connectors: ["a", "b", "c", "d", "e", "f", "g", "h", "i"],
      instructions: "x".repeat(16 * 1024 + 1),
      limits: { ...toDraft(null).limits, max_steps: "0" },
    };
    expect(Object.keys(fromDraft(draft, true).problems).sort()).toEqual([
      "connectors",
      "displayName",
      "instructions",
      "limits",
      "name",
      "skills",
      "tools",
    ]);
  });

  it("maps the gateway's field reasons to the form, and a refusal without them to a sentence", () => {
    const refused = new ApiError(422, "unknown_reference", "x", undefined, [
      { field: "model_route", reason: "unknown_reference" },
    ]);
    expect(profileProblems(refused)).toEqual({
      route: "profiles.unknownReference",
    });
    expect(profileFailure(refused)).toBeNull();
    expect(profileFailure(new ApiError(409, "conflict", "x"))).toBe(
      "profiles.conflict",
    );
    expect(profileFailure(new ApiError(403, "forbidden", "x"))).toBe(
      "profiles.forbidden",
    );
  });
});

// WEB-U-105: a profile's tools are always listed and ticked by default; the saved shape stays as before, every tool
// being no allow-list at all.
describe("WEB-U-105 a profile's tools", () => {
  it("groups every built-in tool once, in the owner's order", () => {
    expect(TOOL_GROUPS.map(([group]) => group)).toEqual([
      "files",
      "finding",
      "commands",
      "notes",
      "web",
      "skills",
      "channels",
    ]);
    const grouped = TOOL_GROUPS.flatMap(([, tools]) => tools);
    expect([...grouped].sort()).toEqual([...TOOLS].sort());
    expect(new Set(grouped).size).toBe(TOOLS.length);
  });

  it("reads no allow-list as every tool ticked, and a list as those", () => {
    expect(toDraft(profile("all", { tools: null })).tools).toEqual([...TOOLS]);
    expect(
      toDraft(profile("some", { tools: ["grep", "read_file"] })).tools,
    ).toEqual(["grep", "read_file"]);
  });

  it("saves every tool as no allow-list, some in the list's order, and refuses none", () => {
    const draft = { ...toDraft(null), name: "t", displayName: "T" };
    expect(fromDraft(draft, true).fields.tools).toBeNull();
    expect(
      fromDraft({ ...draft, tools: ["grep", "read_file"] }, true).fields.tools,
    ).toEqual(["read_file", "grep"]);
    expect(fromDraft({ ...draft, tools: [] }, true).problems).toEqual({
      tools: "profiles.toolsEmpty",
    });
  });
});

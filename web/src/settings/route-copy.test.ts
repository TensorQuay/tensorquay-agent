import { describe, expect, it } from "vitest";

import { NEW_ROUTE, type RouteFields } from "./admin/routes";
import { fromDraft, gatewayProblems, toDraft } from "./route-copy";
import { routeSlug } from "./RouteBasics";

const full: RouteFields = {
  ...NEW_ROUTE,
  display_name: "Qwen Coder",
  base_url: "http://vllm.example.invalid:8000/v1",
  model: "qwen2.5-coder",
  auth: { kind: "header", name: "X-Api-Key" },
  key_ref: "secrets/qwen",
  context_window: 32768,
  max_output: 4096,
  concurrency: 4,
  requests_per_minute: 120,
  reasoning: {
    field: "reasoning_effort",
    allowed: ["low", "medium", "high"],
    default: "medium",
    budget_tokens: { low: 1024, high: 8192 },
  },
  extra_headers: { "X-Team": "agents", "X-Region": "sg" },
  extra_body: { temperature: 0.2, top_p: 0.9 },
  quirks: ["no_parallel_tools", "strip_reasoning"],
  fallbacks: ["local"],
};

// The route form's text round trip and the problems it names before sending (WEB-U-039).
describe("WEB-U-039 route form text", () => {
  it("turns every field into editable text and back without loss", () => {
    const draft = toDraft("qwen", full);
    expect(draft.budgets).toBe("low: 1024\nhigh: 8192");
    expect(draft.headers).toBe("X-Team: agents\nX-Region: sg");
    expect(draft.reasoningAllowed).toBe("low, medium, high");
    expect(draft.quirks).toBe("no_parallel_tools, strip_reasoning");
    expect(fromDraft(draft)).toEqual({ fields: full, problems: {} });
  });

  it("keeps a route's instructions role only when the route sets it", () => {
    const developer: RouteFields = { ...full, instructions_role: "developer" };
    expect(fromDraft(toDraft("qwen", developer)).fields.instructions_role).toBe(
      "developer",
    );
    // Absent stays absent: its default depends on the protocol, so the form never fills one in.
    expect("instructions_role" in fromDraft(toDraft("qwen", full)).fields).toBe(
      false,
    );
  });

  it("names each problem at its field and keeps the rest", () => {
    const draft = {
      ...toDraft("qwen", full),
      context_window: "0",
      max_output: "1.5",
      concurrency: "-2",
      requests_per_minute: "",
      budgets: "low 1024",
      headers: "X-Team agents",
      body: "[1, 2]",
    };
    const { fields, problems } = fromDraft(draft);
    expect(problems).toEqual({
      context_window: "models.wholeNumber",
      max_output: "models.wholeNumber",
      concurrency: "models.wholeNumber",
      budget_tokens: "models.budgetInvalid",
      extra_headers: "models.headerLine",
      extra_body: "models.jsonObject",
    });
    expect(fields.requests_per_minute).toBeNull();
    expect(fields.extra_body).toEqual({});
    for (const budgets of ["low: 0", "low: 1.5", "low: many"])
      expect(fromDraft({ ...draft, budgets }).problems.budget_tokens).toBe(
        "models.budgetInvalid",
      );
    expect(
      fromDraft({ ...toDraft("qwen", full), body: "not json" }).problems,
    ).toEqual({
      extra_body: "models.jsonObject",
    });
  });

  it("sends no key without a credential and trims a query parameter's name", () => {
    const none = fromDraft({
      ...toDraft("qwen", full),
      authKind: "none",
    }).fields;
    expect(none.auth).toEqual({ kind: "none" });
    expect(none.key_ref).toBeNull();
    const query = fromDraft({
      ...toDraft("qwen", full),
      authKind: "query",
      authName: "  api_key ",
    }).fields;
    expect(query.auth).toEqual({ kind: "query", name: "api_key" });
    const noReasoning = fromDraft({
      ...toDraft("qwen", full),
      reasoning: false,
    }).fields;
    expect(noReasoning.reasoning).toBeNull();
    expect(
      gatewayProblems([
        { field: "base_url", reason: "base_url_invalid" },
        { field: "model", reason: "a_reason_from_a_newer_gateway" },
      ]),
    ).toEqual({
      base_url: "models.baseUrlInvalid",
      model: "models.fieldInvalid",
    });
  });
});

// GW-U-098: `new` is the Add route page's address, so no route takes it; the page checks before the gateway does, and
// never makes it from a display name.
describe("GW-U-098 the reserved route name, on the page", () => {
  it("refuses the name new when adding, and words the gateway's refusal", () => {
    const draft = {
      ...toDraft("new", { ...NEW_ROUTE, display_name: "New", model: "m" }),
    };
    expect(fromDraft(draft).problems).toMatchObject({
      name: "models.nameReserved",
    });
    expect(
      fromDraft({ ...draft, name: "new-model" }).problems,
    ).not.toHaveProperty("name");
    expect(
      gatewayProblems([{ field: "name", reason: "name_reserved" }]),
    ).toEqual({ name: "models.nameReserved" });
  });

  it("never makes new from a display name", () => {
    expect(routeSlug("New")).toBe("new-route");
    expect(routeSlug("New model")).toBe("new-model");
  });
});

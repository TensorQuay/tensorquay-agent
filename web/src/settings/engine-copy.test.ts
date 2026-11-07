import { describe, expect, it } from "vitest";

import { ApiError } from "../api/error";
import { engine } from "../test/decision-engines";

import {
  engineFailure,
  engineHost,
  engineLabel,
  engineProblems,
  fieldsOf,
  fromEngineDraft,
  identifierFor,
  secretName,
  toEngineDraft,
  widens,
} from "./engine-copy";

const refused = (status: number, code: string, field = "", reason = "") =>
  new ApiError(
    status,
    code,
    "fixture",
    undefined,
    field ? [{ field, reason }] : [],
  );

// TEST: WEB-U-074
describe("WEB-U-074 the engine form's rules", () => {
  it("widens only when the engine would see more, a new one when it sees more than names", () => {
    expect(widens(undefined, "none")).toBe(false);
    expect(widens(undefined, "calls")).toBe(true);
    expect(widens("calls", "full")).toBe(true);
    expect(widens("full", "calls")).toBe(false);
    expect(widens("calls", "calls")).toBe(false);
  });

  it("makes an identifier people never have to type", () => {
    expect(identifierFor("Policy engine")).toBe("policy-engine");
    expect(identifierFor("Crème Brûlée's Router!")).toBe(
      "creme-brulee-s-router",
    );
    expect(identifierFor("2nd opinion")).toBe("engine-2nd-opinion");
    expect(identifierFor("   ")).toBe("engine");
    expect(identifierFor("x".repeat(90))).toHaveLength(64);
  });

  it("sends the draft's fields: the key with how it is sent, none only when chosen, seconds as milliseconds", () => {
    const draft = {
      ...toEngineDraft(null),
      displayName: "Policy engine",
      url: "https://policy.example",
      key: "secrets/key",
      timeout: "0.5",
    };
    expect(fromEngineDraft(draft, true)).toEqual({
      fields: {
        display_name: "Policy engine",
        url: "https://policy.example",
        auth: { scheme: "bearer" },
        key_ref: "secrets/key",
        timeout_ms: 500,
        exposure: "none",
        enabled: true,
        protocol: "decision_v1",
      },
      problems: {},
    });
    expect(
      fromEngineDraft(
        { ...draft, scheme: "header", header: "X-Engine-Key" },
        false,
      ).fields.auth,
    ).toEqual({ scheme: "header", header: "X-Engine-Key" });
    const none = fromEngineDraft({ ...draft, key: "none" }, false).fields;
    expect([none.auth, none.key_ref]).toEqual([{ scheme: "none" }, undefined]);
    expect(
      fromEngineDraft(
        {
          ...draft,
          displayName: "",
          url: "http://user:pw@policy.example",
          key: "",
          timeout: "11",
        },
        true,
      ).problems,
    ).toEqual({
      displayName: "engines.displayNameRequired",
      url: "engines.urlInvalid",
      key: "engines.keyRequired",
      timeout: "engines.timeoutInvalid",
    });
    expect(
      fromEngineDraft(
        {
          ...draft,
          name: "Bad Name",
          nameEdited: true,
          scheme: "header",
          header: "Cookie",
        },
        true,
      ).problems,
    ).toEqual({ name: "engines.nameInvalid", header: "engines.headerInvalid" });
  });

  it("maps the gateway's refusals to the form's fields and the page's messages", () => {
    expect(
      engineProblems(
        refused(400, "invalid_request", "key_ref", "auth_invalid"),
      ),
    ).toEqual({ key: "engines.authInvalid" });
    expect(
      engineProblems(
        refused(400, "invalid_request", "auth", "auth_header_invalid"),
      ),
    ).toEqual({ header: "engines.headerInvalid" });
    expect(
      engineProblems(refused(400, "invalid_request", "auth", "auth_invalid")),
    ).toEqual({ scheme: "engines.authInvalid" });
    expect(
      engineProblems(
        refused(400, "invalid_request", "display_name", "display_name_invalid"),
      ),
    ).toEqual({ displayName: "engines.displayNameInvalid" });
    expect(
      engineProblems(refused(400, "invalid_request", "url", "something_new")),
    ).toEqual({ url: "engines.fieldInvalid" });
    expect(engineProblems(new Error("offline"))).toEqual({});
    expect(engineFailure(new Error("offline"))).toBe("engines.saveFailed");
    expect(engineFailure(refused(400, "invalid_request"))).toBeNull();
    expect(engineFailure(refused(409, "conflict"))).toBe("engines.conflict");
    expect(engineFailure(refused(409, "decision_engine_limit"))).toBe(
      "engines.full",
    );
    expect(engineFailure(refused(409, "decision_engine_in_use"))).toBe(
      "engines.inUse",
    );
    expect(engineFailure(refused(500, "internal"))).toBe("engines.saveFailed");
  });

  it("reads an engine back into the draft and its fields, and shows it by name", () => {
    const stored = engine("router", {
      display_name: "Policy engine",
      exposure: "full",
      key_ref: "secrets/key",
      auth: { scheme: "header", header: "X-Key", required: false },
      timeout_ms: 500,
      enabled: false,
    });
    expect(toEngineDraft(stored)).toEqual({
      displayName: "Policy engine",
      name: "router",
      nameEdited: true,
      url: "https://router.example",
      exposure: "full",
      key: "secrets/key",
      scheme: "header",
      header: "X-Key",
      timeout: "0.5",
      enabled: false,
      kind: "decision_v1",
      model: "",
      minConfidence: "",
      hints: {},
      budgetCalls: "",
      budgetTokens: "",
      budgetPeriod: "total",
    });
    expect(
      toEngineDraft(
        engine("open", { auth: { scheme: "none", required: false } }),
      ).key,
    ).toBe("none");
    expect(fieldsOf(stored)).toEqual({
      display_name: "Policy engine",
      url: "https://router.example",
      auth: { scheme: "header", header: "X-Key" },
      key_ref: "secrets/key",
      timeout_ms: 500,
      exposure: "full",
      enabled: false,
      protocol: "decision_v1",
    });
    // A typed-question service keeps its own fields through the draft and through a change from the list.
    const typed = engine("router", {
      protocol: "typed_questions_v1",
      model: "pinned-model",
      min_confidence: 0.75,
      route_hints: { cheap: "routine steps", local: "hard steps" },
      budget: { calls: 3000, tokens: 30000000, period: "month" },
    });
    expect(toEngineDraft(typed)).toMatchObject({
      kind: "typed_questions_v1",
      model: "pinned-model",
      minConfidence: "0.75",
      hints: { cheap: "routine steps", local: "hard steps" },
      budgetCalls: "3000",
      budgetTokens: "30000000",
      budgetPeriod: "month",
    });
    const back = fromEngineDraft(toEngineDraft(typed), false);
    expect(back.problems).toEqual({});
    for (const fields of [back.fields, fieldsOf(typed)])
      expect(fields).toMatchObject({
        protocol: "typed_questions_v1",
        model: "pinned-model",
        min_confidence: 0.75,
        route_hints: { cheap: "routine steps", local: "hard steps" },
        budget: { calls: 3000, tokens: 30000000, period: "month" },
      });
    expect(engineLabel(stored)).toBe("Policy engine");
    expect(engineLabel(engine("plain"))).toBe("plain");
    expect(engineHost("https://router.example/v1")).toBe("router.example");
    expect(engineHost("not a url")).toBe("not a url");
    expect(secretName("secrets/key")).toBe("key");
  });
});

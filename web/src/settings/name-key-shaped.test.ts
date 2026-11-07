import { describe, expect, it } from "vitest";

import { ApiError } from "../api/error";
import { connector } from "../test/connectors";
import { engine } from "../test/decision-engines";
import { madeUpKey } from "../test/made-up-keys";

import { connectorProblems, fromDraft, toDraft } from "./connector-copy";
import { engineProblems, fromEngineDraft, toEngineDraft } from "./engine-copy";
import { gatewayProblems } from "./route-copy";
import { keyReference } from "./RouteBasics";
import { reasonCopy, secretProblems } from "./secret-copy";

// WEB-U-303 (SEC-033): a key pasted where a shared key's name belongs is refused before anything is sent, and the
// gateway's own refusal, 400 name_key_shaped at the field, is put in each form's words.
const shaped = madeUpKey();
const refused = (field: string) =>
  new ApiError(400, "invalid_request", "fixture", undefined, [
    { field, reason: "name_key_shaped" },
  ]);

describe("WEB-U-303 a key-shaped name in each form", () => {
  it("refuses a new shared key named like a key, never a replace (which sends no name)", () => {
    expect(secretProblems({ name: shaped, value: "v" }, true)).toEqual([
      { field: "name", reason: "name_key_shaped" },
    ]);
    expect(secretProblems({ name: shaped, value: "v" }, false)).toEqual([]);
    expect(secretProblems({ name: "openai", value: "v" }, true)).toEqual([]);
    expect(reasonCopy("name_key_shaped")).toBe("secrets.nameKeyShaped");
  });

  it("refuses a route's key named like a key, even when it is listed", () => {
    const listed = [`secrets/${shaped}`, "secrets/deepseek"];
    expect(keyReference(shaped, listed)).toBeNull();
    expect(keyReference(`secrets/${shaped}`, listed)).toBeNull();
    expect(keyReference("deepseek", listed)).toBe("secrets/deepseek");
    expect(
      gatewayProblems([{ field: "key_ref", reason: "name_key_shaped" }]),
    ).toEqual({ key_ref: "models.keyRefNotReference" });
  });

  it("refuses a connector's shared key named like a key", () => {
    const draft = {
      ...toDraft(
        connector("tracker", {
          auth: { scheme: "bearer", required: true },
          secret_ref: "secrets/tracker",
        }),
      ),
      secretRef: `secrets/${shaped}`,
    };
    expect(fromDraft(draft, false).problems).toMatchObject({
      secretRef: "connectors.secretKeyShaped",
    });
    expect(
      fromDraft({ ...draft, secretRef: "secrets/tracker" }, false).problems,
    ).not.toHaveProperty("secretRef");
    expect(connectorProblems(refused("secret_ref"))).toEqual({
      secretRef: "connectors.secretKeyShaped",
    });
  });

  it("refuses an engine's key named like a key", () => {
    const draft = {
      ...toEngineDraft(engine("triage")),
      key: `secrets/${shaped}`,
    };
    expect(fromEngineDraft(draft, false).problems).toMatchObject({
      key: "engines.secretKeyShaped",
    });
    expect(engineProblems(refused("key_ref"))).toEqual({
      key: "engines.secretKeyShaped",
    });
  });
});

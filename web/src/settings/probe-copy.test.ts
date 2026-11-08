import { describe, expect, it } from "vitest";

import { ApiError } from "../api/error";
import adminSchema from "../protocol/generated/admin.schema.json";

import {
  FAILURES,
  failureKey,
  probeRefusal,
  STEPS,
  WARNINGS,
} from "./probe-copy";

const observation = adminSchema.definitions.RouteProbeObservation.properties;
const values = (node: { enum?: string[] }) => node.enum ?? [];

// Every value a route check can report reads as a sentence, and a newer one still reads plainly (WEB-U-039).
describe("WEB-U-039 route check copy", () => {
  it("has a sentence for every failure, step and warning the contract defines", () => {
    const failure = observation.failure.oneOf.find(
      (option) => "properties" in option,
    );
    expect(Object.keys(FAILURES).sort()).toEqual(
      values(
        failure && "properties" in failure ? failure.properties.code : {},
      ).sort(),
    );
    expect(Object.keys(STEPS).sort()).toEqual(
      values(observation.tool_call).sort(),
    );
    expect(Object.keys(WARNINGS).sort()).toEqual(
      values(observation.warnings.items).sort(),
    );
    expect(new Set(Object.values(FAILURES)).size).toBe(
      Object.keys(FAILURES).length,
    );
    expect(failureKey("renamed_later")).toBe("models.failUnknown");
    expect(failureKey("unreachable")).toBe("models.failUnreachable");
  });

  it("says why a check could not run, with the wait when the gateway gives one", () => {
    const refusal = (status: number, code: string, retryAfter?: number) =>
      new ApiError(status, code, "x", retryAfter);
    expect(probeRefusal(refusal(409, "key_missing"))).toEqual([
      "models.checkKeyMissing",
      {},
    ]);
    expect(probeRefusal(refusal(409, "probe_running"))).toEqual([
      "models.checkRunning",
      {},
    ]);
    expect(probeRefusal(refusal(429, "rate_limited", 17))).toEqual([
      "models.checkTooSoon",
      { seconds: 17 },
    ]);
    expect(probeRefusal(refusal(429, "rate_limited"))).toEqual([
      "models.checkTooSoonNoTime",
      {},
    ]);
    expect(probeRefusal(refusal(504, "timeout"))).toEqual([
      "models.checkTimeout",
      {},
    ]);
    expect(probeRefusal(refusal(404, "not_found"))).toEqual([
      "models.checkGone",
      {},
    ]);
    expect(probeRefusal(new Error("offline"))).toEqual([
      "models.checkError",
      {},
    ]);
  });
});

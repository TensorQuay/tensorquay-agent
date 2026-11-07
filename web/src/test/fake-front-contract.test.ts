// @vitest-environment node
import { describe, expect, it } from "vitest";

import { contractProblem } from "../../e2e/front/contract.mjs";

const id = "0195d8c0-0000-7000-8000-000000000001";
const messages = `/api/v1/conversations/${id}/messages`;

// GW-I-129: the fake front once answered a message without message_id and held, and no test saw it. Its responses are
// checked against openapi.json, and this pins the checker itself.
describe("the fake front's contract check", () => {
  it("refuses a body the documented schema doesn't allow", () => {
    expect(
      contractProblem("POST", messages, 202, {
        accepted: true,
        conversation_id: id,
      }),
    ).toMatch(/message_id/);
  });

  it("accepts the documented body", () => {
    expect(
      contractProblem("POST", messages, 202, {
        accepted: true,
        conversation_id: id,
        message_id: "1",
        held: false,
      }),
    ).toBeNull();
  });

  it("refuses a status the operation doesn't document", () => {
    expect(contractProblem("POST", messages, 418, {})).toMatch(
      /doesn't document/,
    );
  });

  it("leaves a route the contract doesn't describe alone", () => {
    expect(contractProblem("GET", "/fake/hook", 200, {})).toBeNull();
  });
});

// @vitest-environment node
import { describe, expect, it } from "vitest";

import { failure } from "./error";

const refusal = (error: Record<string, unknown>) => ({
  error: { message: "Refused.", retryable: false, ...error },
});

// A refusal keeps what the gateway says it concerns, so the page can explain it (ADR-0028; WEB-U-047).
describe("WEB-U-047 refusal details", () => {
  it("keeps what still uses a template on 409 template_in_use", () => {
    const error = failure(
      409,
      new Headers(),
      refusal({
        code: "template_in_use",
        in_use: { default: false, conversations: 3, mcp_servers: ["files"] },
      }),
    );
    expect(error.code).toBe("template_in_use");
    expect(error.inUse).toEqual({
      default: false,
      conversations: 3,
      mcp_servers: ["files"],
    });
  });

  it("has no use details on other refusals, and keeps field problems and the wait", () => {
    const error = failure(
      400,
      new Headers({ "retry-after": "7" }),
      refusal({
        code: "invalid_request",
        details: [{ field: "image", reason: "image_not_pinned" }],
      }),
    );
    expect(error.inUse).toBeUndefined();
    expect(error.details).toEqual([
      { field: "image", reason: "image_not_pinned" },
    ]);
    expect(error.retryAfter).toBe(7);
  });
});

// An agent team's runtime routes answer `details` as an object (ADR-0034; the openapi response check): the contract
// takes each shape, and a refusal carrying one keeps its code and its words with no field problems.
describe("WEB-U-233 refusal details in an agent team's shapes", () => {
  it.each([
    ["file_too_large", 413, { path: "reports/q3.csv" }],
    ["member_limit", 409, { limit: 3, by: "slots" }],
    ["team_ended", 409, { messages: [], last_seq: 11, more: false }],
  ])("reads %s with its object details", (code, status, details) => {
    const error = failure(status, new Headers(), refusal({ code, details }));
    expect(error.code).toBe(code);
    expect(error.message).toBe("Refused.");
    expect(error.details).toEqual([]);
  });
});

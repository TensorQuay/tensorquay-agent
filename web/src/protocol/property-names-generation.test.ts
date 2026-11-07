// @vitest-environment node
import Ajv from "ajv";
import { describe, expect, it } from "vitest";

import { runtimeSchema } from "../../scripts/runtime-schema.mjs";
import { generate } from "../../scripts/schema-types.mjs";

describe("WEB-U-008 generated property-name constraints", () => {
  it("keeps a string-keyed map type and enforces the key rule at the runtime boundary", async () => {
    // The shape the protocol exports for an MCP server's environment.
    const value = {
      type: "object",
      maxProperties: 32,
      propertyNames: {
        pattern: "^[A-Z_][A-Z0-9_]{0,63}$",
        not: { pattern: "^TQA_" },
      },
      additionalProperties: { type: "string", maxLength: 4096 },
    };
    const generated = await generate({ definitions: { Env: value } });
    expect(generated).toContain("export type Env = { [key: string]: string };");
    const validate = new Ajv({ strict: true }).compile(
      runtimeSchema(value) as typeof value,
    );
    expect(validate({ LOG_LEVEL: "info" })).toBe(true);
    for (const invalid of [{ lower: "x" }, { TQA_TOKEN: "x" }, { A: 1 }])
      expect(validate(invalid)).toBe(false);
  });

  it("still refuses an unsupported keyword inside the key rule", async () => {
    await expect(
      generate({
        definitions: {
          Unsupported: {
            type: "object",
            propertyNames: { if: { const: "x" } },
          },
        },
      }),
    ).rejects.toThrow("Unsupported schema keyword: if");
  });
});

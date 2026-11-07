// @vitest-environment node
import Ajv from "ajv";
import { describe, expect, it } from "vitest";

import { runtimeSchema } from "../../scripts/runtime-schema.mjs";
import { generate } from "../../scripts/schema-types.mjs";

describe("WEB-U-008 generated tuples", () => {
  it("types a fixed-length array item by item and enforces its length at the runtime boundary", async () => {
    // The shape the protocol exports for a manifest entry's change stamp.
    const value = {
      type: "array",
      items: [
        { type: "integer" },
        { type: "integer" },
        { type: "integer" },
        { type: "integer" },
        { type: "integer", minimum: 0 },
      ],
      minItems: 5,
      maxItems: 5,
    };
    const generated = await generate({ definitions: { Stamp: value } });
    expect(generated).toContain(
      "export type Stamp = [number, number, number, number, number];",
    );
    const validate = new Ajv({ strict: true }).compile(
      runtimeSchema(value) as typeof value,
    );
    expect(validate([1, 2, 3, 4, 5])).toBe(true);
    for (const invalid of [
      [1, 2, 3, 4],
      [1, 2, 3, 4, 5, 6],
      [1, 2, 3, 4, -5],
    ])
      expect(validate(invalid)).toBe(false);
  });

  it("keeps a single item schema as an array of that type", async () => {
    const generated = await generate({
      definitions: { Names: { type: "array", items: { type: "string" } } },
    });
    expect(generated).toContain("export type Names = Array<string>;");
  });
});

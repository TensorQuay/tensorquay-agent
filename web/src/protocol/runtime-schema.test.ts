// @vitest-environment node
import { describe, expect, it } from "vitest";

import { runtimeSchema } from "../../scripts/runtime-schema.mjs";

describe("WEB-U-008 runtime schema annotations", () => {
  it("omits documentation without changing validation constraints or mutating the source", () => {
    const source = {
      title: "Message",
      description: "Documentation only",
      $comment: "A note",
      type: "object",
      required: ["text"],
      additionalProperties: false,
      properties: {
        text: { type: "string", maxLength: 12, description: "The text" },
      },
      anyOf: [{ properties: { text: { minLength: 2, title: "Nonempty" } } }],
    };
    const before = structuredClone(source);
    expect(runtimeSchema(source)).toEqual({
      type: "object",
      required: ["text"],
      additionalProperties: false,
      properties: { text: { type: "string", maxLength: 12 } },
      anyOf: [{ properties: { text: { minLength: 2 } } }],
    });
    expect(source).toEqual(before);
  });

  it("retains wire fields with annotation names and arbitrary constant, enum, default and example data", () => {
    const data = { title: "A title", description: "A value", $comment: "Data" };
    const source = {
      type: "object",
      properties: {
        title: { type: "string" },
        description: { type: "string" },
        $comment: { type: "string" },
      },
      const: data,
      enum: [data],
      default: data,
      examples: [data],
    };
    expect(runtimeSchema(source)).toEqual(source);
  });
});

// @vitest-environment node
import Ajv from "ajv";
import ts from "typescript";
import { describe, expect, it } from "vitest";

import { runtimeSchema } from "../../scripts/runtime-schema.mjs";
import { generate } from "../../scripts/schema-types.mjs";

describe("WEB-U-008 generated negation constraints", () => {
  it("retains the structural type and enforces negation at the runtime boundary", async () => {
    const value = {
      type: "object",
      required: ["host"],
      properties: { host: { type: "string", not: { pattern: "[^A-Za-z.]" } } },
    };
    const generated = await generate({ definitions: { Destination: value } });
    const source = ts.createSourceFile(
      "generated.ts",
      generated,
      ts.ScriptTarget.ES2022,
      true,
    );
    const aliases = source.statements.filter(ts.isTypeAliasDeclaration);
    expect(aliases).toHaveLength(1);
    const shape = aliases[0]?.type;
    if (!shape || !ts.isTypeLiteralNode(shape))
      throw new Error("Expected a structural type");
    const property = shape.members[0];
    if (!property || !ts.isPropertySignature(property))
      throw new Error("Expected a property");
    expect(property.name.getText(source)).toBe("host");
    expect(property.type?.kind).toBe(ts.SyntaxKind.StringKeyword);
    expect(property.questionToken).toBeUndefined();
    const validate = new Ajv({ strict: true }).compile(
      runtimeSchema(value) as typeof value,
    );
    expect(validate({ host: "example.invalid" })).toBe(true);
    for (const invalid of [{}, { host: 42 }, { host: "example.invalid/path" }])
      expect(validate(invalid)).toBe(false);
  });

  it("handles boolean negation without losing the sibling type", async () => {
    const generated = await generate({
      definitions: {
        Impossible: { type: "string", not: true },
        Unchanged: { type: "string", not: false },
      },
    });
    expect(generated).toContain("export type Impossible = never;");
    expect(generated).toContain("export type Unchanged = string;");
  });

  it("still refuses an unsupported keyword nested inside a negation", async () => {
    await expect(
      generate({
        definitions: {
          Unsupported: { type: "string", not: { if: { const: "blocked" } } },
        },
      }),
    ).rejects.toThrow("Unsupported schema keyword: if");
  });
});

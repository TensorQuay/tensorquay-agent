// @vitest-environment node
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { describe, expect, it } from "vitest";

import { protocol } from "../../scripts/protocol.mjs";

describe("WEB-U-008 generated protocol", () => {
  it("rejects stale types when only a schema constraint changes", async () => {
    const directory = mkdtempSync(join(tmpdir(), "protocol-check-"));
    try {
      const schema = join(directory, "schema.json");
      const output = join(directory, "protocol.ts");
      writeFileSync(
        schema,
        JSON.stringify({
          definitions: { Value: { type: "string", maxLength: 8 } },
        }),
      );
      // In-process: starting Node for each run timed out under load (the generator is the same code the CLI runs).
      const run = (...args: string[]) =>
        protocol(["--schema", schema, "--output", output, ...args]);
      expect((await run()).status).toBe(0);
      expect(readFileSync(output, "utf8")).toContain(
        "export type Value = string;",
      );
      expect((await run("--check")).status).toBe(0);
      writeFileSync(
        schema,
        JSON.stringify({
          definitions: { Value: { type: "string", maxLength: 9 } },
        }),
      );
      expect((await run("--check")).status).toBe(1);
    } finally {
      rmSync(directory, { recursive: true, force: true });
    }
  });

  it("creates discriminated unions and rejects unsupported schema shapes", async () => {
    const directory = mkdtempSync(join(tmpdir(), "protocol-union-"));
    try {
      const schema = join(directory, "schema.json");
      const output = join(directory, "protocol.ts");
      writeFileSync(
        schema,
        JSON.stringify({
          definitions: {
            Frame: {
              oneOf: [
                {
                  type: "object",
                  required: ["type"],
                  properties: { type: { const: "first" } },
                },
                {
                  type: "object",
                  required: ["type"],
                  properties: { type: { const: "second" } },
                },
              ],
            },
          },
        }),
      );
      const run = () => protocol(["--schema", schema, "--output", output]);
      expect((await run()).status).toBe(0);
      const source = readFileSync(output, "utf8");
      expect(source).toContain('type: "first"');
      expect(source).toContain('type: "second"');
      writeFileSync(
        schema,
        JSON.stringify({
          definitions: { Unsupported: { if: { type: "string" } } },
        }),
      );
      expect((await run()).status).toBe(1);
    } finally {
      rmSync(directory, { recursive: true, force: true });
    }
  });
});

// @vitest-environment node
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";

import ts from "typescript";
import { expect, it } from "vitest";

import { generate } from "../../scripts/schema-types.mjs";

it("WEB-U-008 preserves Environment fields alongside its schema alternatives", async () => {
  const directory = mkdtempSync(join(tmpdir(), "environment-types-"));
  try {
    const schema: unknown = JSON.parse(
      readFileSync(
        resolve("../crates/tqa-types/schema/tqa-protocol-v1.json"),
        "utf8",
      ),
    );
    writeFileSync(join(directory, "protocol.ts"), await generate(schema));
    const probe = join(directory, "probe.ts");
    writeFileSync(
      probe,
      `
      import type { Environment } from "./protocol";
      const valid: Environment = { template: "workstation", digest: "sha256:" + "a".repeat(64), capabilities: ["shell", "python", "node", "build", "browser", "data", "desktop"] };
      const template: string = valid.template;
      const digest: string = valid.digest;
      const capabilities: Array<"shell" | "python" | "node" | "build" | "browser" | "data" | "desktop"> = valid.capabilities;
      void [template, digest, capabilities];
      // @ts-expect-error required fields remain required despite anyOf
      const missing: Environment = {};
      // @ts-expect-error reserved capabilities cannot become unknown
      const reserved: Environment = { template: "workstation", digest: "", capabilities: ["gpu"] };
      // @ts-expect-error the template remains a string
      const malformed: Environment = { template: 4, digest: "", capabilities: [] };
      void [missing, reserved, malformed];
    `,
    );
    const program = ts.createProgram([probe], {
      strict: true,
      noEmit: true,
      skipLibCheck: true,
      types: [],
      target: ts.ScriptTarget.ES2022,
      module: ts.ModuleKind.ESNext,
      moduleResolution: ts.ModuleResolutionKind.Bundler,
    });
    expect(
      ts
        .getPreEmitDiagnostics(program)
        .map((diagnostic) =>
          ts.flattenDiagnosticMessageText(diagnostic.messageText, "\n"),
        ),
    ).toEqual([]);
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});

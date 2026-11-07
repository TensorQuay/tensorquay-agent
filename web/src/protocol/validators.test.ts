// @vitest-environment node
import { readFileSync } from "node:fs";

import FullAjv from "ajv";
import { describe, expect, it } from "vitest";

import { longTranscriptFrames } from "../../e2e/front/long-transcript.mjs";
import {
  browserSchemas,
  compact,
  FORMAT_SHAPES,
  sharedErrors,
} from "../../scripts/validators.mjs";
import { transcriptFrames } from "../test/frames";
import { messagesFrames } from "../test/messages";
import { referenceAjv } from "../test/reference-ajv";
import { corpus } from "../test/schema-corpus";

import { formats } from "./formats";
import adminSchema from "./generated/admin.schema.json";
import apiSchema from "./generated/api.schema.json";
import protocolSchema from "./generated/protocol.schema.json";
import * as access from "./generated/validators.access";
import * as admin from "./generated/validators.admin";
import * as channels from "./generated/validators.channels";
import * as core from "./generated/validators.core";
import * as previews from "./generated/validators.previews";
import * as profiles from "./generated/validators.profiles";
import * as settings from "./generated/validators.settings";
import * as shares from "./generated/validators.shares";
import * as teams from "./generated/validators.teams";

type Validate = (value: unknown) => boolean;
const MODULES = {
  core,
  access,
  channels,
  settings,
  admin,
  profiles,
  previews,
  shares,
  teams,
} as const;
/** Each generated validator with the reference it answers for. */
const generated: [string, Validate][] = [
  ["urn:tqa:protocol#/definitions/ServerFrame", core.ServerFrame],
  ...Object.entries(MODULES).flatMap(([module, exports]) =>
    Object.entries(exports as Record<string, unknown>)
      .filter(
        (entry): entry is [string, Validate] => typeof entry[1] === "function",
      )
      .filter(([name]) => name !== "ServerFrame")
      .map(([name, validate]): [string, Validate] => [
        `urn:tqa:${module === "admin" ? "admin" : "api"}#/definitions/${name}`,
        validate,
      ]),
  ),
];
/** Values every definition is probed with: none of the kinds a schema might accept or refuse is left out. */
const PROBES: unknown[] = [
  null,
  true,
  0,
  -1,
  1.5,
  "",
  "x",
  "0195d8c0-0000-7000-8000-000000000001",
  "2026-09-27T00:00:00Z",
  [],
  [1, "x"],
  {},
  { type: "unknown" },
];
/** Each frame broken in the ways the stream's validation must catch. */
function broken(frame: object): object[] {
  const entries = Object.entries(frame);
  return [
    ...entries
      .filter(([key]) => key !== "type")
      .map(([key]) =>
        Object.fromEntries(entries.filter(([other]) => other !== key)),
      ),
    ...entries
      .filter(([key]) => key !== "type")
      .map(([key]) => ({ ...frame, [key]: { unexpected: [true] } })),
  ];
}

// WEB-U-014: the page's validators are precompiled (scripts/validators.mjs) instead of compiled by ajv in the browser;
// this proves each accepts and refuses exactly what full ajv does with the same schemas and formats.
describe("WEB-U-008 the precompiled validators", () => {
  it("keeps the generated schemas valid draft-07", () => {
    const meta = new FullAjv({ strict: true, allowUnionTypes: true });
    for (const schema of [protocolSchema, apiSchema, adminSchema])
      expect(meta.validateSchema(schema), JSON.stringify(meta.errors)).toBe(
        true,
      );
  });

  it("agrees with full ajv on each probe, for every validator of every module", () => {
    const reference = referenceAjv();
    let compared = 0;
    for (const [ref, mine] of generated) {
      const theirs = reference.getSchema(ref);
      expect(theirs, ref).toBeDefined();
      for (const probe of PROBES) {
        expect(mine(probe), `${ref} on ${JSON.stringify(probe)}`).toBe(
          theirs?.(probe),
        );
        compared += 1;
      }
    }
    expect(generated.length).toBeGreaterThan(40);
    expect(compared).toBeGreaterThan(500);
  });

  it("accepts every transcript frame and refuses each broken one, as full ajv does", () => {
    const theirs = referenceAjv().getSchema(
      "urn:tqa:protocol#/definitions/ServerFrame",
    );
    let refused = 0;
    for (const frame of transcriptFrames) {
      expect(core.ServerFrame(frame), JSON.stringify(frame)).toBe(true);
      for (const variant of broken(frame)) {
        const verdict = core.ServerFrame(variant);
        expect(verdict, JSON.stringify(variant)).toBe(theirs?.(variant));
        if (!verdict) refused += 1;
      }
    }
    // Positive control: the broken frames are refused, so agreement is not two validators accepting everything.
    expect(refused).toBeGreaterThan(transcriptFrames.length);
  });

  it("calls the formats formats.ts defines, in the shapes the generator declared", () => {
    expect(Object.keys(formats).sort()).toEqual(
      Object.keys(FORMAT_SHAPES).sort(),
    );
    for (const [name, shape] of Object.entries(FORMAT_SHAPES))
      expect(typeof formats[name], name).toBe(
        shape === "number" ? "object" : "function",
      );
  });

  // A Content-Security-Policy without 'unsafe-eval' holds: nothing the page ships compiles code at run time.
  it("ships no code that evaluates strings", () => {
    for (const name of Object.keys(MODULES)) {
      const source = readFileSync(
        new URL(`./generated/validators.${name}.js`, import.meta.url),
        "utf8",
      );
      expect(source, name).not.toMatch(/\bnew Function\b|\beval\(/);
    }
  });
});

// The conversation route's budget (WEB-U-175): the precompiled validators are compacted before generation. A oneOf of
// distinct string constants is generated as the enum it means, and a oneOf of objects told apart by a distinct `type`
// constant is discriminated by it, so the code checks one branch instead of counting matches across all of them. The
// reference (full ajv) still compiles the schemas as written, so these tests compare the old meaning with the new code.
describe("WEB-U-175 the compacted validators accept exactly what the schemas do", () => {
  it("rewrites only unions it can prove equivalent", () => {
    const schema = {
      definitions: {
        Words: {
          oneOf: [
            { const: "a", type: "string", description: "A." },
            { const: "b", type: "string" },
          ],
        },
        Frames: {
          oneOf: [
            {
              type: "object",
              properties: { type: { const: "x", type: "string" } },
              required: ["type"],
            },
            {
              type: "object",
              properties: { type: { const: "y", type: "string" } },
              required: ["type"],
            },
          ],
        },
        Repeated: { oneOf: [{ const: "a" }, { const: "a" }] },
        Mixed: { oneOf: [{ const: "a" }, { type: "integer" }] },
        Untagged: {
          oneOf: [
            { type: "object", properties: { type: { const: "x" } } },
            { type: "object", properties: { type: { const: "y" } } },
          ],
        },
      },
    };
    const { definitions } = compact(schema) as typeof schema & {
      definitions: Record<string, Record<string, unknown>>;
    };
    expect(definitions.Words).toEqual({ type: "string", enum: ["a", "b"] });
    expect(definitions.Frames).toMatchObject({
      type: "object",
      discriminator: { propertyName: "type" },
    });
    // Two equal constants, a branch that isn't a constant, a tag that isn't required: left as written.
    expect(definitions.Repeated).toEqual(schema.definitions.Repeated);
    expect(definitions.Mixed).toEqual(schema.definitions.Mixed);
    expect(definitions.Untagged).toEqual(schema.definitions.Untagged);
  });

  it("agrees with full ajv on the schemas as written, for every frame the tests and the fake front use", () => {
    const theirs = referenceAjv().getSchema(
      "urn:tqa:protocol#/definitions/ServerFrame",
    );
    const retired = readFileSync(
      new URL(
        "../../../crates/tqa-types/tests/fixtures/retired/pre-adr-0035.jsonl",
        import.meta.url,
      ),
      "utf8",
    )
      .split("\n")
      .filter(Boolean)
      .map((line) => JSON.parse(line) as unknown);
    const frames: unknown[] = [
      ...transcriptFrames,
      ...messagesFrames,
      ...(longTranscriptFrames() as unknown[]),
      ...retired,
    ];
    expect(frames.length).toBeGreaterThan(300);
    for (const frame of frames)
      expect(core.ServerFrame(frame), JSON.stringify(frame)).toBe(
        theirs?.(frame),
      );
  });

  it("agrees with full ajv on a generated corpus, valid and broken, for every definition", () => {
    const reference = referenceAjv();
    const schemas = browserSchemas({
      protocol: protocolSchema,
      api: apiSchema,
      admin: adminSchema,
    }) as Record<
      string,
      { definitions: Record<string, Record<string, unknown>> }
    >;
    let compared = 0;
    let accepted = 0;
    let refused = 0;
    for (const [ref, mine] of generated) {
      const [id, name] = ref.split("#/definitions/") as [string, string];
      const theirs = reference.getSchema(ref);
      for (const value of corpus(name, schemas[id]?.definitions ?? {})) {
        const verdict = mine(value);
        expect(verdict, `${ref} on ${JSON.stringify(value)}`).toBe(
          theirs?.(value),
        );
        compared += 1;
        if (verdict) accepted += 1;
        else refused += 1;
      }
    }
    // Positive controls: the corpus is large, and both verdicts occur often, so agreement isn't trivial.
    expect(compared).toBeGreaterThan(5000);
    expect(accepted).toBeGreaterThan(500);
    expect(refused).toBeGreaterThan(2000);
  });
});

// WEB-U-014: a task page's first load carries the core validators. What only other pages decode (New task's and
// Settings' agent profiles, the files panel's previews, a workspace's shares) lives in its own module, loaded there.
describe("WEB-U-014 the core validators leave out what only other pages decode", () => {
  it.each([
    ["profiles", profiles, ["Profiles"]],
    ["previews", previews, ["FileContent", "FileDiff", "SheetPreview"]],
    ["shares", shares, ["WorkspaceShare", "WorkspaceShares"]],
  ] as const)(
    "keeps the %s definitions in their own module",
    (_, module, names) => {
      for (const name of names) {
        expect(module).toHaveProperty(name);
        expect(core).not.toHaveProperty(name);
      }
    },
  );
});

// WEB-U-014: the page only asks whether an answer is valid, so the generated validators point every failure at one
// shared, frozen error object instead of building a detailed one each time (about 10 KB gzip of every page's first
// load). Their arrays, counts and branches are untouched: the full-ajv agreement above is what proves the same pass
// and fail for every definition of every module.
describe("WEB-U-014 one shared error object in the generated validators", () => {
  it("replaces each error object, nested parameters and quoted braces included, and nothing else", () => {
    const code = [
      'if(x){validate1.errors = [{instancePath,schemaPath:"#/type",keyword:"type",params:{type: "string"}}];return false;}',
      'const err0 = {instancePath:instancePath+"/a",schemaPath:"#/x}{",keyword:"enum",params:{allowedValues: schema1.enum}};',
      "if(vErrors === null){vErrors = [err0];}else {vErrors.push(err0);}errors++;",
    ].join("");
    const out = sharedErrors(code);
    expect(out).toContain("validate1.errors = [sharedError];return false;");
    expect(out).toContain("const err0 = sharedError;");
    expect(out).toContain(
      "if(vErrors === null){vErrors = [err0];}else {vErrors.push(err0);}errors++;",
    );
    expect(out).toMatch(/^const sharedError = Object\.freeze\(\{\}\);/);
    expect(out).not.toContain("instancePath:");
  });

  it("refuses code that already names the shared object", () => {
    expect(() => sharedErrors("const sharedError = 1;")).toThrow();
  });

  it.each(Object.entries({ ...MODULES }))(
    "leaves no error object in the %s module",
    (name) => {
      const text = readFileSync(
        new URL(`./generated/validators.${name}.js`, import.meta.url),
        "utf8",
      );
      expect(text).not.toContain("{instancePath");
    },
  );
});

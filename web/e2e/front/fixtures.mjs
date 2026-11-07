import { readFile } from "node:fs/promises";
import { isIPv6 } from "node:net";

import Ajv from "ajv";
import ts from "typescript";

import { onReset } from "./state.mjs";

// Share the typed, synthetic fixture with reducer tests without a second wire definition.
const fixtureModules = new Map();
async function fixture(name) {
  const source = await readFile(
    new URL(`../../src/test/${name}.ts`, import.meta.url),
    "utf8",
  );
  const compiled = ts.transpileModule(source, {
    compilerOptions: { module: ts.ModuleKind.ESNext },
  });
  let output = compiled.outputText;
  for (const [dependency, url] of fixtureModules)
    output = output.replaceAll(`"./${dependency}"`, JSON.stringify(url));
  const url = `data:text/javascript;base64,${Buffer.from(output).toString("base64")}`;
  fixtureModules.set(name, url);
  return import(url);
}
const fixtures = await fixture("frames");
export const { transcriptFrames, ready, frame, conversationId } = fixtures;
export const { templates } = await fixture("templates");
export const { messagesFrames } = await fixture("messages");
const schema = JSON.parse(
  await readFile(
    new URL(
      "../../../crates/tqa-types/schema/tqa-protocol-v1.json",
      import.meta.url,
    ),
    "utf8",
  ),
);
const ajv = new Ajv({ strict: true, allowUnionTypes: true });
ajv.addFormat("ipv6", isIPv6);
for (const [format, max] of [
  ["uint16", 65535],
  ["uint32", 4294967295],
  ["uint64", Number.MAX_SAFE_INTEGER],
]) {
  ajv.addFormat(format, {
    type: "number",
    validate: (n) => Number.isSafeInteger(n) && n >= 0 && n <= max,
  });
}
// Signed 64-bit values (the workspace link's Unix seconds), bounded as src/protocol/validate.ts bounds them.
ajv.addFormat("int64", {
  type: "number",
  validate: (n) => Number.isSafeInteger(n),
});
ajv.addFormat("int32", {
  type: "number",
  validate: (n) => Number.isInteger(n) && n >= -2147483648 && n <= 2147483647,
});
ajv.addFormat(
  "date-time",
  (value) =>
    /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?(?:Z|[+-]\d{2}:\d{2})$/i.test(
      value,
    ) && Number.isFinite(Date.parse(value)),
);
const validate = ajv.compile({ ...schema, $ref: "#/definitions/ServerFrame" });

/** Every fixture, including dynamically numbered events, must obey the real schema. */
export function validFrame(value) {
  if (!validate(value))
    throw new Error(
      `Invalid fake-front frame: ${ajv.errorsText(validate.errors)}`,
    );
  return value;
}
for (const value of transcriptFrames) validFrame(value);

export const user = {
  id: "0195d8c0-0000-7000-8000-000000000002",
  email: "member@example.invalid",
  role: "admin",
};
export const models = {
  items: [
    {
      name: "local",
      model: "local-model",
      protocol: "openai_chat",
      display_name: null,
      description: null,
      verified: null,
      context_window: 32768,
      vision: false,
      native_tools: true,
      reasoning: { allowed: ["off", "medium"], default: "medium" },
      runs_on: { where: "this_computer", provider: null },
    },
  ],
};

models.items.push({
  ...models.items[0],
  name: "second",
  model: "second-model",
});

// 15c's known issue (WEB-U-321/322): the "same-name-routes" scenario lists two routes an administrator named "Qwen",
// the first by name failing its check; any other reset lists the usual two.
const usual = [...models.items];
onReset((scenarios) => {
  models.items = scenarios.includes("same-name-routes")
    ? [
        {
          ...usual[0],
          name: "qwen-a",
          model: "qwen3-4b",
          display_name: "Qwen",
          verified: false,
        },
        {
          ...usual[0],
          name: "qwen-b",
          model: "qwen3-8b",
          display_name: "Qwen",
          verified: true,
        },
      ]
    : [...usual];
});

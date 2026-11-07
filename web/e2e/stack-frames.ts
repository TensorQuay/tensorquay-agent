import { readFileSync } from "node:fs";
import { isIPv6 } from "node:net";

import Ajv from "ajv";

import type { ServerFrame } from "../src/protocol/generated/protocol";

const schema: unknown = JSON.parse(
  readFileSync(
    new URL("../src/protocol/generated/protocol.schema.json", import.meta.url),
    "utf8",
  ),
);
if (schema === null || typeof schema !== "object" || Array.isArray(schema))
  throw new Error("The generated protocol schema must be an object");
const ajv = new Ajv({ strict: true, allowUnionTypes: true });
ajv.addFormat("ipv6", isIPv6);
for (const [name, minimum, maximum] of [
  ["uint16", 0, 65535],
  ["uint32", 0, 4294967295],
  ["uint64", 0, Number.MAX_SAFE_INTEGER],
  ["int32", -2147483648, 2147483647],
  ["int64", -Number.MAX_SAFE_INTEGER, Number.MAX_SAFE_INTEGER],
] as const)
  ajv.addFormat(name, {
    type: "number",
    validate: (value) =>
      Number.isSafeInteger(value) && value >= minimum && value <= maximum,
  });
ajv.addFormat(
  "date-time",
  (value: string) =>
    /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?(?:Z|[+-]\d{2}:\d{2})$/i.test(
      value,
    ) && Number.isFinite(Date.parse(value)),
);
const valid = ajv.compile<ServerFrame>({
  ...schema,
  $ref: "#/definitions/ServerFrame",
});

/** Validate bounded recorded frames without importing the browser's JSON modules in Node. */
export function parseStackFrame(source: string): ServerFrame {
  if (source.length > 262144 || Buffer.byteLength(source, "utf8") > 262144)
    throw new Error("Invalid event size");
  const value: unknown = JSON.parse(source);
  if (!valid(value)) throw new Error("Invalid recorded protocol event");
  return value;
}

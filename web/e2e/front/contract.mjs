// The fake front answers as the gateway's contract says (openapi.json), so it can't drift from the gateway unnoticed:
// a message's acceptance once lacked message_id and held, and no test saw it (GW-I-129). Every JSON response it sends is
// checked against the documented schema for its method, path and status; server.mjs records a mismatch, and the e2e
// run's teardown fails on it.
import { readFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import Ajv2020 from "ajv/dist/2020.js";

const openapi = JSON.parse(
  readFileSync(
    new URL("../../../crates/tqa-gateway/openapi.json", import.meta.url),
    "utf8",
  ),
);

/** Where the fake front records drift, per port, for the e2e teardown to read. */
export const driftLog = (port) =>
  join(tmpdir(), `tqa-fake-front-${port}-drift.log`);

const ajv = new Ajv2020({ strict: false, logger: false, allErrors: false });
ajv.addFormat(
  "uuid",
  /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i,
);
ajv.addFormat("date-time", (value) => Number.isFinite(Date.parse(value)));
for (const name of [
  "int32",
  "int64",
  "uri",
  "email",
  "ipv4",
  "ipv6",
  "hostname",
  "byte",
  "binary",
])
  ajv.addFormat(name, () => true);
ajv.addSchema(openapi, "openapi");

const pointer = (...parts) =>
  parts
    .map((part) => String(part).replaceAll("~", "~0").replaceAll("/", "~1"))
    .join("/");

/** Each documented operation: its path as a pattern, most specific first, and its responses' schema pointers. */
const routes = Object.entries(openapi.paths)
  .flatMap(([path, operations]) =>
    Object.entries(operations)
      .filter(([, operation]) => operation?.responses)
      .map(([method, operation]) => ({
        method: method.toUpperCase(),
        path,
        pattern: new RegExp(`^${path.replace(/\{[^}]+\}/g, "[^/]+")}$`),
        params: (path.match(/\{/g) ?? []).length,
        responses: Object.fromEntries(
          Object.entries(operation.responses).map(([status, response]) => {
            const named = response.$ref?.split("/").pop();
            const shown = named
              ? openapi.components.responses[named]
              : response;
            if (!shown?.content?.["application/json"]?.schema)
              return [status, null];
            return [
              status,
              named
                ? `openapi#/${pointer("components", "responses", named, "content", "application/json", "schema")}`
                : `openapi#/${pointer("paths", path, method, "responses", status, "content", "application/json", "schema")}`,
            ];
          }),
        ),
      })),
  )
  .sort((a, b) => a.params - b.params);

/**
 * Why a JSON response breaks the contract, or null when it keeps it. A route the contract doesn't document (the fake
 * front's own test hooks) isn't checked.
 */
export function contractProblem(method, pathname, status, body) {
  const route = routes.find(
    (candidate) =>
      candidate.method === method && candidate.pattern.test(pathname),
  );
  if (!route) return null;
  const known = String(status) in route.responses;
  const ref = known ? route.responses[String(status)] : route.responses.default;
  if (ref === undefined)
    return `${method} ${route.path} answered ${status}, which openapi.json doesn't document`;
  if (ref === null) return null;
  const validate = ajv.getSchema(ref) ?? ajv.compile({ $ref: ref });
  if (validate(body)) return null;
  return `${method} ${route.path} ${status}: ${ajv.errorsText(validate.errors)}`;
}

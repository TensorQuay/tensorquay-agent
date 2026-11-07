/** Operations under this prefix are administration only (ARCHITECTURE §11, ADR-0028). */
const ADMIN_PREFIX = "/api/v1/admin/";
const LOCAL = "#/definitions/";
const SHARED = "urn:tqa:api#/definitions/";

/**
 * The definitions an OpenAPI node references, by name.
 * @param {unknown} node
 * @param {Set<string>} found
 * @returns {Set<string>}
 */
function references(node, found = new Set()) {
  if (Array.isArray(node)) node.forEach((part) => references(part, found));
  else if (node && typeof node === "object")
    for (const [key, value] of Object.entries(node))
      if (key === "$ref" && typeof value === "string") {
        const name = /^#\/(?:components\/schemas|definitions)\/(.+)$/.exec(
          value,
        )?.[1];
        if (name) found.add(name);
      } else references(value, found);
  return found;
}

/**
 * Every definition reachable from the named ones.
 * @param {Iterable<string>} names
 * @param {Record<string, unknown>} definitions
 */
function closure(names, definitions) {
  const reached = new Set();
  const pending = [...names];
  while (pending.length) {
    const name = pending.pop();
    if (reached.has(name) || !(name in definitions)) continue;
    reached.add(name);
    pending.push(...references(definitions[name]));
  }
  return reached;
}

/** Points an administration definition's references to shared definitions at the shared API schema. */
function toShared(node, shared) {
  if (Array.isArray(node)) return node.map((part) => toShared(part, shared));
  if (!node || typeof node !== "object") return node;
  return Object.fromEntries(
    Object.entries(node).map(([key, value]) => {
      const name =
        key === "$ref" && typeof value === "string" && value.startsWith(LOCAL)
          ? value.slice(LOCAL.length)
          : null;
      return [
        key,
        name && shared.has(name) ? SHARED + name : toShared(value, shared),
      ];
    }),
  );
}

/**
 * Splits the browser's API definitions: those only administration operations use go to their own schema, which only
 * the admin pages load, so the conversation page does not carry them (WEB-U-014 budget). A definition any other
 * operation reaches, or none reaches, stays shared.
 * @param {{ paths: Record<string, unknown> }} api the OpenAPI document
 * @param {Record<string, unknown>} definitions its schemas as `#/definitions/` JSON Schema
 */
export function splitAdmin(api, definitions) {
  const used = { admin: new Set(), other: new Set() };
  for (const [path, operations] of Object.entries(api.paths))
    references(
      operations,
      used[path.startsWith(ADMIN_PREFIX) ? "admin" : "other"],
    );
  const shared = closure(used.other, definitions);
  const admin = [...closure(used.admin, definitions)].filter(
    (name) => !shared.has(name),
  );
  if (admin.length === 0)
    throw new Error("No administration definitions found in the API");
  const names = new Set(admin);
  return {
    shared: Object.fromEntries(
      Object.entries(definitions).filter(([name]) => !names.has(name)),
    ),
    admin: Object.fromEntries(
      admin.sort().map((name) => [name, toShared(definitions[name], shared)]),
    ),
  };
}

/**
 * The shared definitions the browser validates something it receives against: those any operation's responses reach,
 * and those the administration definitions refer to. The browser validates responses only (src/protocol/validate.ts),
 * so a definition only ever sent (a request body) stays out of its runtime schema (WEB-U-014 budget; the TypeScript
 * types keep every definition).
 * @param {{ paths: Record<string, Record<string, { responses?: unknown }>> }} api the OpenAPI document
 * @param {Record<string, unknown>} shared the shared definitions from splitAdmin
 * @param {Record<string, unknown>} admin the administration definitions from splitAdmin
 */
export function receivedOnly(api, shared, admin) {
  const roots = new Set();
  for (const operations of Object.values(api.paths))
    for (const operation of Object.values(operations))
      if (operation && typeof operation === "object")
        references(operation.responses, roots);
  for (const node of Object.values(admin))
    for (const name of references(node)) roots.add(name);
  for (const value of JSON.stringify(admin).matchAll(
    /"urn:tqa:api#\/definitions\/([^"]+)"/g,
  ))
    roots.add(value[1]);
  const kept = closure(roots, shared);
  return Object.fromEntries(
    Object.entries(shared).filter(([name]) => kept.has(name)),
  );
}

/**
 * The protocol definitions the browser validates something it receives against: what ServerFrame (and the events and
 * the queue reason inside it) reaches, and what the browser's API schemas refer to. Frames that travel only between
 * the runtime and the gateway (the Git push relay's) stay out (WEB-U-014 budget).
 * @param {{ definitions: Record<string, unknown> }} schema the protocol schema
 * @param {unknown[]} apiParts the browser's API schema parts, which refer to it as urn:tqa:protocol
 */
export function receivedProtocol(schema, apiParts) {
  const roots = new Set(["ServerFrame", "Event", "QueueReason"]);
  for (const value of JSON.stringify(apiParts).matchAll(
    /"urn:tqa:protocol#\/definitions\/([^"]+)"/g,
  ))
    roots.add(value[1]);
  const kept = closure(roots, schema.definitions);
  return {
    ...schema,
    definitions: Object.fromEntries(
      Object.entries(schema.definitions).filter(([name]) => kept.has(name)),
    ),
  };
}

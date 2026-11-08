// The connector registry admin API on the fake front (ADR-0031): a seeded store with versions, 409s, 400 details
// from the closed reason list and Idempotency-Key replay, like src/test/connectors.ts for the unit tests.
import { seededMap } from "./state.mjs";
const NAME = /^[a-z](?:[a-z0-9]|_(?!_))*$/;
/** The contract's bound (AdminConnectors) and a name an [[mcp.servers]] entry holds. */
const MAX_CONNECTORS = 64;
const RESERVED = ["files"];
const seeded = (name, extra) => ({
  name,
  url: `https://mcp.${name}.example/mcp`,
  auth: { scheme: "none", required: false },
  call_timeout_seconds: 60,
  enabled: true,
  enabled_by_default: false,
  display_name: null,
  key_missing: false,
  version: 1,
  seed: null,
  last_set_by: "admin",
  updated_at: "2026-09-26T09:00:00Z",
  ...extra,
});
const connectors = seededMap(
  [
    seeded("tracker", {
      display_name: "Issue tracker",
      auth: { scheme: "bearer", required: true },
      secret_ref: "secrets/tracker",
      tools: { allow: ["create_issue", "list_issues"] },
      enabled_by_default: true,
      seed: "current",
      last_set_by: "seed",
    }),
    seeded("wiki", { enabled: false, tools: { deny: ["delete_page"] } }),
  ].map((item) => [item.name, item]),
);
const replies = seededMap();

/** The field problems the gateway reports first (document.rs), enough for the page's refusals. */
function problems(name, input) {
  const found = [];
  if (!NAME.test(name) || name.length > 16 || name.endsWith("_"))
    found.push({ field: "name", reason: "name_invalid" });
  if (!/^https:\/\/[^\s@#/]+(\/[^\s#]*)?$/.test(input.url ?? ""))
    found.push({ field: "url", reason: "base_url_invalid" });
  else if (/[?&](api_key|token|signature)=/i.test(input.url))
    found.push({ field: "url", reason: "auth_query_invalid" });
  const scheme = input.auth?.scheme ?? "none";
  if (scheme === "none" && (input.auth?.required || input.secret_ref))
    found.push({ field: "auth", reason: "auth_invalid" });
  return found;
}

function put(name, input, key, answer) {
  if (replies.has(key)) return answer.ok(200, replies.get(key));
  const found = problems(name, input);
  if (found.length) return answer.refuse(400, "invalid_request", found);
  if (input.secret_ref && !answer.mayUse(input.secret_ref, "connector", name))
    return answer.refuse(409, "secret_not_allowed");
  const existing = connectors.get(name);
  if (!existing && RESERVED.includes(name))
    return answer.refuse(409, "name_taken");
  if (existing ? input.version !== existing.version : input.version != null)
    return answer.refuse(409, "conflict");
  if (!existing && connectors.size >= MAX_CONNECTORS)
    return answer.refuse(409, "connector_limit");
  const fields = { ...input };
  delete fields.version;
  const item = seeded(name, {
    ...fields,
    auth: { scheme: "none", required: false, ...fields.auth },
    display_name: fields.display_name ?? null,
    key_missing: !!fields.secret_ref && !answer.hasSecret(fields.secret_ref),
    version: (existing?.version ?? 0) + 1,
    seed: existing?.seed ?? null,
    updated_at: new Date().toISOString(),
  });
  connectors.set(name, item);
  replies.set(key, item);
  answer.ok(existing ? 200 : 201, item);
}

function remove(name, url, answer) {
  const item = connectors.get(name);
  if (!item) return answer.refuse(404, "not_found");
  if (String(item.version) !== url.searchParams.get("version"))
    return answer.refuse(409, "conflict");
  connectors.delete(name);
  answer.none();
}

/** Answers /api/v1/admin/connectors*; returns false for any other path. */
export async function connectorsRoute(url, method, read, key, answer) {
  const name = /^\/api\/v1\/admin\/connectors\/([^/]+)$/.exec(
    url.pathname,
  )?.[1];
  if (url.pathname === "/api/v1/admin/connectors" && method === "GET")
    answer.ok(200, {
      generation: 1,
      items: [...connectors.values()]
        .map((item) =>
          item.secret_ref
            ? { ...item, key_missing: !answer.hasSecret(item.secret_ref) }
            : item,
        )
        .sort((a, b) => a.name.localeCompare(b.name)),
    });
  else if (name && method === "PUT")
    put(decodeURIComponent(name), await read(), key, answer);
  else if (name && method === "DELETE")
    remove(decodeURIComponent(name), url, answer);
  else return false;
  return true;
}

// Each person's own connector credentials: the enabled connectors with whether the caller holds a credential;
// a value is accepted and dropped at once, never returned.
const credentials = new WeakMap();
const PRINTABLE = /^[\x21-\x7e]{1,4096}$/;

/** Answers /api/v1/connectors and /api/v1/connectors/{name}/credential; returns false for any other path. */
export async function userConnectorRoute(
  member,
  url,
  method,
  read,
  reply,
  refuse,
) {
  const name = /^\/api\/v1\/connectors\/([^/]+)\/credential$/.exec(
    url.pathname,
  )?.[1];
  const reads = /^\/api\/v1\/connectors\/([^/]+)\/credential\/reads$/.exec(
    url.pathname,
  )?.[1];
  if (url.pathname !== "/api/v1/connectors" && !name && !reads) return false;
  if (!credentials.has(member)) credentials.set(member, new Map());
  const held = credentials.get(member);
  // The reads of the caller's own credential: 404 when they hold none there.
  if (reads) {
    const connector = decodeURIComponent(reads);
    if (!held.has(connector)) refuse(404, "not_found", "Not found.");
    else
      reply(200, {
        items: [
          {
            id: "0195d8c0-0000-7000-8000-000000000e11",
            consumer_kind: "connector",
            consumer: connector,
            conversation_id: null,
            run_id: null,
            outcome: "ok",
            count: 1,
            first_at: held.get(connector),
            last_at: held.get(connector),
          },
        ],
        next: null,
      });
    return true;
  }
  if (!name && method === "GET") {
    reply(200, {
      items: [...connectors.values()]
        .filter((item) => item.enabled)
        .sort((a, b) => a.name.localeCompare(b.name))
        .map((item) => ({
          name: item.name,
          display_name: item.display_name,
          scheme: item.auth.scheme ?? "none",
          required: item.auth.required ?? false,
          has_credential: held.has(item.name),
          credential_updated_at: held.get(item.name) ?? null,
        })),
    });
    return true;
  }
  const item = connectors.get(decodeURIComponent(name ?? ""));
  if (method === "PUT") {
    const input = await read();
    if (!item || !item.enabled) refuse(404, "not_found", "Not found.");
    else if ((item.auth.scheme ?? "none") === "none")
      refuse(409, "credential_not_used", "The connector sends no credential.");
    else if (typeof input.value !== "string" || !PRINTABLE.test(input.value))
      refuse(400, "invalid_request", "Invalid value.");
    else {
      held.set(item.name, new Date().toISOString());
      reply(204);
    }
    return true;
  }
  if (method === "DELETE") {
    if (!item || !held.delete(item.name))
      refuse(404, "not_found", "Not found.");
    else reply(204);
    return true;
  }
  refuse(404, "not_found", "Not found.");
  return true;
}

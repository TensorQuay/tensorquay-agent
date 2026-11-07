// Decision engines on the fake front (ADR-0013 point 6): a seeded store with versions, 409s (conflict, the limit, and
// in use with its counts), 400 details and Idempotency-Key replay, like src/test/decision-engines.ts for the unit tests.
import { seededMap } from "./state.mjs";
const NAME = /^[a-z][a-z0-9_-]{0,63}$/;
const MAX_ENGINES = 16;
/** The engines profiles or open tasks name, which a delete refuses with who uses them. */
const IN_USE = { policy: { conversations: 3, profiles: ["team"] } };
const seeded = (name, extra) => ({
  name,
  protocol: "decision_v1",
  url: `https://${name}.engines.example`,
  auth: { scheme: "bearer", required: false },
  timeout_ms: 2000,
  exposure: "none",
  enabled: true,
  key_missing: false,
  version: 1,
  seed: null,
  last_set_by: "admin",
  updated_at: "2026-09-28T09:00:00Z",
  ...extra,
});
const engines = seededMap(
  [
    seeded("policy", {
      display_name: "Policy engine",
      key_ref: "secrets/spare",
      exposure: "calls",
    }),
    seeded("reviewer", {
      display_name: "Review assistant",
      key_ref: "secrets/spare",
      exposure: "full",
      enabled: false,
    }),
    seeded("legacy", {
      key_ref: "secrets/legacy-engine",
      seed: "current",
      last_set_by: "seed",
    }),
  ].map((item) => [item.name, item]),
);
const replies = seededMap();

/** The field problems the gateway reports first (document.rs), enough for the page's refusals. */
function problems(name, input) {
  const found = [];
  if (!NAME.test(name)) found.push({ field: "name", reason: "name_invalid" });
  if (!/^https:\/\/[^\s@#/]+(\/[^\s#]*)?$/.test(input.url ?? ""))
    found.push({ field: "url", reason: "base_url_invalid" });
  if (input.key_ref && (input.auth?.scheme ?? "none") === "none")
    found.push({ field: "key_ref", reason: "auth_invalid" });
  const timeout = input.timeout_ms ?? 2000;
  if (timeout < 1 || timeout > 10000)
    found.push({ field: "timeout_ms", reason: "limit_out_of_range" });
  return found;
}

function put(name, input, key, answer) {
  if (replies.has(key)) return answer.ok(200, replies.get(key));
  const found = problems(name, input);
  if (found.length) return answer.refuse(400, "invalid_request", found);
  const existing = engines.get(name);
  // A seeded engine's secret is bound to it by the seed, so keeping the key it has needs no new binding.
  if (
    input.key_ref &&
    input.key_ref !== existing?.key_ref &&
    !answer.mayUse(input.key_ref, "engine", name)
  )
    return answer.refuse(409, "secret_not_allowed");
  if (existing ? input.version !== existing.version : input.version != null)
    return answer.refuse(409, "conflict");
  if (!existing && engines.size >= MAX_ENGINES)
    return answer.refuse(409, "decision_engine_limit");
  const fields = { ...input };
  delete fields.version;
  const item = seeded(name, {
    ...fields,
    auth: { scheme: "none", required: false, ...fields.auth },
    version: (existing?.version ?? 0) + 1,
    seed: existing?.seed ?? null,
    updated_at: new Date().toISOString(),
  });
  engines.set(name, item);
  replies.set(key, item);
  answer.ok(existing ? 200 : 201, withKey(item, answer));
}

function remove(name, url, answer) {
  const item = engines.get(name);
  if (!item) return answer.refuse(404, "not_found");
  if (String(item.version) !== url.searchParams.get("version"))
    return answer.refuse(409, "conflict");
  const used = IN_USE[name];
  if (used)
    return answer.refuse(409, "decision_engine_in_use", [], {
      default: false,
      conversations: used.conversations,
      mcp_servers: [],
      profiles: used.profiles,
      personal_profiles: 0,
    });
  engines.delete(name);
  answer.none();
}

const withKey = (item, answer) =>
  item.key_ref
    ? { ...item, key_missing: !answer.hasSecret(item.key_ref) }
    : item;

/** Answers /api/v1/admin/decision/engines*; returns false for any other path. */
export async function enginesRoute(url, method, read, key, answer) {
  const name = /^\/api\/v1\/admin\/decision\/engines\/([^/]+)$/.exec(
    url.pathname,
  )?.[1];
  if (url.pathname === "/api/v1/admin/decision/engines" && method === "GET")
    answer.ok(200, {
      generation: 1,
      items: [...engines.values()]
        .map((item) => withKey(item, answer))
        .sort((a, b) => a.name.localeCompare(b.name)),
    });
  else if (name && method === "PUT")
    put(decodeURIComponent(name), await read(), key, answer);
  else if (name && method === "DELETE")
    remove(decodeURIComponent(name), url, answer);
  else return false;
  return true;
}

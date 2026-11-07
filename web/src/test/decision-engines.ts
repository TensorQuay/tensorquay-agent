import { ApiError, type FieldProblem } from "../api/error";
import type {
  Engine,
  EngineFields,
  EnginesApi,
} from "../settings/admin/decision-engines";

const refuse = (
  status: number,
  code: string,
  details: FieldProblem[] = [],
  inUse?: ApiError["inUse"],
) => new ApiError(status, code, "fixture refusal", undefined, details, inUse);
const invalid = (field: string, reason: string) =>
  refuse(400, "invalid_request", [{ field, reason }]);

export function engine(name: string, extra: Partial<Engine> = {}): Engine {
  return {
    name,
    protocol: "decision_v1",
    url: `https://${name}.example`,
    auth: { scheme: "bearer", required: false },
    key_ref: "secrets/engine-key",
    timeout_ms: 2000,
    exposure: "none",
    enabled: true,
    key_missing: false,
    version: 1,
    seed: null,
    last_set_by: "admin",
    updated_at: "2026-09-29T01:00:00Z",
    ...extra,
  };
}

/** The gateway's checks that the fake repeats (document.rs): enough to drive each refusal the page explains. */
function problem(name: string, fields: EngineFields): ApiError | null {
  if (!/^[a-z][a-z0-9_-]{0,63}$/.test(name))
    return invalid("name", "name_invalid");
  if (!/^https:\/\/[^\s@#/]+(\/[^\s#]*)?$/.test(fields.url))
    return invalid("url", "base_url_invalid");
  if (fields.key_ref && (fields.auth?.scheme ?? "none") === "none")
    return invalid("key_ref", "auth_invalid");
  const timeout = fields.timeout_ms ?? 2000;
  if (timeout < 1 || timeout > 10000)
    return invalid("timeout_ms", "limit_out_of_range");
  return null;
}

/**
 * In-memory engines with the gateway's semantics: versions, `409 conflict`, `decision_engine_limit`,
 * `decision_engine_in_use` with its counts, `secret_not_allowed`, 400 `details[]` and Idempotency-Key replay.
 */
export function fakeEngines({
  engines = [],
  secrets = ["secrets/engine-key"],
  routes = ["local", "cheap"],
  limit = 16,
  inUse = {},
  bindings = {},
}: {
  engines?: Engine[];
  secrets?: string[];
  /** The installation's model routes, in order. */
  routes?: string[];
  limit?: number;
  /** The engines profiles or open tasks name: active tasks and shared profile names. */
  inUse?: Record<string, { conversations: number; profiles: string[] }>;
  /** The engines each secret is allowed for; a secret missing here allows every engine. */
  bindings?: Record<string, string[]>;
} = {}) {
  const store = new Map(engines.map((item) => [item.name, { ...item }]));
  const replies = new Map<string, Engine | null>();
  const writes: { name: string; fields: EngineFields; version?: number }[] = [];
  const allowed = new Map(Object.entries(bindings));
  const allowCalls: { ref: string; name: string }[] = [];
  const api: EnginesApi = {
    engines: () =>
      Promise.resolve({
        generation: 1,
        items: [...store.values()].sort((a, b) => a.name.localeCompare(b.name)),
      }),
    secretRefs: () => Promise.resolve(secrets),
    routeNames: () => Promise.resolve(routes),
    putEngine(name, fields, version, key) {
      writes.push({
        name,
        fields,
        ...(version === undefined ? {} : { version }),
      });
      const replay = replies.get(key);
      if (replay) return Promise.resolve(replay);
      const refusal = problem(name, fields);
      if (refusal) return Promise.reject(refusal);
      if (
        fields.key_ref &&
        allowed.get(fields.key_ref)?.includes(name) === false
      )
        return Promise.reject(refuse(409, "secret_not_allowed"));
      const existing = store.get(name);
      if (existing ? version !== existing.version : version !== undefined)
        return Promise.reject(refuse(409, "conflict"));
      if (!existing && store.size >= limit)
        return Promise.reject(refuse(409, "decision_engine_limit"));
      const item = engine(name, {
        ...fields,
        auth: { scheme: "none", required: false, ...fields.auth },
        key_missing: !!fields.key_ref && !secrets.includes(fields.key_ref),
        version: (existing?.version ?? 0) + 1,
        seed: existing?.seed ?? null,
      });
      store.set(name, item);
      replies.set(key, item);
      return Promise.resolve({ ...item });
    },
    allowSecret(ref, name) {
      allowCalls.push({ ref, name });
      allowed.set(ref, [...(allowed.get(ref) ?? []), name]);
      return Promise.resolve();
    },
    deleteEngine(name, version, key) {
      if (replies.has(key)) return Promise.resolve();
      const existing = store.get(name);
      if (!existing) return Promise.reject(refuse(404, "not_found"));
      if (existing.version !== version)
        return Promise.reject(refuse(409, "conflict"));
      const used = inUse[name];
      if (used)
        return Promise.reject(
          refuse(409, "decision_engine_in_use", [], {
            default: false,
            conversations: used.conversations,
            mcp_servers: [],
            profiles: used.profiles,
            personal_profiles: 0,
          }),
        );
      store.delete(name);
      replies.set(key, null);
      return Promise.resolve();
    },
  };
  return { api, writes, allowCalls };
}

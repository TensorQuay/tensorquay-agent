import { ApiError, type FieldProblem } from "../api/error";
import type { RouteModels } from "../protocol/generated/api";
import {
  NEW_ROUTE,
  type ProbeResult,
  type RouteFields,
  type RouteItem,
  type RoutesApi,
} from "../settings/admin/routes";

const NAME = /^[A-Za-z0-9._-]{1,64}$/;
const refuse = (status: number, code: string, details: FieldProblem[] = []) =>
  new ApiError(status, code, "fixture refusal", undefined, details);

export function route(name: string, extra: Partial<RouteItem> = {}): RouteItem {
  return {
    ...NEW_ROUTE,
    base_url: "https://models.example.invalid/v1",
    model: `${name}-model`,
    key_ref: `secrets/${name}`,
    name,
    key_missing: false,
    key_ref_invalid: false,
    version: 1,
    seed: null,
    last_set_by: "admin",
    updated_at: "2026-09-25T09:00:00Z",
    fallback_of: [],
    // Never checked; a check stores its result on the route, as the gateway's list shows it (GW-I-140).
    probe: { state: "never", probed_at: null, observed: null },
    ...extra,
  };
}

/** The gateway's checks for a route write, with a few of the closed reasons (ADR-0028). */
function check(name: string, fields: RouteFields): FieldProblem[] {
  const problems: FieldProblem[] = [];
  if (!NAME.test(name))
    problems.push({ field: "name", reason: "route_name_invalid" });
  if (!fields.model) problems.push({ field: "model", reason: "model_empty" });
  if (!/^https?:\/\/[^@/?#\s]+/.test(fields.base_url))
    problems.push({ field: "base_url", reason: "base_url_invalid" });
  if (fields.auth.kind !== "none" && !fields.key_ref)
    problems.push({ field: "key_ref", reason: "key_ref_required" });
  else if (
    fields.key_ref &&
    !/^secrets\/[A-Za-z0-9_-]{1,120}$/.test(fields.key_ref)
  )
    problems.push({ field: "key_ref", reason: "key_ref_not_reference" });
  if (fields.fallbacks.includes(name) || fields.fallbacks.length > 3)
    problems.push({ field: "fallbacks", reason: "fallback_invalid" });
  return problems;
}

type State = {
  routes: Map<string, RouteItem>;
  secrets: Set<string>;
  /** Each stored secret's value by name: a fixture's own, or the one pasted. */
  values: Map<string, string>;
  replies: Map<string, RouteItem | null>;
};

function put(
  state: State,
  name: string,
  fields: RouteFields,
  version: number | undefined,
  key: string,
) {
  const replay = state.replies.get(key);
  if (replay) return replay;
  const problems = check(name, fields);
  if (problems.length) throw refuse(400, "invalid_request", problems);
  const existing = state.routes.get(name);
  if (existing && version !== existing.version) throw refuse(409, "conflict");
  if (!existing && version !== undefined) throw refuse(404, "not_found");
  const item: RouteItem = {
    ...fields,
    name,
    key_missing: fields.key_ref !== null && !state.secrets.has(fields.key_ref),
    key_ref_invalid: false,
    version: (existing?.version ?? 0) + 1,
    seed: existing?.seed ?? null,
    last_set_by: "admin",
    updated_at: "2026-09-26T10:00:00Z",
    fallback_of: existing?.fallback_of ?? [],
    // A change leaves the last check describing the old settings.
    probe:
      existing && existing.probe.state !== "never"
        ? { ...existing.probe, state: "stale" }
        : { state: "never", probed_at: null, observed: null },
  };
  state.routes.set(name, item);
  state.replies.set(key, item);
  return { ...item };
}

function remove(state: State, name: string, version: number, key: string) {
  if (state.replies.has(key)) return;
  const existing = state.routes.get(name);
  if (!existing) throw refuse(404, "not_found");
  if (existing.version !== version) throw refuse(409, "conflict");
  if (existing.fallback_of.length) throw refuse(409, "route_is_fallback");
  state.routes.delete(name);
  state.replies.set(key, null);
}

/** A route check's result: verified by default, or with the observation's parts overridden. */
export function probeResult(
  state: ProbeResult["state"] = "verified",
  observed: Partial<ProbeResult["observed"]> = {},
): ProbeResult {
  return {
    state,
    probed_at: "2026-09-26T23:00:00Z",
    route_version: 1,
    observed: {
      ok: state === "verified",
      streaming: true,
      tool_call: "passed",
      round_trip: "passed",
      usage: { reported: true, cached: false, reasoning: false },
      reasoning_accepted: null,
      first_byte_ms: 420,
      total_ms: 1830,
      tokens: { prompt: 212, completion: 38 },
      failure: null,
      warnings: [],
      ...observed,
    },
  };
}

const settle = <T>(work: () => T): Promise<T> => {
  try {
    return Promise.resolve(work());
  } catch (error) {
    return Promise.reject(
      error instanceof Error ? error : new Error("fixture failure"),
    );
  }
};

/** An in-memory routes API with the contract's semantics: versions, 409s, field details, replay, key_missing. */
export function fakeRoutes({
  routes = [],
  secrets = [],
  probes = {},
  listings = {},
  bindings,
}: {
  routes?: RouteItem[];
  secrets?: string[];
  /** Each route's check outcome: a result, or the refusal the gateway gives (key_missing, rate_limited, …). */
  probes?: Record<string, ProbeResult | ApiError>;
  /** Each route's model listing (WEB-U-067), or the gateway's refusal; by default its own model and one more. */
  listings?: Record<string, RouteModels | ApiError>;
  /** Vault slice 3: the routes each secret is allowed for; a secret missing here (or no map) allows every route. */
  bindings?: Record<string, string[]>;
} = {}) {
  const state: State = {
    routes: new Map(routes.map((item) => [item.name, { ...item }])),
    secrets: new Set(secrets),
    values: new Map(
      secrets.map((ref) => [ref.replace(/^secrets\//, ""), "fixture value"]),
    ),
    replies: new Map(),
  };
  /** Paste key's creates, value included, so a test can see it was sent once and nowhere else. */
  const secretWrites: { name: string; value: string }[] = [];
  const allowed = new Map(Object.entries(bindings ?? {}));
  const allowCalls: {
    ref: string;
    consumer: { kind: "route"; name: string };
  }[] = [];
  let allowRefused = false;
  const writes: {
    name: string;
    fields?: RouteFields;
    version?: number;
    key: string;
  }[] = [];
  const api: RoutesApi = {
    routes: () =>
      settle(() => ({
        items: [...state.routes.values()].sort((a, b) =>
          a.name.localeCompare(b.name),
        ),
        generation: 1,
      })),
    putRoute(name, fields, version, key) {
      writes.push({
        name,
        fields,
        ...(version === undefined ? {} : { version }),
        key,
      });
      return settle(() => {
        const ref = fields.key_ref;
        // An existing secret not allowed for this route is refused without details (vault slice 3).
        if (ref && allowed.get(ref)?.includes(name) === false)
          throw refuse(409, "secret_not_allowed");
        return put(state, name, fields, version, key);
      });
    },
    allowSecret(ref, name) {
      allowCalls.push({ ref, consumer: { kind: "route", name } });
      return settle(() => {
        if (allowRefused) throw refuse(503, "store_unavailable");
        allowed.set(ref, [...(allowed.get(ref) ?? []), name]);
      });
    },
    deleteRoute(name, version, key) {
      writes.push({ name, version, key });
      return settle(() => remove(state, name, version, key));
    },
    secretRefs: () => settle(() => [...state.secrets].sort()),
    createSecret(name, value) {
      secretWrites.push({ name, value });
      return settle(() => {
        if (/^(git|connector)-/i.test(name))
          throw refuse(400, "invalid_request", [
            { field: "name", reason: "reserved_prefix" },
          ]);
        // Create-only, as the gateway answers a PUT without a version: an existing name is a conflict.
        if (state.values.has(name)) throw refuse(409, "conflict");
        state.values.set(name, value);
        state.secrets.add(`secrets/${name}`);
        return `secrets/${name}`;
      });
    },
    exportUrl: () => "/fixture/models.toml",
    models: (name) =>
      settle(() => {
        const item = state.routes.get(name);
        if (!item) throw refuse(404, "not_found");
        if (item.key_missing) throw refuse(409, "key_missing");
        const outcome = listings[name] ?? {
          route: name,
          protocol: item.protocol,
          models: [{ id: item.model }, { id: `${item.model}-mini` }],
          truncated: false,
          failure: null,
        };
        if (outcome instanceof ApiError) throw outcome;
        return outcome;
      }),
    probe(name, key) {
      writes.push({ name, key });
      return settle(() => {
        const item = state.routes.get(name);
        if (!item) throw refuse(404, "not_found");
        if (item.key_missing) throw refuse(409, "key_missing");
        const outcome = probes[name] ?? probeResult();
        if (outcome instanceof ApiError) throw outcome;
        state.routes.set(name, {
          ...item,
          probe: {
            state: outcome.state,
            probed_at: outcome.probed_at,
            observed: outcome.observed,
          },
        });
        return outcome;
      });
    },
  };
  return {
    api,
    writes,
    secretWrites,
    allowCalls,
    /** Allowing a secret fails from now on. */
    refuseAllow() {
      allowRefused = true;
    },
    storedValue: (name: string) => state.values.get(name),
    /** Another administrator's change, to provoke a stale version. */
    touch(name: string) {
      const item = state.routes.get(name);
      if (item) state.routes.set(name, { ...item, version: item.version + 1 });
    },
  };
}

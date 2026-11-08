import { ApiError, type FieldProblem } from "../api/error";
import type {
  Connector,
  ConnectorFields,
  ConnectorsApi,
} from "../settings/admin/connectors";

const refuse = (status: number, code: string, details: FieldProblem[] = []) =>
  new ApiError(status, code, "fixture refusal", undefined, details);
const invalid = (field: string, reason: string) =>
  refuse(400, "invalid_request", [{ field, reason }]);

export function connector(
  name: string,
  extra: Partial<Connector> = {},
): Connector {
  return {
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
  };
}

/** The gateway's checks that the fake repeats (document.rs): enough to drive each refusal the page explains. */
function problem(name: string, fields: ConnectorFields): ApiError | null {
  if (
    !/^[a-z](?:[a-z0-9]|_(?!_))*$/.test(name) ||
    name.length > 16 ||
    name.endsWith("_")
  )
    return invalid("name", "name_invalid");
  if (!/^https:\/\/[^\s@#/]+(\/[^\s#]*)?$/.test(fields.url))
    return invalid("url", "base_url_invalid");
  if (/[?&](api_key|token|signature)=/i.test(fields.url))
    return invalid("url", "auth_query_invalid");
  const scheme = fields.auth?.scheme ?? "none";
  if (scheme === "none" && fields.auth?.required)
    return invalid("auth", "auth_invalid");
  if (fields.secret_ref && scheme === "none")
    return invalid("secret_ref", "auth_invalid");
  const timeout = fields.call_timeout_seconds ?? 60;
  if (timeout < 1 || timeout > 600)
    return invalid("call_timeout_seconds", "limit_out_of_range");
  return null;
}

/**
 * An in-memory registry with the gateway's semantics: versions, `409 conflict`, `name_taken` (also a name an
 * [[mcp.servers]] entry holds), `connector_limit`, 400 `details[]` and Idempotency-Key replay.
 */
export function fakeConnectors({
  connectors = [],
  secrets = ["secrets/tracker"],
  reserved = ["files"],
  limit = 64,
  bindings = {},
}: {
  connectors?: Connector[];
  secrets?: string[];
  reserved?: string[];
  limit?: number;
  /** The connectors each secret is allowed for; a secret missing here allows every connector. */
  bindings?: Record<string, string[]>;
} = {}) {
  const store = new Map(connectors.map((item) => [item.name, { ...item }]));
  const replies = new Map<string, Connector | null>();
  const writes: { name: string; fields: ConnectorFields; version?: number }[] =
    [];
  const allowed = new Map(Object.entries(bindings));
  const allowCalls: {
    ref: string;
    consumer: { kind: "connector"; name: string };
  }[] = [];
  const api: ConnectorsApi = {
    connectors: () =>
      Promise.resolve({
        generation: 1,
        items: [...store.values()].sort((a, b) => a.name.localeCompare(b.name)),
      }),
    secretRefs: () => Promise.resolve(secrets),
    putConnector(name, fields, version, key) {
      writes.push({
        name,
        fields,
        ...(version === undefined ? {} : { version }),
      });
      const replay = replies.get(key);
      if (replay) return Promise.resolve(replay);
      const refusal = problem(name, fields);
      if (refusal) return Promise.reject(refusal);
      // An existing secret not allowed for this connector is refused without details.
      if (
        fields.secret_ref &&
        allowed.get(fields.secret_ref)?.includes(name) === false
      )
        return Promise.reject(refuse(409, "secret_not_allowed"));
      const existing = store.get(name);
      if (!existing && reserved.includes(name))
        return Promise.reject(refuse(409, "name_taken"));
      if (existing ? version !== existing.version : version !== undefined)
        return Promise.reject(refuse(409, "conflict"));
      if (!existing && store.size >= limit)
        return Promise.reject(refuse(409, "connector_limit"));
      const item = connector(name, {
        ...fields,
        auth: { scheme: "none", required: false, ...fields.auth },
        display_name: fields.display_name ?? null,
        key_missing:
          !!fields.secret_ref && !secrets.includes(fields.secret_ref),
        version: (existing?.version ?? 0) + 1,
        seed: existing?.seed ?? null,
      });
      store.set(name, item);
      replies.set(key, item);
      return Promise.resolve({ ...item });
    },
    allowSecret(ref, name) {
      allowCalls.push({ ref, consumer: { kind: "connector", name } });
      allowed.set(ref, [...(allowed.get(ref) ?? []), name]);
      return Promise.resolve();
    },
    deleteConnector(name, version, key) {
      if (replies.has(key)) return Promise.resolve();
      const existing = store.get(name);
      if (!existing) return Promise.reject(refuse(404, "not_found"));
      if (existing.version !== version)
        return Promise.reject(refuse(409, "conflict"));
      store.delete(name);
      replies.set(key, null);
      return Promise.resolve();
    },
  };
  /** Another administrator's change: the stored connector moves to the next version. */
  const touch = (name: string) => {
    const existing = store.get(name);
    if (existing)
      store.set(name, { ...existing, version: existing.version + 1 });
  };
  return { api, writes, touch, allowCalls };
}

import { ApiError, type FieldProblem } from "../api/error";
import type {
  EgressApi,
  EgressList,
  EgressPresetId,
  EgressRule,
} from "../settings/admin/egress";

/** The gateway's presets (the first hour's F20; docs/admin-guide/web-settings.md): each a fixed list of rules, `*` only through open_web. */
export const PRESET_RULES: Readonly<
  Record<EgressPresetId, { host: string; ports: number[] }[]>
> = {
  package_registries: [
    "pypi.org",
    "files.pythonhosted.org",
    "registry.npmjs.org",
    "crates.io",
    "static.crates.io",
    "index.crates.io",
    "proxy.golang.org",
    "sum.golang.org",
  ].map((host) => ({ host, ports: [443] })),
  open_web: [{ host: "*", ports: [80, 443] }],
  code_hosting: [
    "github.com",
    "codeload.github.com",
    "raw.githubusercontent.com",
    "objects.githubusercontent.com",
    "gitlab.com",
  ].map((host) => ({ host, ports: [443] })),
};

const refuse = (status: number, code: string, details: FieldProblem[] = []) =>
  new ApiError(status, code, "fixture refusal", undefined, details);

export function rule(
  host: string,
  extra: Partial<EgressRule> = {},
): EgressRule {
  return {
    host,
    ports: [443],
    internal: false,
    version: 1,
    seed: null,
    last_set_by: "admin",
    preset: null,
    updated_at: "2026-09-25T09:00:00Z",
    ...extra,
  };
}

/**
 * An in-memory allow-list with the gateway's semantics (slice 4): canonical lower-case hosts, sorted ports, versions,
 * `409 conflict`, `egress_rule_limit` and `egress_disabled`, 400 `details[]` and Idempotency-Key replay.
 */
export function fakeEgress({
  rules = [] as EgressRule[],
  disabled = false,
  limit = 256,
  /** Refuses every internal rule as the gateway does a wildcard's or an address's (`internal_name_only`). */
  refuseInternal = false,
} = {}) {
  const store = new Map(rules.map((item) => [item.host, { ...item }]));
  const replies = new Map<string, EgressRule | null>();
  const writes: {
    host: string;
    ports: number[];
    internal: boolean;
    version?: number;
  }[] = [];
  const applies: EgressPresetId[] = [];
  const removals: EgressPresetId[] = [];
  const api: EgressApi = {
    rules: () =>
      Promise.resolve<EgressList>({
        disabled,
        limit,
        items: [...store.values()].sort((a, b) => a.host.localeCompare(b.host)),
      }),
    putRule(host, { ports, internal }, version, key) {
      writes.push({
        host,
        ports,
        internal,
        ...(version === undefined ? {} : { version }),
      });
      const replay = replies.get(key);
      if (replay) return Promise.resolve(replay);
      if (disabled) return Promise.reject(refuse(409, "egress_disabled"));
      const canonical = host.trim().toLowerCase().replace(/\.$/, "");
      if (!/^[a-z0-9*][a-z0-9.*-]*$/.test(canonical))
        return Promise.reject(
          refuse(400, "invalid_request", [
            { field: "host", reason: "host_invalid" },
          ]),
        );
      // Only one exact host name may be an internal server's (ADR-0035's amendment).
      if (
        internal &&
        (refuseInternal ||
          canonical.includes("*") ||
          /^[\d.]+$|:/.test(canonical))
      )
        return Promise.reject(
          refuse(400, "invalid_request", [
            { field: "internal", reason: "internal_name_only" },
          ]),
        );
      const existing = store.get(canonical);
      if (existing && version !== existing.version)
        return Promise.reject(refuse(409, "conflict"));
      if (!existing && version !== undefined)
        return Promise.reject(refuse(409, "conflict"));
      if (!existing && store.size >= limit)
        return Promise.reject(refuse(409, "egress_rule_limit"));
      const item = rule(canonical, {
        ports: [...ports].sort((a, b) => a - b),
        internal,
        version: (existing?.version ?? 0) + 1,
        seed: existing?.seed ?? null,
        updated_at: "2026-09-26T10:00:00Z",
      });
      store.set(canonical, item);
      replies.set(key, item);
      return Promise.resolve({ ...item });
    },
    deleteRule(host, version, key) {
      if (replies.has(key)) return Promise.resolve();
      if (disabled) return Promise.reject(refuse(409, "egress_disabled"));
      const existing = store.get(host);
      if (!existing) return Promise.reject(refuse(404, "not_found"));
      if (existing.version !== version)
        return Promise.reject(refuse(409, "conflict"));
      store.delete(host);
      replies.set(key, null);
      return Promise.resolve();
    },
    presets: () =>
      Promise.resolve({
        items: (Object.keys(PRESET_RULES) as EgressPresetId[]).map((id) => {
          const tagged = PRESET_RULES[id].filter(
            ({ host }) => store.get(host)?.preset === id,
          ).length;
          return {
            id,
            rules: PRESET_RULES[id].map((each) => ({ ...each })),
            applied:
              tagged === 0
                ? ("none" as const)
                : tagged === PRESET_RULES[id].length
                  ? ("all" as const)
                  : ("partial" as const),
          };
        }),
      }),
    applyPreset(id) {
      applies.push(id);
      if (disabled) return Promise.reject(refuse(409, "egress_disabled"));
      const added = [];
      const kept = [];
      for (const each of PRESET_RULES[id]) {
        if (store.has(each.host)) {
          kept.push({ host: each.host });
          continue;
        }
        store.set(
          each.host,
          rule(each.host, { ports: [...each.ports], preset: id }),
        );
        added.push({ ...each });
      }
      return Promise.resolve({ added, kept });
    },
    removePreset(id) {
      removals.push(id);
      if (disabled) return Promise.reject(refuse(409, "egress_disabled"));
      const removed = [];
      const kept = [];
      for (const { host } of PRESET_RULES[id]) {
        const existing = store.get(host);
        if (existing?.preset === id) {
          store.delete(host);
          removed.push({ host });
        } else if (existing) kept.push({ host });
      }
      return Promise.resolve({ removed, kept });
    },
  };
  /** Another administrator's change: the stored rule moves to the next version. */
  const touch = (host: string) => {
    const existing = store.get(host);
    if (existing)
      store.set(host, { ...existing, version: existing.version + 1 });
  };
  return { api, writes, applies, removals, touch };
}

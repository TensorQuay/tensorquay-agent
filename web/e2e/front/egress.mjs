// The egress allow-list admin API on the fake front (slice 4, ADR-0028): a seeded store with canonical hosts, versions,
// 409s, 400 details from the closed reason list and Idempotency-Key replay, like src/test/egress.ts for the unit tests.
import { onReset, seededMap } from "./state.mjs";
const HOST = /^(?:[a-z0-9*][a-z0-9.*-]{0,252}|[0-9a-f:.]{2,45})$/;
/** The contract's bounds for a rule's ports and the list (AdminEgressRuleWrite, AdminEgressRules). */
const MAX_PORTS = 16;
const MAX_PORT = 65535;
const MAX_RULES = 256;
const seeded = (host, ports, seed) => ({
  host,
  ports,
  version: 1,
  seed,
  last_set_by: seed ? "seed" : "admin",
  preset: null,
  internal: false,
  updated_at: "2026-09-25T09:00:00Z",
});
const rules = seededMap(
  [
    seeded("api.example.com", [443, 8443], "current"),
    seeded("pypi.org", [443], null),
  ].map((item) => [item.host, item]),
);
const replies = seededMap();
// Internal servers' rules the gateway can't use yet (ADR-0035's amendment): this machine's own addresses not named in
// gateway.toml, and the container engine's networks unreadable for now.
onReset((scenarios) => {
  if (!scenarios.includes("internal-rules")) return;
  for (const [host, state] of [
    ["git.corp.example", "internal_never_missing"],
    ["wiki.corp.example", "engine_unknown"],
  ])
    rules.set(host, {
      ...seeded(host, [443], null),
      internal: true,
      internal_state: state,
    });
});
/** An exact address: a rule for it is never a wildcard's, and never an internal server's. */
const isAddress = (host) => host.includes(":") || /^\d+(\.\d+){3}$/.test(host);

/** Whether tasks may reach `host` on `port` now: its own rule, or a wildcard's for a name (never for an address). */
export function egressAllows(host, port) {
  const covers = (rule) =>
    rule.ports.includes(port) &&
    (rule.host === host ||
      (!isAddress(host) &&
        (rule.host === "*" ||
          (rule.host.startsWith("*.") && host.endsWith(rule.host.slice(1))))));
  return [...rules.values()].some(covers);
}
/** The built-in presets, as the gateway lists them; like PRESET_RULES in src/test/egress.ts. */
const on443 = (hosts) => hosts.map((host) => ({ host, ports: [443] }));
const PRESETS = {
  package_registries: on443([
    "pypi.org",
    "files.pythonhosted.org",
    "registry.npmjs.org",
    "crates.io",
    "static.crates.io",
    "index.crates.io",
    "proxy.golang.org",
    "sum.golang.org",
  ]),
  open_web: [{ host: "*", ports: [80, 443] }],
  code_hosting: on443([
    "github.com",
    "codeload.github.com",
    "raw.githubusercontent.com",
    "objects.githubusercontent.com",
    "gitlab.com",
  ]),
};

const canonical = (host) => host.trim().toLowerCase().replace(/\.$/, "");
const validPorts = (ports) =>
  Array.isArray(ports) &&
  ports.length >= 1 &&
  ports.length <= MAX_PORTS &&
  new Set(ports).size === ports.length &&
  ports.every(
    (port) => Number.isInteger(port) && port >= 1 && port <= MAX_PORT,
  );

function put(raw, input, key, answer) {
  if (replies.has(key)) return answer.ok(200, replies.get(key));
  const host = canonical(raw);
  const found = [];
  if (!HOST.test(host)) found.push({ field: "host", reason: "host_invalid" });
  if (!validPorts(input.ports))
    found.push({ field: "ports", reason: "ports_invalid" });
  // Only one exact host name may be an internal server's (ADR-0035's amendment).
  if (input.internal === true && (host.includes("*") || isAddress(host)))
    found.push({ field: "internal", reason: "internal_name_only" });
  if (found.length) return answer.refuse(400, "invalid_request", found);
  const existing = rules.get(host);
  if (
    existing ? input.version !== existing.version : input.version !== undefined
  )
    return answer.refuse(409, "conflict");
  if (!existing && rules.size >= MAX_RULES)
    return answer.refuse(409, "egress_rule_limit");
  const item = {
    host,
    ports: [...input.ports].sort((a, b) => a - b),
    version: (existing?.version ?? 0) + 1,
    seed: existing?.seed ?? null,
    last_set_by: "admin",
    // An edit makes a rule the administrator's own.
    preset: null,
    internal: input.internal === true,
    updated_at: new Date().toISOString(),
  };
  rules.set(host, item);
  replies.set(key, item);
  answer.ok(existing ? 200 : 201, item);
}

function remove(host, url, answer) {
  const item = rules.get(host);
  if (!item) return answer.refuse(404, "not_found");
  if (String(item.version) !== url.searchParams.get("version"))
    return answer.refuse(409, "conflict");
  rules.delete(host);
  answer.none();
}

function presets() {
  return {
    items: Object.entries(PRESETS).map(([id, list]) => {
      const tagged = list.filter(
        ({ host }) => rules.get(host)?.preset === id,
      ).length;
      return {
        id,
        rules: list,
        applied:
          tagged === 0 ? "none" : tagged === list.length ? "all" : "partial",
      };
    }),
  };
}

/** Adds what the list lacks, tagged with the preset; a host with a rule keeps it (listed in kept). */
function apply(id, key, answer) {
  if (replies.has(key)) return answer.ok(200, replies.get(key));
  const added = [];
  const kept = [];
  for (const each of PRESETS[id]) {
    if (rules.has(each.host)) {
      kept.push({ host: each.host });
      continue;
    }
    if (rules.size >= MAX_RULES) return answer.refuse(409, "egress_rule_limit");
    rules.set(each.host, {
      ...seeded(each.host, [...each.ports], null),
      preset: id,
      updated_at: new Date().toISOString(),
    });
    added.push(each);
  }
  if (key) replies.set(key, { added, kept });
  answer.ok(200, { added, kept });
}

/** Removes the preset's rules still as it put them; the administrator's own stay (listed in kept). */
function removePreset(id, key, answer) {
  if (replies.has(key)) return answer.ok(200, replies.get(key));
  const removed = [];
  const kept = [];
  for (const { host } of PRESETS[id]) {
    const existing = rules.get(host);
    if (existing?.preset === id) {
      rules.delete(host);
      removed.push({ host });
    } else if (existing) kept.push({ host });
  }
  if (key) replies.set(key, { removed, kept });
  answer.ok(200, { removed, kept });
}

/** Answers /api/v1/admin/egress*; returns false for any other path. */
export async function egressRoute(url, method, read, key, answer) {
  const preset = /^\/api\/v1\/admin\/egress\/presets(?:\/([^/]+))?$/.exec(
    url.pathname,
  );
  const host = preset
    ? undefined
    : /^\/api\/v1\/admin\/egress\/([^/]+)$/.exec(url.pathname)?.[1];
  if (preset && !preset[1] && method === "GET") answer.ok(200, presets());
  else if (preset?.[1] && !Object.hasOwn(PRESETS, preset[1]))
    answer.refuse(404, "not_found");
  else if (preset?.[1] && method === "POST") apply(preset[1], key, answer);
  else if (preset?.[1] && method === "DELETE")
    removePreset(preset[1], key, answer);
  else if (url.pathname === "/api/v1/admin/egress" && method === "GET")
    answer.ok(200, {
      disabled: false,
      limit: MAX_RULES,
      items: [...rules.values()].sort((a, b) => a.host.localeCompare(b.host)),
    });
  else if (host && method === "PUT")
    put(decodeURIComponent(host), await read(), key, answer);
  else if (host && method === "DELETE")
    remove(decodeURIComponent(host), url, answer);
  else return false;
  return true;
}

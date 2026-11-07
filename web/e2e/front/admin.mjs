// Administration for the fake front (task 090): the readiness detail the Status page reads, as the gateway serves it at
// GET /readyz?detail=1 (administrators only; 503 with the same body when a check fails), and the admin usage and
// secrets API (ADR-0028, ADR-0033).
import { connectorsRoute } from "./connectors.mjs";
import { egressRoute } from "./egress.mjs";
import { enginesRoute } from "./engines.mjs";
import { limitsRoute } from "./limits.mjs";
import { listed, routesRoute } from "./routes.mjs";
import { seededMap } from "./state.mjs";
import { agentTeamsRoute } from "./teams.mjs";
import { templateDiagnostics, templatesRoute } from "./templates.mjs";

const checks = {
  backend_preflight: true,
  image_present: true,
  runtime_revision: true,
  database: true,
  secret_store: true,
};

/** Answers /readyz; returns false for any other path. */
export function readinessRoute(
  url,
  request,
  response,
  memberships,
  json,
  error,
  sessions,
) {
  if (url.pathname !== "/readyz") return false;
  if (url.searchParams.get("detail") !== "1") {
    response.end();
    return true;
  }
  const cookie = request.headers.cookie ?? "";
  const token = /(?:^|;\s*)fixture_session=([^;]+)/.exec(cookie)?.[1];
  const member = memberships.get(token);
  if (!member) {
    error(response, 401, "unauthenticated", "Sign in to continue.");
    return true;
  }
  if (member.account.role !== "admin") {
    error(response, 403, "forbidden", "administrator role is required");
    return true;
  }
  const failing = /(?:^|;\s*)fixture_readiness=not-ready(?:;|$)/.test(cookie);
  json(response, failing ? 503 : 200, {
    ready: !failing,
    checks: { ...checks, image_present: !failing },
    sandbox: { backend: "docker", network_mode: "bridge" },
    // Template diagnostics (templates slice 3), never part of readiness: from the environments store.
    templates: templateDiagnostics(),
    git_credentials_kept: { not_web_address: 0, waiting: 0 },
    // How many tasks are working on a step (GW-I-229): this session's conversations whose run is going, as the
    // gateway counts linked runtimes whose run goes on. Only the signed-in session's, so specs running beside this
    // one never change its count (WEB-E2E-038).
    runs_active: [...(sessions?.get(token)?.values() ?? [])].filter(
      (conversation) => conversation.running,
    ).length,
  });
  return true;
}

// A small seeded store with versions, 409s, 400 details with the closed reason list, and Idempotency-Key replay.
// Values are accepted and dropped at once; nothing here ever holds one.
const NAME = /^[A-Za-z0-9_-]{1,120}$/;
// Personal Git and connector credentials own these prefixes, in any case (bc96026).
const RESERVED = /^(git|connector)-/i;
const secrets = seededMap(
  [
    {
      ref: "secrets/local",
      seed: "current",
      last_set_by: "seed",
      routes: ["local"],
      consumers: [{ kind: "route", name: "local" }],
      // Vault slice 1: last changed long ago, with a reminder that is due.
      rotated_at: "2026-05-01T09:00:00Z",
      rotate_after_days: 90,
      rotation_due: true,
    },
    { ref: "secrets/spare", seed: null, last_set_by: "admin", routes: [] },
    // A long name that truncates on one line, used by three models: the row reads "3 models" (UI-GUIDELINES §7).
    {
      ref: "secrets/provider-gateway-shared-production-key",
      seed: "current",
      last_set_by: "seed",
      routes: ["gateway-fast", "gateway-long", "gateway-vision"],
    },
  ].map((item) => [
    item.ref,
    {
      rotated_at: "2026-09-25T09:00:00Z",
      rotate_after_days: null,
      rotation_due: false,
      consumers: [],
      ...item,
      version: 1,
      updated_at: "2026-09-25T09:00:00Z",
    },
  ]),
);
const replies = seededMap();
const problem = (json, response, status, code, details = [], inUse) =>
  json(response, status, {
    error: {
      code,
      message: "fixture refusal",
      retryable: false,
      ...(details.length ? { details } : {}),
      ...(inUse ? { in_use: inUse } : {}),
    },
  });

// Usage that varies by day, so the page's chart, rankings and change against an earlier period have something to show:
// September runs busier than August, and some days have no requests at all.
const DAY_MS = 86_400_000;
function usage(url, response, json) {
  const group = url.searchParams.get("group") ?? "day";
  const to = url.searchParams.get("to") ?? "2026-09-26";
  const from = url.searchParams.get("from") ?? "2026-08-28";
  if (!["day", "user", "route"].includes(group))
    return problem(json, response, 400, "invalid_request", [
      { field: "group", reason: "group_invalid" },
    ]);
  const figures = (requests) => ({
    requests,
    reported: requests,
    estimated: 0,
    prompt_tokens: requests * 1200,
    completion_tokens: requests * 90,
    cached_tokens: null,
    reasoning_tokens: requests * 20,
  });
  const days = [];
  for (
    let at = Date.parse(`${from}T00:00:00Z`);
    at <= Date.parse(`${to}T00:00:00Z`);
    at += DAY_MS
  ) {
    const date = new Date(at);
    const busy = date.getUTCMonth() === 8 ? 5 : 3;
    days.push([
      date.toISOString().slice(0, 10),
      (date.getUTCDate() * busy) % 9,
    ]);
  }
  const requests = days.reduce((sum, [, count]) => sum + count, 0);
  // People and models split the period's requests, largest first.
  const split = (entries) =>
    entries.map(([key, label, part], index) => ({
      key,
      label,
      ...figures(
        index === entries.length - 1
          ? requests -
              entries
                .slice(0, -1)
                .reduce(
                  (sum, [, , share]) => sum + Math.floor(requests * share),
                  0,
                )
          : Math.floor(requests * part),
      ),
    }));
  const rows =
    group === "day"
      ? days.map(([key, count]) => ({ key, label: null, ...figures(count) }))
      : group === "user"
        ? split([
            [
              "0195d8c0-0000-7000-8000-000000000001",
              "member@example.invalid",
              0.6,
            ],
            [
              "0195d8c0-0000-7000-8000-000000000002",
              "colleague@example.invalid",
              0.3,
            ],
            ["0195d8c0-0000-7000-8000-000000000009", null, 0.1],
          ])
        : split([
            ["local", null, 0.7],
            ["deepseek-flash", null, 0.3],
          ]);
  json(response, 200, {
    from,
    to,
    group,
    rows,
    totals: figures(requests),
    truncated: false,
  });
}

function problems(name, input) {
  const found = [];
  if (!NAME.test(name)) found.push({ field: "name", reason: "name_invalid" });
  else if (RESERVED.test(name))
    found.push({ field: "name", reason: "reserved_prefix" });
  if (typeof input.value !== "string" || input.value.length === 0)
    found.push({ field: "value", reason: "value_empty" });
  return found;
}

function putSecret(name, input, key, response, json) {
  if (replies.has(key)) return json(response, 200, replies.get(key));
  const found = problems(name, input);
  if (found.length)
    return problem(json, response, 400, "invalid_request", found);
  const ref = `secrets/${name}`;
  const existing = secrets.get(ref);
  if (existing && input.version !== existing.version)
    return problem(json, response, 409, "conflict");
  if (!existing && input.version !== undefined)
    return problem(json, response, 404, "not_found");
  const now = new Date().toISOString();
  const item = {
    ref,
    version: (existing?.version ?? 0) + 1,
    updated_at: now,
    // Only a new value moves rotated_at; the reminder stays.
    rotated_at: now,
    rotate_after_days: existing?.rotate_after_days ?? null,
    rotation_due: false,
    consumers: existing?.consumers ?? [],
    seed: existing?.seed ?? null,
    last_set_by: "admin",
    routes: existing?.routes ?? [],
  };
  secrets.set(ref, item);
  replies.set(key, item);
  json(response, existing ? 200 : 201, item);
}

/**
 * A secret's reads (vault slice 2), newest first: a first page whose `next` leads to one older read. Each read is the
 * caller's own, so the admin view names a person the Users page knows.
 */
function secretReads(name, url, member, json, response) {
  if (!secrets.has(`secrets/${name}`))
    return problem(json, response, 404, "not_found");
  const read = (id, extra) => ({
    id,
    consumer_kind: "route",
    consumer: "local",
    conversation_id: null,
    user_id: member.account.id,
    run_id: null,
    outcome: "ok",
    count: 1,
    first_at: "2026-09-26T09:00:00Z",
    last_at: "2026-09-26T09:00:00Z",
    ...extra,
  });
  if (url.searchParams.get("before"))
    return json(response, 200, {
      items: [
        read("0195d8c0-0000-7000-8000-000000000e01", {
          outcome: "unavailable",
          first_at: "2026-09-20T09:00:00Z",
          last_at: "2026-09-20T09:00:00Z",
        }),
      ],
      next: null,
    });
  json(response, 200, {
    items: [
      read("0195d8c0-0000-7000-8000-000000000e03", {
        count: 12,
        first_at: "2026-09-21T09:00:00Z",
        last_at: "2026-09-27T09:00:00Z",
      }),
      read("0195d8c0-0000-7000-8000-000000000e02", {
        consumer_kind: "connector",
        consumer: "tracker",
        outcome: "absent",
      }),
    ],
    next: "0195d8c0-0000-7000-8000-000000000e02",
  });
}

/** Replaces who may use a secret, with the version read (vault slice 3); sorted by kind, then name. */
function putConsumers(name, input, response, json) {
  const item = secrets.get(`secrets/${name}`);
  const list = input.consumers;
  if (
    !Array.isArray(list) ||
    list.some(
      (entry) =>
        !["route", "connector"].includes(entry?.kind) ||
        typeof entry.name !== "string" ||
        entry.name.length === 0,
    )
  )
    return problem(json, response, 400, "invalid_request", [
      { field: "consumers", reason: "consumers_invalid" },
    ]);
  if (!item) return problem(json, response, 404, "not_found");
  if (input.version !== item.version)
    return problem(json, response, 409, "conflict");
  const next = {
    ...item,
    version: item.version + 1,
    updated_at: new Date().toISOString(),
    consumers: [...list].sort((a, b) =>
      `${a.kind}/${a.name}`.localeCompare(`${b.kind}/${b.name}`),
    ),
  };
  secrets.set(next.ref, next);
  json(response, 200, next);
}

/**
 * Whether a route or connector may name a secret (vault slice 3): an existing secret only when it is bound to it; one
 * that does not exist yet is left to key_missing.
 */
function mayUse(ref, kind, name) {
  const item = secrets.get(ref);
  return (
    !item ||
    item.consumers.some((entry) => entry.kind === kind && entry.name === name)
  );
}

/** Sets or clears a secret's rotation reminder with the version read (vault slice 1). */
function putReminder(name, input, response, json) {
  const item = secrets.get(`secrets/${name}`);
  const days = input.rotate_after_days;
  if (days !== null && !(Number.isInteger(days) && days >= 1 && days <= 3650))
    return problem(json, response, 400, "invalid_request", [
      { field: "rotate_after_days", reason: "rotate_after_invalid" },
    ]);
  if (!item) return problem(json, response, 404, "not_found");
  if (input.version !== item.version)
    return problem(json, response, 409, "conflict");
  const age = Date.now() - Date.parse(item.rotated_at);
  const next = {
    ...item,
    version: item.version + 1,
    updated_at: new Date().toISOString(),
    rotate_after_days: days,
    rotation_due: days !== null && age >= days * 86_400_000,
  };
  secrets.set(next.ref, next);
  json(response, 200, next);
}

function deleteSecret(name, url, response, json) {
  const item = secrets.get(`secrets/${name}`);
  if (!item) return problem(json, response, 404, "not_found");
  if (String(item.version) !== url.searchParams.get("version"))
    return problem(json, response, 409, "conflict");
  if (item.routes.length) return problem(json, response, 409, "secret_in_use");
  secrets.delete(item.ref);
  response.writeHead(204);
  response.end();
}

/** Answers /api/v1/admin/*; returns false for any other path. */
export async function adminRoute(
  member,
  url,
  method,
  read,
  request,
  response,
  json,
) {
  if (!url.pathname.startsWith("/api/v1/admin/")) return false;
  if (member.account.role !== "admin") {
    problem(json, response, 403, "forbidden");
    return true;
  }
  const name = /^\/api\/v1\/admin\/secrets\/([^/]+)$/.exec(url.pathname)?.[1];
  const reads = /^\/api\/v1\/admin\/secrets\/([^/]+)\/reads$/.exec(
    url.pathname,
  )?.[1];
  const bindings = /^\/api\/v1\/admin\/secrets\/([^/]+)\/consumers$/.exec(
    url.pathname,
  )?.[1];
  const reminder = /^\/api\/v1\/admin\/secrets\/([^/]+)\/reminder$/.exec(
    url.pathname,
  )?.[1];
  const key = request.headers["idempotency-key"] ?? "";
  const answer = {
    ok: (status, body) => json(response, status, body),
    refuse: (status, code, details, inUse) =>
      problem(json, response, status, code, details, inUse),
    none: () => {
      response.writeHead(204);
      response.end();
    },
    accepted: () => {
      response.writeHead(202);
      response.end();
    },
    text: (body) => {
      response.writeHead(200, {
        "content-type": "application/toml",
        "content-disposition": 'attachment; filename="models.toml"',
      });
      response.end(body);
    },
    listed: (item) => listed(item, (ref) => secrets.has(ref)),
    hasSecret: (ref) => secrets.has(ref),
    mayUse,
  };
  if (await routesRoute(url, method, read, key, answer)) return true;
  if (await egressRoute(url, method, read, key, answer)) return true;
  if (await limitsRoute(url, method, read, key, answer)) return true;
  if (await agentTeamsRoute(member, url, method, read, answer)) return true;
  if (await connectorsRoute(url, method, read, key, answer)) return true;
  if (await enginesRoute(url, method, read, key, answer)) return true;
  if (await templatesRoute(url, method, read, key, answer)) return true;
  // The configured tool servers a shared profile may bring (WEB-U-059).
  if (url.pathname === "/api/v1/admin/mcp/servers" && method === "GET")
    json(response, 200, {
      items: [
        { name: "search", templates: [], enabled_by_default: true },
        {
          name: "tickets",
          templates: ["workstation"],
          enabled_by_default: false,
        },
      ],
    });
  else if (url.pathname === "/api/v1/admin/usage" && method === "GET")
    usage(url, response, json);
  else if (url.pathname === "/api/v1/admin/secrets" && method === "GET")
    json(response, 200, {
      items: [...secrets.values()].sort((a, b) => a.ref.localeCompare(b.ref)),
      limit: 4096,
    });
  else if (reads && method === "GET")
    secretReads(decodeURIComponent(reads), url, member, json, response);
  else if (bindings && method === "PUT")
    putConsumers(decodeURIComponent(bindings), await read(), response, json);
  else if (reminder && method === "PUT")
    putReminder(decodeURIComponent(reminder), await read(), response, json);
  else if (name && method === "PUT")
    putSecret(decodeURIComponent(name), await read(), key, response, json);
  else if (name && method === "DELETE")
    deleteSecret(decodeURIComponent(name), url, response, json);
  else problem(json, response, 404, "not_found");
  return true;
}

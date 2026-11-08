// The routes admin API on the fake front (slice 3, ADR-0028): a seeded store with versions, 409s, 400 details from
// the closed reason list, route_is_fallback and Idempotency-Key replay, like src/test/routes.ts for the unit tests.
import { onReset, seededMap } from "./state.mjs";

const NAME = /^[A-Za-z0-9._-]{1,64}$/;
const BASE_URL = /^https?:\/\/[^@/?#\s]+/;
/** At most this many fallbacks per route (the gateway's bound, ADR-0028). */
const MAX_FALLBACKS = 3;
const NEVER = { state: "never", probed_at: null, observed: null };
const DEFAULTS = {
  display_name: null,
  description: null,
  protocol: "openai_chat",
  auth: { kind: "bearer" },
  key_ref: null,
  context_window: null,
  max_output: null,
  concurrency: null,
  requests_per_minute: null,
  native_tools: true,
  parallel_tools: false,
  vision: false,
  reasoning: null,
  cache_hint: "none",
  stream_usage: false,
  extra_headers: {},
  extra_body: {},
  quirks: [],
  // Per-route tool-schema settings: required on every listed route.
  strict: "auto",
  schema_keywords: "full",
  fallbacks: [],
};
const seeded = (name, fields) => ({
  ...DEFAULTS,
  ...fields,
  name,
  version: 1,
  updated_at: "2026-09-25T09:00:00Z",
  probe: NEVER,
});
/** The routes an install lists before anyone edits one. */
const seeds = () => [
  seeded("local", {
    display_name: "Local model",
    base_url: "http://model-server.example.invalid:8000/v1",
    model: "qwen2.5-coder",
    key_ref: "secrets/local",
    seed: "current",
    last_set_by: "seed",
  }),
  seeded("deepseek-flash", {
    display_name: "DeepSeek Flash",
    base_url: "https://models.example.invalid/v1",
    model: "deepseek-flash",
    key_ref: "secrets/deepseek",
    fallbacks: ["local"],
    seed: null,
    last_set_by: "admin",
  }),
  // The first-hour install's second route: a real description and a real key's reference, which the table
  // must fit at 1024 and 1280 without breaking the key one character per line.
  seeded("small-mac", {
    display_name: "Local, small Mac (4k context)",
    description:
      "The same local model with a 4,096-token context, as Ollama gives a Mac with little memory",
    base_url: "http://host.docker.internal:11434/v1",
    model: "qwen3-4b-ctx4k",
    key_ref: "secrets/provider-gateway-shared-production-key",
    seed: "current",
    last_set_by: "seed",
  }),
];
/**
 * A route whose pasted key the upgrade scrubbed (key_ref_invalid, SEC-033), for the test that gives it a key again
 * (`resetFront(request, ["scrubbed-route"])` in e2e/test.ts).
 */
const scrubbed = () =>
  seeded("team-proxy", {
    display_name: "Team proxy",
    base_url: "https://models.example.invalid/v1",
    model: "gpt-4.1-mini",
    key_ref: null,
    key_ref_invalid: true,
    seed: null,
    last_set_by: "admin",
  });
const routes = seededMap(seeds().map((item) => [item.name, item]));
const replies = seededMap();
onReset((scenarios) => {
  if (scenarios.includes("scrubbed-route"))
    routes.set("team-proxy", scrubbed());
});

/** A listed route: its key state and the routes that list it as a fallback, as the gateway computes them. */
export function listed(item, hasSecret) {
  return {
    ...item,
    key_missing: item.key_ref !== null && !hasSecret(item.key_ref),
    key_ref_invalid: item.key_ref_invalid ?? false,
    fallback_of: [...routes.values()]
      .filter((other) => other.fallbacks.includes(item.name))
      .map((other) => other.name),
  };
}

function problems(name, input) {
  const found = [];
  if (!NAME.test(name))
    found.push({ field: "name", reason: "route_name_invalid" });
  if (!input.model) found.push({ field: "model", reason: "model_empty" });
  if (!BASE_URL.test(input.base_url ?? ""))
    found.push({ field: "base_url", reason: "base_url_invalid" });
  if ((input.auth?.kind ?? "bearer") !== "none" && !input.key_ref)
    found.push({ field: "key_ref", reason: "key_ref_required" });
  else if (
    input.key_ref &&
    !/^secrets\/[A-Za-z0-9_-]{1,120}$/.test(input.key_ref)
  )
    found.push({ field: "key_ref", reason: "key_ref_not_reference" });
  const fallbacks = input.fallbacks ?? [];
  if (
    fallbacks.length > MAX_FALLBACKS ||
    fallbacks.some((other) => other === name || !routes.has(other))
  )
    found.push({ field: "fallbacks", reason: "fallback_invalid" });
  return found;
}

function put(name, input, key, answer) {
  if (replies.has(key)) return answer.ok(200, replies.get(key));
  const found = problems(name, input);
  if (found.length) return answer.refuse(400, "invalid_request", found);
  if (input.key_ref && !answer.mayUse(input.key_ref, "route", name))
    return answer.refuse(409, "secret_not_allowed");
  const existing = routes.get(name);
  if (existing && input.version !== existing.version)
    return answer.refuse(409, "conflict");
  if (!existing && input.version !== undefined)
    return answer.refuse(409, "conflict");
  const fields = { ...input };
  delete fields.version;
  const item = {
    ...DEFAULTS,
    ...fields,
    name,
    version: (existing?.version ?? 0) + 1,
    seed: existing?.seed ?? null,
    last_set_by: "admin",
    updated_at: new Date().toISOString(),
    probe: existing?.probe ?? NEVER,
  };
  routes.set(name, item);
  const reply = answer.listed(item);
  replies.set(key, reply);
  answer.ok(existing ? 200 : 201, reply);
}

function remove(name, url, answer) {
  const item = routes.get(name);
  if (!item) return answer.refuse(404, "not_found");
  if (String(item.version) !== url.searchParams.get("version"))
    return answer.refuse(409, "conflict");
  if ([...routes.values()].some((other) => other.fallbacks.includes(name)))
    return answer.refuse(409, "route_is_fallback");
  routes.delete(name);
  answer.none();
}

/** Answers /api/v1/admin/routes*; returns false for any other path. */
/**
 * A route check: "local" is verified, any other route fails as unreachable; a route without its key is 409. A "schema-"
 * route stands for a provider refusing JSON Schema keywords: its check fails with a 400 and suggests Schema keywords
 * basic, and passes once the route keeps basic keywords only. A "thinker-" route passes but its model thinks unasked
 * (WEB-U-133). A "slow-" route's check runs out of time (a first-hour check before 17.0; WEB-U-264).
 */
/**
 * An Ollama-like local route (WEB-U-269): an "ollama-small-" route's server gives the model 4,096 tokens, under the
 * 16,384 tasks need, so its check fails context_too_small and suggests the route's Context window; an "ollama-roomy-"
 * route's server gives 32,768, so its check passes, warning context_window_differs while the route says otherwise.
 */
function ollamaProbe(name, item, answer) {
  const small = name.startsWith("ollama-small-");
  const context = small ? 4096 : 32768;
  const server_context = {
    server: "ollama",
    context_length: context,
    minimum: 16384,
  };
  const differs = !small && item.context_window !== context;
  answer.ok(200, {
    probe: {
      state: small ? "failed" : "verified",
      probed_at: new Date().toISOString(),
      route_version: item.version,
      observed: {
        ok: !small,
        streaming: true,
        tool_call: small ? "not_run" : "passed",
        round_trip: small ? "not_run" : "passed",
        usage: { reported: true, cached: false, reasoning: false },
        reasoning_accepted: null,
        first_byte_ms: 900,
        total_ms: 2400,
        tokens: { prompt: small ? 0 : 212, completion: small ? 0 : 38 },
        failure: small
          ? {
              code: "context_too_small",
              status: null,
              excerpt: null,
              suggestion: {
                field: "context_window",
                value: String(context),
                reason: "the server's context is under the minimum tasks need",
              },
            }
          : null,
        warnings: differs ? ["context_window_differs"] : [],
        server_context,
      },
    },
  });
}

/**
 * WEB-E2E-039's cases: a "desktop-" route at localhost is unreachable and its gateway reaches
 * host.docker.internal (container_host true), and it passes once its address uses that name; a "linux-" one is
 * unreachable and host.docker.internal doesn't resolve (false); a "wrongpath-" route answers 404 at the protocol's own
 * request (endpoint_missing); a "nousage-" route's server refuses usage reporting until the route stops asking; a
 * "slow-" route's server never answers within the check's time (timeout; WEB-E2E-040).
 */
function reviewProbe(name, item, answer) {
  const local = /^https?:\/\/(localhost|127\.[0-9.]+|\[::1\])(:|\/|$)/i.test(
    item.base_url,
  );
  let failure = null;
  if (name.startsWith("desktop-") && local)
    failure = {
      code: "unreachable",
      status: null,
      excerpt: null,
      container_host: true,
    };
  else if (name.startsWith("linux-") && local)
    failure = {
      code: "unreachable",
      status: null,
      excerpt: null,
      container_host: false,
    };
  else if (name.startsWith("slow-"))
    failure = { code: "timeout", status: null, excerpt: null };
  else if (name.startsWith("wrongpath-"))
    failure = {
      code: "endpoint_missing",
      status: 404,
      excerpt: "404 page not found",
    };
  else if (name.startsWith("nousage-") && item.stream_usage !== false)
    failure = {
      code: "usage_refused",
      status: 400,
      excerpt: "Unrecognized request argument supplied: stream_options",
      suggestion: {
        field: "stream_usage",
        value: "false",
        reason: "the server refuses stream_options",
      },
    };
  const verified = failure === null;
  answer.ok(200, {
    probe: {
      state: verified ? "verified" : "failed",
      probed_at: new Date().toISOString(),
      route_version: item.version,
      observed: {
        ok: verified,
        streaming: verified ? true : null,
        tool_call: verified ? "passed" : "not_run",
        round_trip: verified ? "passed" : "not_run",
        usage: {
          reported: verified && item.stream_usage !== false,
          cached: false,
          reasoning: false,
        },
        reasoning_accepted: null,
        first_byte_ms: verified ? 420 : null,
        total_ms: verified ? 1830 : 200,
        tokens: { prompt: verified ? 212 : 0, completion: verified ? 38 : 0 },
        failure,
        warnings:
          verified && item.stream_usage === false ? ["usage_missing"] : [],
      },
    },
  });
}

function probe(name, answer) {
  const item = routes.get(name);
  if (!item) return answer.refuse(404, "not_found");
  if (answer.listed(item).key_missing) return answer.refuse(409, "key_missing");
  if (name.startsWith("ollama-small-") || name.startsWith("ollama-roomy-"))
    return ollamaProbe(name, item, answer);
  if (/^(desktop|linux|wrongpath|nousage|slow)-/.test(name))
    return reviewProbe(name, item, answer);
  const schemaRefused =
    name.startsWith("schema-") && item.schema_keywords !== "basic";
  const verified =
    name === "local" ||
    name.startsWith("thinker-") ||
    (name.startsWith("schema-") && item.schema_keywords === "basic");
  answer.ok(200, {
    probe: {
      state: verified ? "verified" : "failed",
      probed_at: new Date().toISOString(),
      route_version: item.version,
      observed: {
        ok: verified,
        streaming: verified ? true : null,
        tool_call: verified ? "passed" : "not_run",
        round_trip: verified ? "passed" : "not_run",
        usage: { reported: verified, cached: false, reasoning: false },
        reasoning_accepted: null,
        first_byte_ms: verified ? 420 : null,
        total_ms: verified ? 1830 : 5000,
        tokens: { prompt: verified ? 212 : 0, completion: verified ? 38 : 0 },
        failure: verified
          ? null
          : schemaRefused
            ? {
                code: "upstream_error",
                status: 400,
                excerpt: 'Invalid schema: keyword "pattern" is not supported',
                suggestion: {
                  field: "schema_keywords",
                  value: "basic",
                  reason:
                    "the endpoint refused a JSON Schema keyword in a tool schema; basic keeps only the widely supported ones",
                },
              }
            : name.startsWith("slow-")
              ? { code: "timeout", status: null, excerpt: null }
              : { code: "unreachable", status: null, excerpt: null },
        warnings: name.startsWith("thinker-") ? ["thinks_unasked"] : [],
      },
    },
  });
}

export async function routesRoute(url, method, read, key, answer) {
  const name = /^\/api\/v1\/admin\/routes\/([^/]+)$/.exec(url.pathname)?.[1];
  const probed = /^\/api\/v1\/admin\/routes\/([^/]+)\/probe$/.exec(
    url.pathname,
  )?.[1];
  if (probed && method === "POST") {
    probe(decodeURIComponent(probed), answer);
    return true;
  }
  // WEB-U-067: the saved route's upstream models, as the gateway lists them.
  const listing = /^\/api\/v1\/admin\/routes\/([^/]+)\/models$/.exec(
    url.pathname,
  )?.[1];
  if (listing && method === "GET") {
    const item = routes.get(decodeURIComponent(listing));
    if (!item)
      answer.ok(404, {
        error: { code: "not_found", message: "not_found", retryable: false },
      });
    else
      answer.ok(200, {
        route: item.name,
        protocol: item.protocol,
        models: [
          { id: item.model },
          { id: "qwen3-coder-32b", display_name: "Qwen3 Coder 32B" },
          { id: "qwen3-embedding" },
        ],
        truncated: false,
        failure: null,
      });
    return true;
  }
  if (url.pathname === "/api/v1/admin/routes" && method === "GET")
    answer.ok(200, {
      items: [...routes.values()]
        .sort((a, b) => a.name.localeCompare(b.name))
        .map((item) => answer.listed(item)),
      generation: 1,
    });
  else if (url.pathname === "/api/v1/admin/routes/export" && method === "GET")
    answer.text(
      [...routes.values()]
        .map((item) => `[[routes]]\nname = ${JSON.stringify(item.name)}\n`)
        .join("\n"),
    );
  else if (name && name !== "export" && method === "PUT")
    put(decodeURIComponent(name), await read(), key, answer);
  else if (name && name !== "export" && method === "DELETE")
    remove(decodeURIComponent(name), url, answer);
  else return false;
  return true;
}

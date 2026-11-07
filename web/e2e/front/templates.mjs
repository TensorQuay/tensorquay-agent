// The environment (template) registry admin API on the fake front (slice 4, ADR-0032): seeded Workstation and Minimal,
// versions, 409s with in_use, the offered-only default, 400 details, and a checker that answers on the next list.
import { seededMap, seededSet } from "./state.mjs";
const NAME = /^[a-z][a-z0-9_-]{0,63}$/;
const PINNED = /^(?:sha256:[0-9a-f]{64}|[^\s@]+@sha256:[0-9a-f]{64})$/;
const DIGEST = `sha256:${"a".repeat(64)}`;
const MAX_TEMPLATES = 32;
/** What an environment without its own size gets, and the most any may have ([sandbox.ceiling]). */
export const SIZE_DEFAULTS = {
  cpus: 1,
  memory_mib: 512,
  pids: 256,
  workspace_mib: 10240,
};
export const SIZE_CEILING = {
  cpus: 8,
  memory_mib: 16384,
  pids: 4096,
  workspace_mib: 102400,
};
const seeded = (name, extra) => ({
  name,
  display_name: name[0].toUpperCase() + name.slice(1),
  description: null,
  image: `tqa-runtime:${name}@${DIGEST}`,
  capabilities: ["shell"],
  cpus: null,
  memory_mib: null,
  pids: null,
  workspace_mib: null,
  clamped: [],
  clamped_from: null,
  // Not a built-in, and not an administrator's change of one (the environments API).
  builtin: null,
  customised: false,
  is_default: false,
  enabled: true,
  version: 1,
  last_set_by: "seed",
  seed: "current",
  check: {
    state: "verified",
    reason: null,
    checked_version: 1,
    secret_files: null,
    scan_limit: null,
  },
  base: null,
  image_id: DIGEST,
  bound_conversations: 0,
  updated_at: "2026-09-26T09:00:00Z",
  ...extra,
});
const templates = seededMap(
  [
    seeded("workstation", {
      is_default: true,
      bound_conversations: 2,
      capabilities: ["shell", "python", "node", "build", "browser"],
    }),
    // Minimal is Shell and Python, as its image (GW-U-101).
    seeded("minimal", { capabilities: ["shell", "python"] }),
    seeded("broken", {
      display_name: "Broken tools",
      base: "workstation",
      seed: null,
      last_set_by: "admin",
      check: {
        state: "refused",
        reason: "not_derived",
        checked_version: 1,
        secret_files: null,
        scan_limit: null,
      },
    }),
    // A built-in an administrator edited (WEB-U-098): theirs now, so rebuilds no longer update it, until it is reset.
    seeded("data", {
      display_name: "Data science",
      description: "Notebooks, tables and charts",
      image: `sha256:${"d".repeat(64)}`,
      image_id: `sha256:${"d".repeat(64)}`,
      capabilities: ["shell", "python"],
      builtin: { image_created: "2026-09-20T09:00:00Z", updated_at: null },
      customised: true,
      last_set_by: "admin",
      seed: null,
      version: 2,
    }),
  ].map((item) => [item.name, item]),
);
const replies = seededMap();

/** One environment image on the engine host (the environment images API): usable, untagged and unused by default. */
const hostImage = (hex, extra) => ({
  id: `sha256:${hex.repeat(64)}`,
  tags: [],
  created: "2026-09-28T09:00:00Z",
  size_bytes: 1_500_000_000,
  runtime: "current",
  environment: null,
  display_name: null,
  capabilities: [],
  capabilities_valid: true,
  usable: true,
  reason: null,
  based_on: null,
  used_by: [],
  sandboxes: 0,
  built_in: null,
  sizes: null,
  ...extra,
});
/** The host's images, newest first (WEB-U-097): the built-ins' builds, one derived, one untagged, two unusable. */
const images = [
  hostImage("1", {
    tags: ["tqa-runtime:workstation"],
    created: "2026-09-29T09:00:00Z",
    environment: "workstation",
    display_name: "Workstation",
    capabilities: ["shell", "python", "node", "build", "browser"],
    built_in: "registered",
    size_bytes: 3_200_000_000,
  }),
  hostImage("7", {
    tags: ["team/notebooks:1"],
    created: "2026-09-29T08:00:00Z",
    environment: "workstation",
    display_name: "Workstation",
    capabilities: ["shell", "python", "node", "build", "browser"],
    based_on: "workstation",
  }),
  hostImage("2", {
    tags: ["tqa-runtime:base"],
    environment: "minimal",
    display_name: "Minimal",
    capabilities: ["shell", "python"],
    built_in: "registered",
    size_bytes: 900_000_000,
  }),
  hostImage("3", {
    tags: ["tqa-runtime:data"],
    environment: "data",
    display_name: "Data science",
    capabilities: ["shell", "python"],
    built_in: "registered",
    size_bytes: 2_100_000_000,
  }),
  hostImage("4", { capabilities: ["shell"], based_on: "minimal" }),
  hostImage("5", {
    tags: ["team/old-tools:2025"],
    created: "2025-11-02T09:00:00Z",
    runtime: "too_old",
    usable: false,
    reason: "too old for this version: rebuild it",
  }),
  hostImage("6", {
    tags: ["postgres:16"],
    created: "2025-10-01T09:00:00Z",
    runtime: "none",
    usable: false,
    reason: "there is no TensorQuay runtime in it",
  }),
];

/** Hands a customised built-in back to its rebuilds (the gateway's reset: its refusals in the same order). */
function reset(name, input, key, answer) {
  if (replies.has(key)) return answer.ok(200, replies.get(key));
  const item = templates.get(name);
  if (!item) return answer.refuse(404, "not_found");
  if (input.version !== item.version) return answer.refuse(409, "conflict");
  if (!item.customised) return answer.refuse(409, "not_customised");
  const build = images.find(
    (each) => each.built_in !== null && each.environment === name,
  );
  if (!build) return answer.refuse(503, "unavailable");
  if (
    item.is_default &&
    (build.id !== item.image ||
      build.capabilities.join() !== item.capabilities.join())
  )
    return answer.refuse(409, "template_is_default");
  const next = {
    ...item,
    image: build.id,
    image_id: build.id,
    display_name: build.display_name ?? item.display_name,
    capabilities: build.capabilities,
    version: item.version + 1,
    last_set_by: "builtin",
    customised: false,
    check: {
      state: "unchecked",
      reason: null,
      checked_version: null,
      secret_files: null,
      scan_limit: null,
    },
  };
  templates.set(name, next);
  replies.set(key, next);
  answer.ok(200, next);
}
/** Environments asked to check again: an image named "fragile@…" is refused then (its engine image changed). */
const rechecked = seededSet();
const offered = (item) =>
  item.enabled &&
  (item.check.state === "verified" ||
    (item.check.state === "unchecked" && item.last_set_by === "seed"));

/** The checker answers once a list is read after a write: an image named "refused@…" is refused, others verified. */
function settle() {
  for (const item of templates.values())
    if (item.check.state === "unchecked" && item.check.checked_version === null)
      templates.set(item.name, {
        ...item,
        // An image named "secrets@…" holds credential files in its own layers (ADR-0032; WEB-U-060).
        check: item.image.startsWith("secrets@")
          ? {
              state: "refused",
              reason: "image_secret_file",
              checked_version: item.version,
              secret_files: {
                items: [
                  {
                    path: "/root/.git-credentials",
                    rule: "git_credentials",
                    acknowledged: false,
                  },
                  {
                    path: "/opt/test/fixture_key",
                    rule: "private_key",
                    acknowledged: true,
                  },
                ],
                more: 2,
              },
              scan_limit: null,
            }
          : item.image.startsWith("refused@")
            ? {
                state: "refused",
                reason: "rootfs_missing",
                checked_version: item.version,
                secret_files: null,
                scan_limit: null,
              }
            : rechecked.delete(item.name) && item.image.startsWith("fragile@")
              ? {
                  state: "refused",
                  reason: "runtime_too_old",
                  checked_version: item.version,
                  secret_files: null,
                  scan_limit: null,
                }
              : {
                  state: "verified",
                  reason: null,
                  checked_version: item.version,
                  secret_files: null,
                  scan_limit: null,
                },
      });
}

/** The readiness detail's template diagnostics (settings/templates.rs `unavailable`), from the store. */
export function templateDiagnostics() {
  const item = [...templates.values()].find((entry) => entry.is_default);
  const reason =
    !item || offered(item)
      ? null
      : !item.enabled
        ? "disabled"
        : item.check.state === "refused"
          ? (item.check.reason ?? "refused")
          : "unverified";
  return {
    default: item?.name ?? null,
    default_offered: Boolean(item) && reason === null,
    default_reason: reason,
    checks_pending: [...templates.values()].filter(
      (entry) => entry.check.state === "unchecked",
    ).length,
  };
}

function put(name, input, key, answer) {
  if (replies.has(key)) return answer.ok(200, replies.get(key));
  const found = [];
  if (!NAME.test(name)) found.push({ field: "name", reason: "name_invalid" });
  // The Add page's address (GW-U-093).
  else if (name === "new")
    found.push({ field: "name", reason: "name_reserved" });
  if (!PINNED.test(input.image ?? ""))
    found.push({ field: "image", reason: "image_not_pinned" });
  if (found.length) return answer.refuse(400, "invalid_request", found);
  const existing = templates.get(name);
  if (existing && input.version == null)
    return answer.refuse(409, "name_taken");
  if (existing && input.version !== existing.version)
    return answer.refuse(409, "conflict");
  if (!existing && input.version != null)
    return answer.refuse(404, "not_found");
  if (!existing && templates.size >= MAX_TEMPLATES)
    return answer.refuse(409, "template_count");
  // What the default runs stays fixed while it is the default; the check resets only when what it checked changes.
  const changed =
    existing &&
    ["image", "capabilities", "cpus", "memory_mib"].some(
      (field) =>
        JSON.stringify(input[field] ?? null) !==
        JSON.stringify(existing[field] ?? null),
    );
  if (existing?.is_default && (input.enabled === false || changed))
    return answer.refuse(409, "template_is_default");
  const fields = { ...input };
  delete fields.version;
  delete fields.default;
  const item = seeded(name, {
    ...existing,
    ...fields,
    cpus: fields.cpus ?? null,
    memory_mib: fields.memory_mib ?? null,
    pids: fields.pids ?? null,
    workspace_mib: fields.workspace_mib ?? null,
    clamped: [],
    clamped_from: null,
    builtin: existing?.builtin ?? null,
    // An administrator's edit of a built-in makes it theirs: rebuilds no longer update it (WEB-U-098).
    customised: existing?.builtin ? true : (existing?.customised ?? false),
    enabled: fields.enabled ?? true,
    version: (existing?.version ?? 0) + 1,
    last_set_by: "admin",
    seed: existing?.seed ?? null,
    base: existing?.base ?? "workstation",
    check:
      existing && !changed
        ? existing.check
        : {
            state: "unchecked",
            reason: null,
            checked_version: null,
            secret_files: null,
            scan_limit: null,
          },
    updated_at: new Date().toISOString(),
  });
  templates.set(name, item);
  replies.set(key, item);
  answer.ok(existing ? 200 : 201, item);
}

function remove(name, url, key, answer) {
  if (replies.has(key)) return answer.none();
  const item = templates.get(name);
  if (!item) return answer.refuse(404, "not_found");
  if (String(item.version) !== url.searchParams.get("version"))
    return answer.refuse(409, "conflict");
  if (item.is_default || item.bound_conversations > 0)
    return answer.refuse(409, "template_in_use", undefined, {
      default: item.is_default,
      conversations: item.bound_conversations,
      mcp_servers: [],
    });
  templates.delete(name);
  replies.set(key, null);
  answer.none();
}

function makeDefault(name, input, key, answer) {
  if (replies.has(key)) return answer.ok(200, replies.get(key));
  const item = templates.get(name);
  if (!item) return answer.refuse(404, "not_found");
  if (input.version !== item.version) return answer.refuse(409, "conflict");
  if (!offered(item)) return answer.refuse(409, "template_unavailable");
  for (const [other, entry] of templates)
    templates.set(other, { ...entry, is_default: other === name });
  replies.set(key, templates.get(name));
  answer.ok(200, templates.get(name));
}

/** Answers /api/v1/admin/templates*; returns false for any other path. */
export async function templatesRoute(url, method, read, key, answer) {
  const match =
    /^\/api\/v1\/admin\/templates\/([^/]+)(?:\/(default|check|reset))?$/.exec(
      url.pathname,
    );
  const name = match ? decodeURIComponent(match[1]) : null;
  if (url.pathname === "/api/v1/admin/templates" && method === "GET") {
    const items = [...templates.values()].sort((a, b) =>
      a.name.localeCompare(b.name),
    );
    settle();
    answer.ok(200, {
      generation: 1,
      items,
      defaults: SIZE_DEFAULTS,
      ceiling: SIZE_CEILING,
    });
  } else if (
    url.pathname === "/api/v1/admin/environment-images" &&
    method === "GET"
  )
    answer.ok(200, { items: images, limit: 200 });
  else if (name && match[2] === "reset" && method === "POST")
    reset(name, await read(), key, answer);
  else if (name && match[2] === "default" && method === "POST")
    makeDefault(name, await read(), key, answer);
  else if (name && match[2] === "check" && method === "POST") {
    const item = templates.get(name);
    if (!item) answer.refuse(404, "not_found");
    else {
      rechecked.add(name);
      templates.set(name, {
        ...item,
        check: {
          state: "unchecked",
          reason: null,
          checked_version: null,
          secret_files: null,
          scan_limit: null,
        },
      });
      answer.accepted();
    }
  } else if (name && !match[2] && method === "PUT")
    put(name, await read(), key, answer);
  else if (name && !match[2] && method === "DELETE")
    remove(name, url, key, answer);
  else return false;
  return true;
}

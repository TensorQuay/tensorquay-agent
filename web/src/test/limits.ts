import { ApiError, type FieldProblem } from "../api/error";
import type {
  Limit,
  LimitKey,
  LimitsApi,
  TeamSettings,
} from "../settings/admin/limits";

import { route } from "./routes";

const refuse = (status: number, code: string, details: FieldProblem[] = []) =>
  new ApiError(status, code, "fixture refusal", undefined, details);

export function limit(key: LimitKey, extra: Partial<Limit> = {}): Limit {
  return {
    key,
    value: 4,
    default: 4,
    min: 1,
    max: 64,
    unit: "sandboxes",
    applies: "next_admission",
    version: 1,
    seed: "current",
    last_set_by: "seed",
    updated_at: "2026-09-26T08:00:00Z",
    clamped_from: null,
    below_floor: false,
    ...extra,
  };
}

/** The limits as a typical install lists them (ADR-0028); environment sizes are per environment now. */
export const LIMITS: Limit[] = [
  // The file's max_live fell to 8, so at load the gateway moved the stored 12 to 8 and kept 12 in clamped_from.
  limit("admission.slots", {
    value: 8,
    default: 8,
    max: 8,
    clamped_from: 12,
  }),
  // The configuration file sets 1, below the floor of 2 that an administrator may set.
  limit("admission.per_user", {
    value: 1,
    default: 1,
    min: 2,
    below_floor: true,
  }),
  limit("sandbox.idle_seconds", {
    value: 900,
    default: 1200,
    min: 60,
    max: 86400,
    unit: "seconds",
    applies: "next_sandbox",
    last_set_by: "admin",
    version: 3,
  }),
  limit("sandbox.max_lifetime_seconds", {
    value: 14400,
    default: 14400,
    min: 600,
    max: 604800,
    unit: "seconds",
    applies: "next_sandbox",
    seed: "removed",
  }),
  limit("mcp.calls_per_minute", {
    value: 120,
    default: 60,
    min: 1,
    max: 6000,
    unit: "calls",
    applies: "next_call",
    last_set_by: "admin",
    version: 2,
  }),
];

/** The helper limits at the file's defaults (ADR-0034 S6): the gateway lists them only while agent teams are on. */
/** The Desktop's limits (ADR-0039 §9-10, GW-U-146) and the computer-use caps per run, as an install with the Desktop template lists them. */
export const DESKTOP_LIMITS: Limit[] = [
  // Its max is the slots, as the gateway lists it (admin_bounds: at most Tasks running at once, 8 here).
  limit("desktop.max_sessions", {
    value: 2,
    default: 2,
    min: 1,
    max: 8,
    unit: "sandboxes",
    applies: "next_admission",
  }),
  limit("desktop.screenshot_retention_days", {
    value: 30,
    default: 30,
    min: 1,
    max: 365,
    unit: "days",
    applies: "next_purge",
  }),
  limit("computer.max_screenshots_per_run", {
    value: 200,
    default: 200,
    min: 1,
    max: 1000,
    unit: "screenshots",
    applies: "next_session",
  }),
  limit("computer.max_actions_per_run", {
    value: 1000,
    default: 1000,
    min: 1,
    max: 5000,
    unit: "actions",
    applies: "next_session",
  }),
];

export const TEAM_LIMITS: Limit[] = [
  limit("agent_teams.max_members", {
    value: 3,
    default: 3,
    max: 8,
    unit: "helpers",
    applies: "next_member",
  }),
  limit("agent_teams.token_budget", {
    value: 2_000_000,
    default: 2_000_000,
    min: 10_000,
    max: 100_000_000,
    unit: "tokens",
    applies: "next_team",
  }),
  limit("agent_teams.deadline_minutes", {
    value: 120,
    default: 120,
    min: 5,
    max: 1440,
    unit: "minutes",
    applies: "next_team",
  }),
  ...(
    [
      ["agent_teams.member_idle_minutes", 10],
      ["agent_teams.member_stuck_minutes", 15],
      ["agent_teams.idle_stop_minutes", 30],
    ] as const
  ).map(([key, value]) =>
    limit(key, {
      value,
      default: value,
      max: 1440,
      unit: "minutes",
      applies: "next_check",
    }),
  ),
];

/**
 * An in-memory limits store with the gateway's semantics (slice 5): versions and `409 conflict`, bounds and the
 * per-user order and the helpers' (one place below the slots and the per-user cap) as 400 `details[]`, reset to the default, Idempotency-Key replay. `floors` lets a test make the
 * gateway stricter than the page's own check for one limit, as a newer gateway could be.
 */
/** What an environment without its own size gets, and the most any may have (the configuration file's). */
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

/** The routes the helpers' model is chosen from: one with a display name, one without, one deleted since it was listed. */
export const TEAM_ROUTES = [
  route("local", { display_name: "Local model" }),
  route("frontier"),
  route("retired", { display_name: "Retired model" }),
];

/**
 * `teams` is the helpers' settings the gateway stores (ADR-0034 S6); `null`, agent teams are off and it answers
 * `403 teams_disabled`. A helpers' model must name a route the gateway has (`422 unknown_reference`; `gone`, listed to
 * the page but deleted since); the acceptance check is one line of at most 4,096 bytes. The first version is 0.
 */
export function fakeLimits({
  limits = LIMITS,
  floors = {},
  teams = { member_route: null, acceptance_check: null, version: 0 },
  routes = TEAM_ROUTES,
  gone = ["retired"],
}: {
  limits?: Limit[];
  floors?: Partial<Record<LimitKey, number>>;
  teams?: TeamSettings | null;
  routes?: ReturnType<typeof route>[];
  gone?: string[];
} = {}) {
  const store = new Map(limits.map((item) => [item.key, { ...item }]));
  let settings = teams && { ...teams };
  const teamWrites: TeamSettings[] = [];
  const replies = new Map<string, Limit>();
  const writes: { key: LimitKey; value?: number; version: number }[] = [];
  const answer = (key: string, item: Limit) => {
    replies.set(key, item);
    return Promise.resolve({ ...item });
  };
  const api: LimitsApi = {
    limits: () =>
      Promise.resolve({
        items: [...store.values()],
        generation: 1,
        defaults: SIZE_DEFAULTS,
        ceiling: SIZE_CEILING,
      }),
    putLimit(key, value, version, idempotency) {
      writes.push({ key, value, version });
      const replay = replies.get(idempotency);
      if (replay) return Promise.resolve(replay);
      const existing = store.get(key);
      if (!existing) return Promise.reject(refuse(400, "invalid_request"));
      if (existing.version !== version)
        return Promise.reject(refuse(409, "conflict"));
      const out = (reason: string) =>
        Promise.reject(
          refuse(400, "invalid_request", [{ field: "value", reason }]),
        );
      if (
        value < Math.max(existing.min, floors[key] ?? 0) ||
        value > existing.max
      )
        return out("limit_out_of_range");
      const slots = store.get("admission.slots")?.value ?? Infinity;
      if (key === "admission.per_user" && value > slots)
        return out("limit_order");
      // ADR-0039 §10: the Desktop environments at once fit within the tasks running at once.
      if (key === "desktop.max_sessions" && value > slots)
        return out("limit_order");
      const perUser = store.get("admission.per_user")?.value ?? Infinity;
      if (
        key === "agent_teams.max_members" &&
        value > Math.min(slots, perUser) - 1
      )
        return out("limit_order");
      const item = {
        ...existing,
        value,
        version: existing.version + 1,
        last_set_by: "admin" as const,
        clamped_from: null,
        below_floor: false,
      };
      store.set(key, item);
      return answer(idempotency, item);
    },
    resetLimit(key, version, idempotency) {
      writes.push({ key, version });
      const replay = replies.get(idempotency);
      if (replay) return Promise.resolve(replay);
      const existing = store.get(key);
      if (!existing) return Promise.reject(refuse(400, "invalid_request"));
      if (existing.version !== version)
        return Promise.reject(refuse(409, "conflict"));
      const item = {
        ...existing,
        value: existing.default,
        version: existing.version + 1,
        last_set_by: "seed" as const,
        clamped_from: null,
      };
      store.set(key, item);
      return answer(idempotency, item);
    },
    agentTeams: () =>
      settings
        ? Promise.resolve({ ...settings })
        : Promise.reject(refuse(403, "teams_disabled")),
    putAgentTeams(write) {
      teamWrites.push(write);
      if (!settings) return Promise.reject(refuse(403, "teams_disabled"));
      if (write.version !== settings.version)
        return Promise.reject(refuse(409, "conflict"));
      const chosen = write.member_route;
      if (
        chosen !== null &&
        (gone.includes(chosen) || !routes.some((item) => item.name === chosen))
      )
        return Promise.reject(
          refuse(422, "unknown_reference", [
            { field: "member_route", reason: "unknown_reference" },
          ]),
        );
      const check = write.acceptance_check;
      // As the gateway: a line break or NUL is value_invalid, too long value_too_long.
      const broken = check !== null && /[\r\n\0]/.test(check);
      if (
        check !== null &&
        (broken || new TextEncoder().encode(check).length > 4096)
      )
        return Promise.reject(
          refuse(400, "invalid_request", [
            {
              field: "acceptance_check",
              reason: broken ? "value_invalid" : "value_too_long",
            },
          ]),
        );
      settings = { ...write, version: settings.version + 1 };
      return Promise.resolve({ ...settings });
    },
    routes: () => Promise.resolve({ items: routes, generation: 1 }),
  };
  /** Another administrator's change: the stored limit moves to the next version. */
  const touch = (key: LimitKey) => {
    const existing = store.get(key);
    if (existing)
      store.set(key, { ...existing, version: existing.version + 1 });
  };
  /** Another administrator's change to the helpers' settings. */
  const touchTeams = () => {
    if (settings) settings = { ...settings, version: settings.version + 1 };
  };
  return { api, writes, touch, teamWrites, touchTeams };
}

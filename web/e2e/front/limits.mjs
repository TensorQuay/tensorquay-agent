// The admin limits API on the fake front (slice 5, ADR-0028): five seeded limits (no parking, ADR-0035; sizes are each
// environment's) with versions, 409s, bounds and the per-user order as 400 details, reset to the default and Idempotency-Key replay, like
// src/test/limits.ts.
import { desktopOn } from "./screen.mjs";
import { seededMap } from "./state.mjs";
import { teamsOn } from "./teams.mjs";
import { SIZE_CEILING, SIZE_DEFAULTS } from "./templates.mjs";

const seeded = (key, fields) => ({
  key,
  min: 1,
  max: 64,
  unit: "sandboxes",
  applies: "next_admission",
  version: 1,
  seed: "current",
  last_set_by: "seed",
  updated_at: "2026-09-25T09:00:00Z",
  clamped_from: null,
  below_floor: false,
  ...fields,
  default: fields.default ?? fields.value,
});
const next = { applies: "next_sandbox" };
const limits = seededMap(
  [
    // The file's max_live fell to 8, so at load the gateway moved the stored 12 to 8 and kept 12 in clamped_from.
    seeded("admission.slots", { value: 8, max: 8, clamped_from: 12 }),
    seeded("admission.per_user", { value: 2 }),
    seeded("sandbox.idle_seconds", {
      ...next,
      value: 900,
      min: 60,
      max: 86400,
      unit: "seconds",
    }),
    seeded("sandbox.max_lifetime_seconds", {
      ...next,
      value: 14400,
      min: 600,
      max: 604800,
      unit: "seconds",
    }),
    // G5: each person's connector calls per minute, applied at the next proxied call.
    seeded("mcp.calls_per_minute", {
      value: 60,
      default: 60,
      min: 1,
      max: 6000,
      unit: "calls",
      applies: "next_call",
    }),
    // ADR-0034 S6: the helper limits, listed and written only while agent teams are on.
    seeded("agent_teams.max_members", {
      value: 3,
      max: 8,
      unit: "helpers",
      applies: "next_member",
    }),
    seeded("agent_teams.token_budget", {
      value: 2_000_000,
      min: 10_000,
      max: 100_000_000,
      unit: "tokens",
      applies: "next_team",
    }),
    seeded("agent_teams.deadline_minutes", {
      value: 120,
      min: 5,
      max: 1440,
      unit: "minutes",
      applies: "next_team",
    }),
    ...[
      ["agent_teams.member_idle_minutes", 10],
      ["agent_teams.member_stuck_minutes", 15],
      ["agent_teams.idle_stop_minutes", 30],
    ].map(([key, value]) =>
      seeded(key, { value, max: 1440, unit: "minutes", applies: "next_check" }),
    ),
    // The Desktop's two (ADR-0039 §9-10), listed only in the "desktop" scenario so the other specs' pages stay as they are.
    // Its max is the slots, as the gateway lists it (`admin_bounds`): the page refuses more in words before sending.
    seeded("desktop.max_sessions", { value: 2, max: 8 }),
    seeded("desktop.screenshot_retention_days", {
      value: 30,
      max: 365,
      unit: "days",
      applies: "next_purge",
    }),
    // The computer tool's caps per run (ADR-0039 §4, GW-U-152), in the Desktop's section (WEB-U-280).
    seeded("computer.max_screenshots_per_run", {
      value: 200,
      default: 200,
      max: 1000,
      unit: "screenshots",
      applies: "next_session",
    }),
    seeded("computer.max_actions_per_run", {
      value: 1000,
      default: 1000,
      max: 5000,
      unit: "actions",
      applies: "next_session",
    }),
  ].map((item) => [item.key, item]),
);
const shown = (key) =>
  (teamsOn() || !key.startsWith("agent_teams.")) &&
  (desktopOn() || !(key.startsWith("desktop.") || key.startsWith("computer.")));
const replies = seededMap();

function put(key, input, idempotency, answer) {
  if (replies.has(idempotency)) return answer.ok(200, replies.get(idempotency));
  const item = shown(key) ? limits.get(key) : undefined;
  if (!item)
    return answer.refuse(400, "invalid_request", [
      { field: "key", reason: "limit_unknown" },
    ]);
  if (input.version !== item.version) return answer.refuse(409, "conflict");
  const value = input.value;
  if (!Number.isSafeInteger(value) || value < item.min || value > item.max)
    return answer.refuse(400, "invalid_request", [
      { field: "value", reason: "limit_out_of_range" },
    ]);
  if (
    key === "admission.per_user" &&
    value > limits.get("admission.slots").value
  )
    return answer.refuse(400, "invalid_request", [
      { field: "value", reason: "limit_order" },
    ]);
  // A lead task and its helpers must fit: helpers one below the slots and the per-user cap.
  const room = Math.min(
    limits.get("admission.slots").value,
    limits.get("admission.per_user").value,
  );
  if (
    key === "desktop.max_sessions" &&
    value > limits.get("admission.slots").value
  )
    return answer.refuse(400, "invalid_request", [
      { field: "value", reason: "limit_order" },
    ]);
  if (key === "agent_teams.max_members" && value > room - 1)
    return answer.refuse(400, "invalid_request", [
      { field: "value", reason: "limit_order" },
    ]);
  const stored = {
    ...item,
    value,
    version: item.version + 1,
    last_set_by: "admin",
    updated_at: new Date().toISOString(),
    clamped_from: null,
    below_floor: false,
  };
  limits.set(key, stored);
  replies.set(idempotency, stored);
  answer.ok(200, stored);
}

function reset(key, url, idempotency, answer) {
  if (replies.has(idempotency)) return answer.ok(200, replies.get(idempotency));
  const item = shown(key) ? limits.get(key) : undefined;
  if (!item)
    return answer.refuse(400, "invalid_request", [
      { field: "key", reason: "limit_unknown" },
    ]);
  if (String(item.version) !== url.searchParams.get("version"))
    return answer.refuse(409, "conflict");
  const stored = {
    ...item,
    value: item.default,
    version: item.version + 1,
    last_set_by: "seed",
    updated_at: new Date().toISOString(),
    clamped_from: null,
  };
  limits.set(key, stored);
  replies.set(idempotency, stored);
  answer.ok(200, stored);
}

/** Answers /api/v1/admin/limits*; returns false for any other path. */
export async function limitsRoute(url, method, read, idempotency, answer) {
  const key = /^\/api\/v1\/admin\/limits\/([^/]+)$/.exec(url.pathname)?.[1];
  if (url.pathname === "/api/v1/admin/limits" && method === "GET")
    answer.ok(200, {
      items: [...limits.values()].filter((item) => shown(item.key)),
      generation: 1,
      defaults: SIZE_DEFAULTS,
      ceiling: SIZE_CEILING,
    });
  else if (key && method === "PUT")
    put(decodeURIComponent(key), await read(), idempotency, answer);
  else if (key && method === "DELETE")
    reset(decodeURIComponent(key), url, idempotency, answer);
  else return false;
  return true;
}

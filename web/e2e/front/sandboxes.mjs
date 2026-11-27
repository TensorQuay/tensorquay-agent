// The Sandboxes page's gateway operations for the fake front (the Sandboxes page design note, §2): the listing of the
// running sandboxes with their owners, states, sizes and measured use, Stop (202, or a 409 for a working one), and the
// readiness detail's leak verdict. A seeded store: each test starts from the four sandboxes below. The scenario cookie
// fixture_sandboxes=empty answers no sandbox; =amber adds a network the sweeps left behind; =long gives the other
// owners long addresses.
import { seededMap } from "./state.mjs";

const GIB = 2 ** 30;
/** The signed-in administrator (fixtures.mjs's `user`) owns the kept-on Desktop, so its row reads "You". */
const ADMIN = {
  id: "0195d8c0-0000-7000-8000-000000000002",
  email: "member@example.invalid",
};

const seed = [
  {
    conversation_id: "0195d8c0-0000-7000-8000-00000000c001",
    owner: {
      id: "0195d8c0-0000-7000-8000-0000000000b1",
      email: "ben@example.org",
    },
    environment: {
      name: "workstation",
      display_name: "Workstation",
      desktop: false,
    },
    state: "working",
    started_at: "2026-11-10T02:12:00Z",
    last_activity_at: "2026-11-10T08:58:00Z",
    stops_at: null,
    kept_on: false,
    size: { cpu_millicores: 2000, memory_bytes: 4 * GIB, pids: 512 },
    usage: {
      cpu_millicores: 1400,
      memory_bytes: Math.round(3.1 * GIB),
      pids: 40,
    },
    can_stop: false,
    stop_refusal: "working",
    screen_held: false,
  },
  {
    conversation_id: "0195d8c0-0000-7000-8000-00000000c002",
    owner: {
      id: "0195d8c0-0000-7000-8000-0000000000b2",
      email: "cho@example.org",
    },
    environment: { name: "minimal", display_name: "Minimal", desktop: false },
    state: "starting",
    started_at: "2026-11-10T08:59:30Z",
    last_activity_at: "2026-11-10T08:59:30Z",
    stops_at: null,
    kept_on: false,
    size: { cpu_millicores: 1000, memory_bytes: 2 * GIB, pids: 512 },
    usage: null,
    can_stop: false,
    stop_refusal: "starting",
    screen_held: false,
  },
  {
    conversation_id: "0195d8c0-0000-7000-8000-00000000c003",
    owner: {
      id: "0195d8c0-0000-7000-8000-0000000000b1",
      email: "ben@example.org",
    },
    environment: { name: "minimal", display_name: "Minimal", desktop: false },
    state: "idle",
    started_at: "2026-11-10T07:40:00Z",
    last_activity_at: "2026-11-10T08:52:00Z",
    stops_at: "2026-11-10T09:02:00Z",
    kept_on: false,
    size: { cpu_millicores: 1000, memory_bytes: 2 * GIB, pids: 512 },
    usage: { cpu_millicores: 20, memory_bytes: Math.round(0.4 * GIB), pids: 7 },
    can_stop: true,
    stop_refusal: null,
    screen_held: false,
  },
  {
    conversation_id: "0195d8c0-0000-7000-8000-00000000c004",
    owner: { ...ADMIN },
    environment: { name: "desktop", display_name: "Desktop", desktop: true },
    state: "idle",
    started_at: "2026-11-10T01:00:00Z",
    last_activity_at: "2026-11-10T08:30:00Z",
    stops_at: "2026-11-10T09:00:00Z",
    kept_on: true,
    size: { cpu_millicores: 4000, memory_bytes: 8 * GIB, pids: 2048 },
    usage: {
      cpu_millicores: 150,
      memory_bytes: Math.round(1.9 * GIB),
      pids: 60,
    },
    can_stop: true,
    stop_refusal: null,
    screen_held: false,
  },
];
const rows = seededMap(seed.map((row) => [row.conversation_id, row]));
/** Stops asked for by key: a repeat answers the same. */
const stops = seededMap();

const LEFT_BEHIND = {
  kind: "network",
  name: "tqa-sbx-18e7c62210a67d70",
  age_seconds: 660,
  reason: "timeout",
  remove: "docker network rm tqa-sbx-18e7c62210a67d70",
};

/** Ordinary long addresses (=long): the owner column must wrap them between their parts, never run under the state. */
const LONG_EMAILS = [
  "member@example.invalid",
  "operations.team@tensorquay-example.org",
];

function scenario(request) {
  const cookie = request?.headers?.cookie ?? "";
  return /(?:^|;\s*)fixture_sandboxes=([a-z]+)(?:;|$)/.exec(cookie)?.[1] ?? "";
}

function listing(request) {
  const which = scenario(request);
  const items =
    which === "empty"
      ? []
      : which === "long"
        ? [...rows.values()].map((row, index) =>
            row.owner.id === ADMIN.id
              ? row
              : {
                  ...row,
                  owner: {
                    ...row.owner,
                    email: LONG_EMAILS[index % LONG_EMAILS.length],
                  },
                },
          )
        : [...rows.values()];
  const measured = items.filter((row) => row.usage);
  const desktops = items.filter((row) => row.environment.desktop).length;
  return {
    backend: "docker",
    limits: {
      slots: 6,
      per_user: 4,
      desktops: 2,
      idle_seconds: 600,
      max_lifetime_seconds: 28_800,
    },
    totals: {
      in_use: items.length,
      starting: items.filter((row) => row.state === "starting").length,
      working: items.filter((row) => row.state === "working").length,
      idle: items.filter((row) => row.state === "idle").length,
      stopping: items.filter((row) => row.state === "stopping").length,
      desktops,
      waiting: 0,
      reserved: {
        cpu_millicores: items.reduce(
          (sum, row) => sum + row.size.cpu_millicores,
          0,
        ),
        memory_bytes: items.reduce(
          (sum, row) => sum + row.size.memory_bytes,
          0,
        ),
      },
      usage:
        measured.length > 0
          ? {
              cpu_millicores: measured.reduce(
                (sum, row) => sum + row.usage.cpu_millicores,
                0,
              ),
              memory_bytes: measured.reduce(
                (sum, row) => sum + row.usage.memory_bytes,
                0,
              ),
              measured_at: new Date().toISOString(),
            }
          : null,
    },
    items,
    next_after: null,
    left_behind: which === "amber" ? [LEFT_BEHIND] : [],
  };
}

/** The readiness detail's `sandboxes` object, in step with the listing. */
export function sandboxesReadiness(request) {
  const which = scenario(request);
  const items = which === "empty" ? [] : [...rows.values()];
  const oldest = items.reduce(
    (max, row) =>
      Math.max(max, (Date.now() - Date.parse(row.started_at)) / 1000),
    0,
  );
  const amber = which === "amber";
  return {
    running: items.length,
    oldest_seconds: Math.round(oldest),
    unowned: { containers: 0, networks: amber ? 1 : 0 },
    left_behind: {
      containers: 0,
      networks: amber ? 1 : 0,
      oldest_seconds: amber ? 660 : null,
      reason: amber ? "timeout" : null,
    },
    swept_at: "2026-11-10T08:55:00Z",
  };
}

/** Answers the Sandboxes page's routes; false for any other. */
export async function sandboxesRoute(url, method, key, request, answer) {
  if (url.pathname === "/api/v1/admin/sandboxes" && method === "GET") {
    answer.ok(200, listing(request));
    return true;
  }
  const stop = /^\/api\/v1\/admin\/sandboxes\/([^/]+)\/stop$/.exec(
    url.pathname,
  )?.[1];
  if (!stop || method !== "POST") return false;
  if (!key) {
    answer.refuse(400, "invalid_request", [
      { field: "idempotency-key", reason: "key_required" },
    ]);
    return true;
  }
  if (stops.has(key)) {
    answer.ok(202, { stopping: true });
    return true;
  }
  const row = rows.get(stop);
  if (!row) {
    answer.refuse(404, "not_found");
    return true;
  }
  if (row.stop_refusal === "work_waiting") {
    answer.refuse(409, "work_waiting");
    return true;
  }
  if (row.stop_refusal === "working" || row.stop_refusal === "starting") {
    answer.refuse(409, "working");
    return true;
  }
  stops.set(key, stop);
  // The gateway retires it in the background: it reads as stopping, then goes.
  rows.set(stop, {
    ...row,
    state: "stopping",
    can_stop: false,
    stop_refusal: "stopping",
    screen_held: false,
    stops_at: null,
  });
  setTimeout(() => rows.delete(stop), 1500);
  answer.ok(202, { stopping: true });
  return true;
}

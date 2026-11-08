import { ApiError, type FieldProblem } from "../api/error";
import type { SecretConsumer, SecretReadPage } from "../protocol/generated/api";
import type {
  AdminApi,
  SecretItem,
  SecretWrite,
  UsageReport,
} from "../settings/admin/contract";

const NAME = /^[A-Za-z0-9_-]{1,120}$/;
const VALUE_BYTES = 8192;

const refuse = (status: number, code: string, details: FieldProblem[] = []) =>
  new ApiError(status, code, "fixture refusal", undefined, details);

/** The gateway's checks for a secret write, with the closed reason list. */
function check(write: SecretWrite) {
  const problems: FieldProblem[] = [];
  if (!NAME.test(write.name))
    problems.push({ field: "name", reason: "name_invalid" });
  else if (/^(git|connector)-/i.test(write.name))
    problems.push({ field: "name", reason: "reserved_prefix" });
  if (write.value.length === 0)
    problems.push({ field: "value", reason: "value_empty" });
  else if (new TextEncoder().encode(write.value).length > VALUE_BYTES)
    problems.push({ field: "value", reason: "value_too_long" });
  else if (write.value.includes("\0"))
    problems.push({ field: "value", reason: "value_nul" });
  if (problems.length) throw refuse(400, "invalid_request", problems);
}

/** A listed shared secret with the gateway's fields, the rotation reminder's included (never a value). */
export function secretItem(
  name: string,
  extra: Partial<SecretItem> = {},
): SecretItem {
  return {
    ref: `secrets/${name}`,
    version: 1,
    updated_at: "2026-09-25T09:00:00Z",
    seed: null,
    last_set_by: "admin",
    routes: [],
    rotated_at: "2026-09-25T09:00:00Z",
    rotate_after_days: null,
    rotation_due: false,
    consumers: [],
    ...extra,
  };
}

const DAY_MS = 86_400_000;
/** Whether a reminder is due at the fixture's clock, as the gateway computes it. */
function due(item: SecretItem, now: string) {
  const days = item.rotate_after_days;
  if (!days || !item.rotated_at) return false;
  return Date.parse(now) - Date.parse(item.rotated_at) >= days * DAY_MS;
}

type State = {
  store: Map<string, SecretItem>;
  replies: Map<string, SecretItem | null>;
  limit: number;
  session: boolean;
  now: () => string;
};

/** Creates or replaces with the contract's version rules; the reply is kept for an Idempotency-Key replay. */
function put(state: State, write: SecretWrite, key: string): SecretItem {
  const replay = state.replies.get(key);
  if (replay) return replay;
  if (!state.session) throw refuse(403, "session_required");
  check(write);
  const ref = `secrets/${write.name}`;
  const existing = state.store.get(ref);
  if (existing && write.version !== existing.version)
    throw refuse(409, "conflict");
  if (!existing && write.version !== undefined) throw refuse(404, "not_found");
  if (!existing && state.store.size >= state.limit)
    throw refuse(409, "secret_limit");
  const now = state.now();
  const item: SecretItem = {
    ref,
    version: (existing?.version ?? 0) + 1,
    updated_at: now,
    // Only a new value moves rotated_at; the reminder stays as it was.
    rotated_at: now,
    rotate_after_days: existing?.rotate_after_days ?? null,
    rotation_due: false,
    seed: existing?.seed ?? null,
    last_set_by: "admin",
    routes: existing?.routes ?? [],
    consumers: existing?.consumers ?? [],
  };
  state.store.set(ref, item);
  state.replies.set(key, item);
  return { ...item };
}

function remove(state: State, name: string, version: number, key: string) {
  if (state.replies.has(key)) return;
  if (!state.session) throw refuse(403, "session_required");
  const ref = `secrets/${name}`;
  const existing = state.store.get(ref);
  if (!existing) throw refuse(404, "not_found");
  if (existing.version !== version) throw refuse(409, "conflict");
  if (existing.routes.length) throw refuse(409, "secret_in_use");
  state.store.delete(ref);
  state.replies.set(key, null);
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

/**
 * An in-memory administration API with the semantics of the 090 contract: versions and 409 conflict, secret_in_use,
 * secret_limit, session_required, 400 details with the closed reason list, and Idempotency-Key replay.
 */
export function fakeAdmin({
  secrets = [],
  limit = 4096,
  session = true,
  usage,
  refuseReminder = false,
  reads = {},
  people = {},
  choices = [],
}: {
  secrets?: SecretItem[];
  limit?: number;
  /** False acts as an admin API token: secret writes are refused with 403 session_required. */
  session?: boolean;
  usage?: UsageReport;
  /** The gateway refuses every reminder's days (400 rotate_after_invalid). */
  refuseReminder?: boolean;
  /** Each secret's reads, one page per name; a later page is the same page with `next` cleared. */
  reads?: Record<string, SecretReadPage>;
  /** The people an administrator can see, by id. */
  people?: Record<string, string>;
  /** The routes and connectors a secret can be allowed for. */
  choices?: SecretConsumer[];
} = {}) {
  let clock = Date.parse("2026-09-26T10:00:00Z");
  const state: State = {
    store: new Map(secrets.map((item) => [item.ref, { ...item }])),
    replies: new Map(),
    limit,
    session,
    now: () => new Date((clock += 1000)).toISOString(),
  };
  const writes: { name: string; version?: number; key: string }[] = [];
  const consumerWrites: {
    name: string;
    consumers: SecretConsumer[];
    version: number;
  }[] = [];
  let consumersRefused = false;
  const readCalls: { name: string; before: string | undefined }[] = [];
  const reminders: {
    name: string;
    rotate_after_days: number | null;
    version: number;
  }[] = [];
  const api: AdminApi = {
    usage: () =>
      settle(
        () =>
          usage ??
          (() => {
            throw refuse(503, "store_unavailable");
          })(),
      ),
    secrets: () =>
      settle(() => ({
        items: [...state.store.values()]
          .sort((a, b) => a.ref.localeCompare(b.ref))
          .map((item) => ({ ...item, routes: [...item.routes] })),
        limit,
      })),
    putSecret(write, key) {
      writes.push({
        name: write.name,
        ...(write.version === undefined ? {} : { version: write.version }),
        key,
      });
      return settle(() => put(state, write, key));
    },
    deleteSecret(name, version, key) {
      writes.push({ name, version, key });
      return settle(() => remove(state, name, version, key));
    },
    secretReads(name, before) {
      readCalls.push({ name, before });
      return settle(() => {
        const page = reads[name] ?? { items: [], next: null };
        return before ? { ...page, next: null } : page;
      });
    },
    people: () => settle(() => ({ ...people })),
    consumerChoices: () => settle(() => [...choices]),
    setConsumers(name, consumers, version) {
      consumerWrites.push({ name, consumers, version });
      return settle(() => {
        const ref = `secrets/${name}`;
        const existing = state.store.get(ref);
        if (!existing) throw refuse(404, "not_found");
        if (existing.version !== version) throw refuse(409, "conflict");
        if (consumersRefused)
          throw refuse(400, "invalid_request", [
            { field: "consumers", reason: "consumers_invalid" },
          ]);
        const item: SecretItem = {
          ...existing,
          version: existing.version + 1,
          updated_at: state.now(),
          // Sorted as the gateway answers: by kind, then name.
          consumers: [...consumers].sort((a, b) =>
            `${a.kind}/${a.name}`.localeCompare(`${b.kind}/${b.name}`),
          ),
        };
        state.store.set(ref, item);
        return { ...item };
      });
    },
    setReminder(name, rotate_after_days, version) {
      reminders.push({ name, rotate_after_days, version });
      return settle(() => {
        if (!state.session) throw refuse(403, "session_required");
        if (
          refuseReminder ||
          (rotate_after_days !== null &&
            !(
              Number.isInteger(rotate_after_days) &&
              rotate_after_days >= 1 &&
              rotate_after_days <= 3650
            ))
        )
          throw refuse(400, "invalid_request", [
            { field: "rotate_after_days", reason: "rotate_after_invalid" },
          ]);
        const ref = `secrets/${name}`;
        const existing = state.store.get(ref);
        if (!existing) throw refuse(404, "not_found");
        if (existing.version !== version) throw refuse(409, "conflict");
        const item: SecretItem = {
          ...existing,
          version: existing.version + 1,
          updated_at: state.now(),
          rotate_after_days,
        };
        item.rotation_due = due(item, "2026-09-26T10:00:00Z");
        state.store.set(ref, item);
        return { ...item };
      });
    },
  };
  return {
    api,
    writes,
    reminders,
    readCalls,
    consumerWrites,
    /** The gateway refuses every list from now on (400 consumers_invalid). */
    refuseConsumers() {
      consumersRefused = true;
    },
    /** Another administrator's change, to provoke a stale version. */
    touch(ref: string) {
      const item = state.store.get(ref);
      if (item)
        state.store.set(ref, {
          ...item,
          version: item.version + 1,
          updated_at: state.now(),
        });
    },
  };
}

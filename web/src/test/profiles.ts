import { ApiError, type FieldProblem } from "../api/error";
import type { Profile, ProfileFields, ProfilesApi } from "../settings/profiles";

const refuse = (status: number, code: string, details: FieldProblem[] = []) =>
  new ApiError(status, code, "fixture refusal", undefined, details);

export function profile(name: string, extra: Partial<Profile> = {}): Profile {
  return {
    id: `0195d8c0-0000-7000-8000-${name
      .padEnd(12, "0")
      .slice(0, 12)
      .replace(/[^0-9a-f]/g, "0")}`,
    name,
    display_name: name[0]?.toUpperCase() + name.slice(1),
    shared: false,
    version: 1,
    updated_at: "2026-09-27T09:00:00Z",
    usable: true,
    skills: "all",
    mcp_servers: [],
    connectors: [],
    limits: {},
    ...extra,
  };
}

/**
 * The gateway's semantics (ADR-0012): the caller's own profiles, then the shared ones; members write only their own
 * (403 on a shared one); versions, name_taken, Idempotency-Key replay, and 422 unknown_reference for names the
 * install lacks.
 */
export function fakeProfiles({
  profiles = [],
  admin = false,
  routes = ["local"],
  templates = ["workstation", "minimal"],
  connectors = ["tracker"],
}: {
  profiles?: Profile[];
  admin?: boolean;
  routes?: string[];
  templates?: string[];
  connectors?: string[];
} = {}) {
  const store = new Map(profiles.map((item) => [item.id, { ...item }]));
  const replies = new Map<string, Profile | null>();
  const writes: { id?: string; fields: ProfileFields; version?: number }[] = [];
  let next = 0;
  const settle = <T>(work: () => T): Promise<T> => {
    try {
      return Promise.resolve(work());
    } catch (error) {
      return Promise.reject(
        error instanceof Error ? error : new Error("fixture failure"),
      );
    }
  };
  const references = (fields: ProfileFields) => {
    const missing: FieldProblem[] = [];
    if (fields.model_route && !routes.includes(fields.model_route))
      missing.push({ field: "model_route", reason: "unknown_reference" });
    if (fields.template && !templates.includes(fields.template))
      missing.push({ field: "template", reason: "unknown_reference" });
    if ((fields.connectors ?? []).some((name) => !connectors.includes(name)))
      missing.push({ field: "connectors", reason: "unknown_reference" });
    if (missing.length) throw refuse(422, "unknown_reference", missing);
  };
  // The gateway refuses the removed project-link switch by name, whatever its value (400 field repositories, removed).
  const removed = (fields: ProfileFields) => {
    if ("repositories" in fields)
      throw refuse(400, "invalid_request", [
        { field: "repositories", reason: "removed" },
      ]);
  };
  const taken = (fields: ProfileFields, shared: boolean, except?: string) =>
    [...store.values()].some(
      (item) =>
        item.id !== except &&
        item.name === fields.name &&
        item.shared === shared,
    );
  const api: ProfilesApi = {
    profiles: () =>
      settle(() => ({
        items: [...store.values()].sort(
          (a, b) =>
            Number(a.shared) - Number(b.shared) || a.name.localeCompare(b.name),
        ),
      })),
    createProfile: (fields, key) =>
      settle(() => {
        writes.push({ fields });
        const replay = replies.get(key);
        if (replay) return replay;
        if (fields.shared && !admin) throw refuse(403, "forbidden");
        removed(fields);
        references(fields);
        if (taken(fields, Boolean(fields.shared)))
          throw refuse(409, "name_taken");
        next += 1;
        const item = profile(fields.name, {
          ...fields,
          id: `0195d8c0-0000-7000-8000-${String(900000000000 + next)}`,
          shared: Boolean(fields.shared),
        });
        store.set(item.id, item);
        replies.set(key, item);
        return { ...item };
      }),
    putProfile: (id, fields, version, key) =>
      settle(() => {
        writes.push({ id, fields, version });
        const replay = replies.get(key);
        if (replay) return replay;
        const existing = store.get(id);
        if (!existing) throw refuse(404, "not_found");
        if (existing.shared && !admin) throw refuse(403, "forbidden");
        removed(fields);
        if (existing.version !== version) throw refuse(409, "conflict");
        references(fields);
        if (taken(fields, existing.shared, id)) throw refuse(409, "name_taken");
        const item: Profile = {
          ...existing,
          ...(fields as Partial<Profile>),
          shared: existing.shared,
          version: existing.version + 1,
          usable: true,
        };
        delete item.unusable_field;
        store.set(id, item);
        replies.set(key, item);
        return { ...item };
      }),
    deleteProfile: (id, version, key) =>
      settle(() => {
        if (replies.has(key)) return;
        const existing = store.get(id);
        if (!existing) throw refuse(404, "not_found");
        if (existing.shared && !admin) throw refuse(403, "forbidden");
        if (existing.version !== version) throw refuse(409, "conflict");
        store.delete(id);
        replies.set(key, null);
      }),
  };
  /** Another tab changes a profile, so the next write with the old version conflicts. */
  const touch = (id: string) => {
    const existing = store.get(id);
    if (existing) store.set(id, { ...existing, version: existing.version + 1 });
  };
  return { api, writes, touch };
}

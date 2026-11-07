import { vi } from "vitest";

import { ApiError } from "../api/error";
import type { SecretRead } from "../protocol/generated/api";
import { parseAddress } from "../settings/host-keys/address";
import {
  type AdminHostKeysApi,
  type HostKey,
  type PersonalKeyRow,
  type HostKeysApi,
  type HostKeyUpdate,
  type NewHostKey,
} from "../settings/host-keys/contract";

let next = 0;

/** A key as the gateway would list it: metadata only. */
export function hostKey(overrides: Partial<HostKey> = {}): HostKey {
  next++;
  return {
    id: `0195d8c0-0000-7000-8000-${String(next).padStart(12, "0")}`,
    name: "GITHUB_WORK",
    host: "github.com",
    port: 443,
    path_prefix: "/acme",
    scheme: "bearer",
    user_name: null,
    header_name: null,
    shared: false,
    users: null,
    set_at: "2026-09-12T10:00:00Z",
    last_use: null,
    allowed: true,
    check_name: false,
    version: 1,
    ...overrides,
  };
}

/**
 * An in-memory host-keys API: the keys it starts with, the shared names, and every added key's body (so a test can
 * see that the value went once and never came back). `missing` answers 404, as a gateway without host keys.
 */
export function fakeHostKeys({
  keys = [],
  shared = [],
  missing = false,
  refuse,
  readRows = [],
}: {
  keys?: HostKey[];
  shared?: HostKey[];
  missing?: boolean;
  /** A code to refuse the next add with, as the gateway would (409 name_taken, 503 store_unavailable, …). */
  refuse?: { status: number; code: string };
  /** The key's uses, as its reads page answers them. */
  readRows?: SecretRead[];
} = {}) {
  const items = [...keys];
  const added: NewHostKey[] = [];
  const keysCall = vi.fn(() =>
    missing
      ? Promise.reject(new ApiError(404, "not_found", "Not found."))
      : Promise.resolve({ items: [...items] }),
  );
  const sharedCall = vi.fn(() => Promise.resolve({ items: [...shared] }));
  const addKey = vi.fn((key: NewHostKey) => {
    added.push(key);
    if (refuse)
      return Promise.reject(
        new ApiError(refuse.status, refuse.code, "Refused."),
      );
    const parsed = parseAddress(key.address);
    if (!parsed.ok)
      return Promise.reject(new Error("the fake takes a valid address"));
    const made = hostKey({
      name: key.name,
      ...parsed.address,
      scheme: key.scheme,
      user_name: key.user_name ?? null,
      header_name: key.header_name ?? null,
      set_at: "2026-09-30T20:00:00Z",
    });
    items.push(made);
    return Promise.resolve(made);
  });
  const updated: HostKeyUpdate[] = [];
  const updateKey = vi.fn((id: string, key: HostKeyUpdate) => {
    updated.push(key);
    if (refuse)
      return Promise.reject(
        new ApiError(refuse.status, refuse.code, "Refused."),
      );
    const at = items.findIndex((item) => item.id === id);
    const current = items[at];
    if (!current)
      return Promise.reject(new ApiError(404, "not_found", "Not found."));
    if (current.version !== key.version)
      return Promise.reject(new ApiError(409, "conflict", "Changed."));
    const parsed = parseAddress(key.address);
    if (!parsed.ok)
      return Promise.reject(new Error("the fake takes a valid address"));
    const next: HostKey = {
      ...current,
      name: key.name,
      ...parsed.address,
      scheme: key.scheme,
      user_name: key.user_name ?? null,
      header_name: key.header_name ?? null,
      ...(key.value === undefined ? {} : { set_at: "2026-09-30T20:00:00Z" }),
      check_name: false,
      version: current.version + 1,
    };
    items[at] = next;
    return Promise.resolve(next);
  });
  const deleteKey = vi.fn((id: string, version: number) => {
    const at = items.findIndex((item) => item.id === id);
    const current = items[at];
    if (!current)
      return Promise.reject(new ApiError(404, "not_found", "Not found."));
    if (current.version !== version)
      return Promise.reject(new ApiError(409, "conflict", "Changed."));
    items.splice(at, 1);
    return Promise.resolve();
  });
  const reads = vi.fn(() => Promise.resolve({ items: readRows, next: null }));
  const api: HostKeysApi = {
    keys: keysCall,
    shared: sharedCall,
    addKey,
    updateKey,
    deleteKey,
    reads,
  };
  // The mocks apart from the API, to assert on without unbinding its methods.
  return {
    api,
    added,
    updated,
    items,
    calls: {
      keys: keysCall,
      shared: sharedCall,
      addKey,
      updateKey,
      deleteKey,
      reads,
    },
  };
}

/** A member's own key row, told apart from a shared key by its owner. */
const isPersonal = (item: HostKey | PersonalKeyRow): item is PersonalKeyRow =>
  "owner" in item;

/** A member's own key as an administrator's list shows it. */
export function personalRow(
  overrides: Partial<PersonalKeyRow> = {},
): PersonalKeyRow {
  const key = hostKey();
  return {
    id: key.id,
    host: key.host,
    port: key.port,
    path_prefix: null,
    set_at: key.set_at,
    last_use: null,
    version: 1,
    owner: { id: "u-ana", email: "ana@acme.test" },
    ...overrides,
  };
}

/**
 * An in-memory administrators' host-keys API: shared keys (with `users`) and members' own keys as rows, the
 * installation's people, and every body sent. `missing` answers 404, as a gateway without host keys.
 */
export function fakeAdminHostKeys({
  items = [],
  people = { "u-ana": "ana@acme.test", "u-sam": "sam@acme.test" },
  missing = false,
  refuse,
}: {
  items?: (HostKey | PersonalKeyRow)[];
  people?: Record<string, string>;
  missing?: boolean;
  /** Refuses every add or change of a shared key with this, as the gateway would. */
  refuse?: Error;
} = {}) {
  const list = [...items];
  const sent: (NewHostKey | HostKeyUpdate)[] = [];
  const refs = (users: NewHostKey["users"]) =>
    users === undefined || users === "all"
      ? "all"
      : users.map((id) => ({ id, email: people[id] ?? id }));
  const keys = vi.fn(() =>
    missing
      ? Promise.reject(new ApiError(404, "not_found", "Not found."))
      : Promise.resolve({
          items: list.filter((item): item is HostKey => !isPersonal(item)),
          personal: list.filter(isPersonal),
        }),
  );
  const addShared = vi.fn((key: NewHostKey) => {
    sent.push(key);
    if (refuse) return Promise.reject(refuse);
    const parsed = parseAddress(key.address);
    if (!parsed.ok)
      return Promise.reject(new Error("the fake takes a valid address"));
    const made = hostKey({
      name: key.name,
      ...parsed.address,
      scheme: key.scheme,
      user_name: key.user_name ?? null,
      shared: true,
      users: refs(key.users),
    });
    list.push(made);
    return Promise.resolve(made);
  });
  const updateShared = vi.fn((id: string, key: HostKeyUpdate) => {
    sent.push(key);
    if (refuse) return Promise.reject(refuse);
    const at = list.findIndex((item) => item.id === id);
    const current = list[at];
    if (!current || isPersonal(current))
      return Promise.reject(new ApiError(404, "not_found", "Not found."));
    const parsed = parseAddress(key.address);
    if (!parsed.ok)
      return Promise.reject(new Error("the fake takes a valid address"));
    const next: HostKey = {
      ...current,
      name: key.name,
      ...parsed.address,
      scheme: key.scheme,
      users: refs(key.users),
      version: current.version + 1,
    };
    list[at] = next;
    return Promise.resolve(next);
  });
  const deleteKey = vi.fn((id: string, version: number) => {
    const at = list.findIndex((item) => item.id === id);
    const current = list[at];
    if (!current)
      return Promise.reject(new ApiError(404, "not_found", "Not found."));
    if (current.version !== version)
      return Promise.reject(new ApiError(409, "conflict", "Changed."));
    list.splice(at, 1);
    return Promise.resolve();
  });
  const reads = vi.fn(() => Promise.resolve({ items: [], next: null }));
  const peopleCall = vi.fn(() => Promise.resolve({ ...people }));
  const api: AdminHostKeysApi = {
    keys,
    addShared,
    updateShared,
    deleteKey,
    reads,
    people: peopleCall,
  };
  return {
    api,
    sent,
    list,
    calls: {
      keys,
      addShared,
      updateShared,
      deleteKey,
      reads,
      people: peopleCall,
    },
  };
}

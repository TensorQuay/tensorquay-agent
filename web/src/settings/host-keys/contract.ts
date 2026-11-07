/**
 * Per-service keys (ADR-0037) as the web reads them: the gateway's own types (`protocol/generated/api`), and the
 * `HostKeysApi` boundary the pages depend on. Every answer is metadata: a value never comes back.
 */

import type {
  AdminHostKeyAdd,
  AdminHostKeyPut,
  AdminHostKeys,
  Error as ApiErrorBody,
  HostKey,
  HostKeyAdd,
  HostKeyPut,
  PersonalHostKey,
  SecretReadPage,
} from "../../protocol/generated/api";

export type {
  HostKey,
  HostKeyUse,
  UserRef,
} from "../../protocol/generated/api";

/** How the service expects the key (ADR-0037 §5). */
export type HostKeyScheme = HostKey["scheme"];

/** A shared key the person may use (their names are taken too); the gateway answers it as a `HostKey`. */
export type SharedKeyName = Pick<
  HostKey,
  "id" | "name" | "host" | "port" | "path_prefix"
>;

/**
 * A new key as sent: the address as the person typed it (the gateway parses and answers host, port and path). A shared
 * key adds its people (administrators only): everyone, or these user ids.
 */
export type NewHostKey = HostKeyAdd & Partial<Pick<AdminHostKeyAdd, "users">>;

/**
 * A key as changed (`PUT`): the whole item with the version it was read at; `value` only when replacing it. Any
 * successful save clears `check_name`.
 */
export type HostKeyUpdate = HostKeyPut &
  Partial<Pick<AdminHostKeyPut, "users">>;

/** What a person's Keys pages need; the gateway's binding comes with the agreed API, tests pass a fake. */
export type HostKeysApi = {
  /** The person's own keys. A gateway without host keys answers 404, and the pages leave them out. */
  keys(signal?: AbortSignal): Promise<{ items: HostKey[] }>;
  /** The shared keys the person may use (read-only for them; their names are taken too), with their own last use. */
  shared(signal?: AbortSignal): Promise<{ items: HostKey[] }>;
  /** Adds a key; the value is sent once and never returned. */
  addKey(key: NewHostKey): Promise<HostKey>;
  /** Changes a key; a stale version is `409 conflict`, a key gone is `404`. */
  updateKey(id: string, key: HostKeyUpdate): Promise<HostKey>;
  deleteKey(id: string, version: number): Promise<void>;
  /** The key's uses, newest first, 50 a page: today's reads page (`consumer_kind: "host"`). */
  reads(
    id: string,
    before?: string,
    signal?: AbortSignal,
  ): Promise<SecretReadPage>;
};

/** The gateway's refusals of an added key: a `400` detail's reason by field, or a `409`/`503` code. */
export type HostKeyFieldReason = Extract<
  Extract<
    NonNullable<ApiErrorBody["error"]["details"]>,
    unknown[]
  >[number]["reason"],
  | "address_not_https"
  | "address_not_plain"
  | "host_invalid"
  | "name_invalid"
  | "name_reserved"
  | "name_taken"
  /** A shared key's name that someone who may use it has for an own key (GW-I-170); whose is never said. */
  | "name_taken_by_user"
  | "user_name_required"
  | "header_name_required"
  | "scheme_not_supported"
  | "value_empty"
  | "value_too_long"
  | "value_nul"
  | "value_invalid"
>;

/** A member's own key as an administrator sees it: that it exists and when it was last used, to delete, never to edit. */
export type PersonalKeyRow = PersonalHostKey;

/** What the administrators' Shared tab needs (the gateway plan's §1.2 admin routes); tests pass a fake. */
export type AdminHostKeysApi = {
  /** Every shared key with its `users`, and the members' own keys apart. 404 on a gateway without host keys. */
  keys(signal?: AbortSignal): Promise<AdminHostKeys>;
  addShared(key: NewHostKey): Promise<HostKey>;
  updateShared(id: string, key: HostKeyUpdate): Promise<HostKey>;
  /** A shared key, or a member's own key (an administrator may only delete it). */
  deleteKey(id: string, version: number): Promise<void>;
  reads(
    id: string,
    before?: string,
    signal?: AbortSignal,
  ): Promise<SecretReadPage>;
  /** The installation's people, by id, for Who may use. */
  people(signal?: AbortSignal): Promise<Record<string, string>>;
};

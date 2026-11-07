/**
 * The model routes contract (ADR-0028): the gateway's generated types. The Models page depends on the
 * `RoutesApi` interface; the gateway client implements it and tests pass an in-memory fake.
 */

import type {
  AdminRoute,
  AdminRoutes,
  RouteModels,
  RouteProbeResult,
  RouteReasoning as GatewayReasoning,
} from "../../protocol/generated/api";

export type { RouteAuth, RouteModels } from "../../protocol/generated/api";
export type RouteReasoning = NonNullable<GatewayReasoning>;
export type RouteProtocol = AdminRoute["protocol"];
export type CacheHint = AdminRoute["cache_hint"];
export type RouteItem = AdminRoute;
export type RouteList = AdminRoutes;
export type RouteProbe = AdminRoute["probe"];
/** One route check's observation (099b-2): passing or not, it is a result, never an error. */
export type ProbeResult = RouteProbeResult["probe"];

/** Every editable route field: a listed route without the gateway's own fields (`AdminRouteWrite` less `version`). */
export type RouteFields = Omit<
  AdminRoute,
  | "name"
  | "key_missing"
  | "key_ref_invalid"
  | "version"
  | "seed"
  | "last_set_by"
  | "updated_at"
  | "fallback_of"
  | "probe"
>;

export type RoutesApi = {
  routes(signal?: AbortSignal): Promise<RouteList>;
  /** Creates (no version) or replaces (the version the page read) one route. */
  putRoute(
    name: string,
    fields: RouteFields,
    version: number | undefined,
    key: string,
  ): Promise<RouteItem>;
  deleteRoute(name: string, version: number, key: string): Promise<void>;
  /** The shared secrets' refs, to pick a key from. */
  secretRefs(signal?: AbortSignal): Promise<string[]>;
  /** Stores a pasted key as a new shared secret (never a replace: an existing name is a 409) and answers its ref. */
  createSecret(name: string, value: string, key: string): Promise<string>;
  /** Adds this route to who may use a secret, on the secret's current version (vault slice 3). */
  allowSecret(ref: string, route: string): Promise<void>;
  /** A link to download every route as models.toml. */
  exportUrl(): string;
  /** Checks the stored route as a task would use it: at most 60 s, or 2 minutes for a local model (GW-U-092). A failed
   * check is a result, not an error. */
  probe(name: string, key: string): Promise<ProbeResult>;
  /**
   * The models the stored route's server offers, with its stored address and key (WEB-U-067); at most 500. The
   * server's own refusal is a result (`failure`), not an error.
   */
  models(name: string, signal?: AbortSignal): Promise<RouteModels>;
};

/** A new route's starting values: the defaults models.toml applies when a field is omitted. */
export const NEW_ROUTE: RouteFields = {
  display_name: null,
  description: null,
  protocol: "openai_chat",
  base_url: "",
  model: "",
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
  stream_usage: true,
  extra_headers: {},
  extra_body: {},
  quirks: [],
  strict: "auto",
  schema_keywords: "full",
  fallbacks: [],
};

/** The editable fields of a listed route. */
export function fieldsOf(item: RouteItem): RouteFields {
  const fields = Object.fromEntries(
    Object.keys(NEW_ROUTE).map((key) => [key, item[key as keyof RouteFields]]),
  );
  return fields as RouteFields;
}

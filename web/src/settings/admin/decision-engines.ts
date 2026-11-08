/**
 * The decision engine contract (ADR-0013 point 6): the gateway's generated types. The Decision engines page depends on
 * the `EnginesApi` interface; the gateway client implements it and tests pass an in-memory fake.
 */
import type {
  AdminDecisionEngine,
  AdminDecisionEngines,
  AdminDecisionEngineWrite,
} from "../../protocol/generated/api";

export type Engine = AdminDecisionEngine;
export type EngineList = AdminDecisionEngines;
/** What an administrator sets; the version travels separately (none creates, the version read replaces). */
export type EngineFields = Omit<AdminDecisionEngineWrite, "version">;
export type Exposure = NonNullable<Engine["exposure"]>;
export type EngineScheme = NonNullable<Engine["auth"]["scheme"]>;

/** Bounds from `AdminDecisionEngineWrite` and `AdminDecisionEngines` (openapi.json). */
export const MAX_ENGINES = 16;
export const MAX_ENGINE_NAME = 64;
export const MAX_HEADER = 64;
export const ENGINE_TIMEOUT = { min: 1, max: 10000, default: 2000 } as const;

export type EnginesApi = {
  engines(signal?: AbortSignal): Promise<EngineList>;
  /** Creates (no version) or replaces (the version the page read) one engine. */
  putEngine(
    name: string,
    fields: EngineFields,
    version: number | undefined,
    key: string,
  ): Promise<Engine>;
  deleteEngine(name: string, version: number, key: string): Promise<void>;
  /** The shared secrets' refs, offered as an engine's key. */
  secretRefs(signal?: AbortSignal): Promise<string[]>;
  /** Adds this engine to who may use a secret, on the secret's current version. */
  allowSecret(ref: string, engine: string): Promise<void>;
  /** The installation's model routes, by name, in their order: a multiple-choice service chooses among them. */
  routeNames(signal?: AbortSignal): Promise<string[]>;
};

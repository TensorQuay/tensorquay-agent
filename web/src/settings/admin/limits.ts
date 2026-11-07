/**
 * The admin limits contract (slice 5, ADR-0028): the gateway's generated types. The Limits page depends on the
 * `LimitsApi` interface; the gateway client implements it and tests pass an in-memory fake.
 */
import type {
  AdminLimit,
  AdminLimits,
  AgentTeamSettings,
  AgentTeamSettingsWrite,
} from "../../protocol/generated/api";

import type { RouteList } from "./routes";

export type Limit = AdminLimit;
export type LimitList = AdminLimits;
export type LimitKey = AdminLimit["key"];
export type LimitUnit = AdminLimit["unit"];
/** The helpers' two settings that aren't numbers (ADR-0034 S6): the helpers' model route and the acceptance check. */
export type TeamSettings = AgentTeamSettings;
export type TeamSettingsWrite = AgentTeamSettingsWrite;

export type LimitsApi = {
  limits(signal?: AbortSignal): Promise<LimitList>;
  /** Sets a limit to a value with the version the page read; limits always exist. */
  putLimit(
    key: LimitKey,
    value: number,
    version: number,
    idempotency: string,
  ): Promise<Limit>;
  /** Resets a limit to the configuration file's value (or its built-in default). */
  resetLimit(
    key: LimitKey,
    version: number,
    idempotency: string,
  ): Promise<Limit>;
  /** The helpers' settings; the gateway answers them only while agent teams are on. */
  agentTeams(signal?: AbortSignal): Promise<TeamSettings>;
  /** Writes both settings with the version the page read. */
  putAgentTeams(
    write: TeamSettingsWrite,
    idempotency: string,
  ): Promise<TeamSettings>;
  /** The model routes, for the helpers' model choice. */
  routes(signal?: AbortSignal): Promise<RouteList>;
};

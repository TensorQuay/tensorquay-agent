import type { ApiClient } from "../../api/client";
import type {
  AdminConnectorWrite,
  AdminDecisionEngineWrite,
  AdminTemplateWrite,
  AdminEgressRuleWrite,
  AdminLimitWrite,
  AdminRouteWrite,
  AdminSecretConsumers,
  AdminSecretReminder,
  AdminSecretWrite,
  AgentTeamSettingsWrite,
  SecretConsumer,
} from "../../protocol/generated/api";
import { parseApi } from "../../protocol/validate";
import { parseAdminApi } from "../../protocol/validate-admin";
import { settingsEndpoint } from "../operations";
import { readsQuery } from "../reads";

import type { ConnectorFields } from "./connectors";
import type { SecretWrite, UsageQuery } from "./contract";
import type { EngineFields } from "./decision-engines";
import type { LimitKey } from "./limits";
import type { RouteFields } from "./routes";
import type { EnvironmentFields } from "./templates";
import "../../protocol/generated/validators.settings";

/**
 * The administration requests on one client (ADR-0028). They live with the admin pages, not in the shared
 * client, so the administration schema loads only there (WEB-U-014).
 */
/**
 * Waits for the answers the gateway bounds itself for longer than the web's usual 10 s (the c13c how-to check,
 * WEB-U-138). A route check is bounded per route by the gateway: a hosted model's by a minute, a local model's by its
 * progress within an outer cap of 10 minutes (a first-hour check before 17.0), plus a margin; so one ceiling past that
 * is never the one that ends it. Listing a route's models and reading the container engine (its images, Reset to
 * built-in) are bounded at 30 s there.
 */
const CHECK_TIMEOUT_MS = 660_000;
const LISTING_TIMEOUT_MS = 45_000;

export function adminRequests(client: ApiClient) {
  return {
    ...secretRequests(client),
    ...routeRequests(client),
    ...egressRequests(client),
    ...limitRequests(client),
    ...connectorRequests(client),
    ...engineRequests(client),
    ...templateRequests(client),
  };
}

/** Usage and shared secrets. */
function secretRequests(client: ApiClient) {
  return {
    /** Token usage for administrators over UTC days, grouped by day, person or model. */
    usage: (query: UsageQuery, signal?: AbortSignal) =>
      client.request(
        settingsEndpoint("adminUsage"),
        (value) => parseAdminApi(value, "AdminUsageReport"),
        { query: new URLSearchParams(query), signal },
      ),
    /** Shared secrets by name; never a value. */
    secrets: (signal?: AbortSignal) =>
      client.request(
        settingsEndpoint("adminSecrets"),
        (value) => parseAdminApi(value, "AdminSecrets"),
        { signal },
      ),
    /** Creates (no version) or replaces (the version read) a secret; the value is sent once and never returned. */
    putSecret: (write: SecretWrite, key: string) => {
      const body: AdminSecretWrite =
        write.version === undefined
          ? { value: write.value }
          : { value: write.value, version: write.version };
      return client.request(
        settingsEndpoint("adminSecretPut"),
        (value) => parseAdminApi(value, "AdminSecret"),
        { parameters: { name: write.name }, body: JSON.stringify(body), key },
      );
    },
    secretReads: (name: string, before?: string, signal?: AbortSignal) =>
      client.request(
        settingsEndpoint("adminSecretReads"),
        (value) => parseApi(value, "SecretReadPage"),
        { parameters: { name }, query: readsQuery(before), signal },
      ),
    setConsumers: (
      name: string,
      consumers: SecretConsumer[],
      version: number,
    ) => {
      const body: AdminSecretConsumers = { consumers, version };
      return client.request(
        settingsEndpoint("adminSecretConsumers"),
        (value) => parseAdminApi(value, "AdminSecret"),
        { parameters: { name }, body: JSON.stringify(body) },
      );
    },
    setReminder: (name: string, days: number | null, version: number) => {
      const body: AdminSecretReminder = { rotate_after_days: days, version };
      return client.request(
        settingsEndpoint("adminSecretReminder"),
        (value) => parseAdminApi(value, "AdminSecret"),
        { parameters: { name }, body: JSON.stringify(body) },
      );
    },
    deleteSecret: (name: string, version: number, key: string) =>
      client.request(settingsEndpoint("adminSecretDelete"), () => undefined, {
        parameters: { name },
        query: new URLSearchParams({ version: String(version) }),
        key,
      }),
  };
}

/** Model routes (slice 3). */
function routeRequests(client: ApiClient) {
  return {
    /** Checks one stored route as a task would use it; a failed check is still a result. */
    /** The saved route's upstream models (WEB-U-067); the server's refusal is in `failure`. */
    routeModels: (name: string, signal?: AbortSignal) =>
      client.request(
        settingsEndpoint("adminRouteModels"),
        (value) => parseAdminApi(value, "RouteModels"),
        { parameters: { name }, signal, timeoutMs: LISTING_TIMEOUT_MS },
      ),
    probeRoute: async (name: string, key: string) =>
      (
        await client.request(
          settingsEndpoint("adminRouteProbe"),
          (value) => parseAdminApi(value, "RouteProbeResult"),
          { parameters: { name }, key, timeoutMs: CHECK_TIMEOUT_MS },
        )
      ).probe,
    /** Every model route with its source, key state and the routes that fall back to it. */
    routes: (signal?: AbortSignal) =>
      client.request(
        settingsEndpoint("adminRoutes"),
        (value) => parseAdminApi(value, "AdminRoutes"),
        { signal },
      ),
    /** Creates (no version) or replaces (the version read) one route. */
    putRoute: (
      name: string,
      fields: RouteFields,
      version: number | undefined,
      key: string,
    ) => {
      const body: AdminRouteWrite =
        version === undefined ? fields : { ...fields, version };
      return client.request(
        settingsEndpoint("adminRoutePut"),
        (value) => parseAdminApi(value, "AdminRoute"),
        { parameters: { name }, body: JSON.stringify(body), key },
      );
    },
    deleteRoute: (name: string, version: number, key: string) =>
      client.request(settingsEndpoint("adminRouteDelete"), () => undefined, {
        parameters: { name },
        query: new URLSearchParams({ version: String(version) }),
        key,
      }),
  };
}

/** The egress allow-list (slice 4). */
function egressRequests(client: ApiClient) {
  return {
    /** The egress allow-list, sorted by host; an empty list denies every destination. */
    egress: (signal?: AbortSignal) =>
      client.request(
        settingsEndpoint("adminEgress"),
        (value) => parseAdminApi(value, "AdminEgressRules"),
        { signal },
      ),
    /** Creates (no version) or replaces (the version read) the rule for one host. */
    putEgress: (
      host: string,
      { ports, internal }: { ports: number[]; internal: boolean },
      version: number | undefined,
      key: string,
    ) => {
      const body: AdminEgressRuleWrite =
        version === undefined
          ? { ports, internal }
          : { ports, internal, version };
      return client.request(
        settingsEndpoint("adminEgressPut"),
        (value) => parseAdminApi(value, "AdminEgressRule"),
        { parameters: { host }, body: JSON.stringify(body), key },
      );
    },
    deleteEgress: (host: string, version: number, key: string) =>
      client.request(settingsEndpoint("adminEgressDelete"), () => undefined, {
        parameters: { host },
        query: new URLSearchParams({ version: String(version) }),
        key,
      }),
    /** The ready-made lists and whether each is in the allow-list. */
    egressPresets: (signal?: AbortSignal) =>
      client.request(
        settingsEndpoint("adminEgressPresets"),
        (value) => parseAdminApi(value, "AdminEgressPresets"),
        { signal },
      ),
    applyEgressPreset: (id: string, key: string) =>
      client.request(
        settingsEndpoint("adminEgressPresetApply"),
        (value) => parseAdminApi(value, "AdminEgressPresetApplied"),
        { parameters: { id }, key },
      ),
    removeEgressPreset: (id: string, key: string) =>
      client.request(
        settingsEndpoint("adminEgressPresetRemove"),
        (value) => parseAdminApi(value, "AdminEgressPresetRemoved"),
        { parameters: { id }, key },
      ),
  };
}

/** Admission and sandbox limits (slice 5). */
function limitRequests(client: ApiClient) {
  return {
    /** Every overridable limit with its default, bounds and when a change applies. */
    limits: (signal?: AbortSignal) =>
      client.request(
        settingsEndpoint("adminLimits"),
        (value) => parseAdminApi(value, "AdminLimits"),
        { signal },
      ),
    /** Sets a limit with the version read; limits always exist. */
    putLimit: (
      key: LimitKey,
      value: number,
      version: number,
      idempotency: string,
    ) => {
      const body: AdminLimitWrite = { value, version };
      return client.request(
        settingsEndpoint("adminLimitPut"),
        (answer) => parseAdminApi(answer, "AdminLimit"),
        { parameters: { key }, body: JSON.stringify(body), key: idempotency },
      );
    },
    /** Resets a limit to the configuration file's value; the answer is the limit. */
    resetLimit: (key: LimitKey, version: number, idempotency: string) =>
      client.request(
        settingsEndpoint("adminLimitDelete"),
        (answer) => parseAdminApi(answer, "AdminLimit"),
        {
          parameters: { key },
          query: new URLSearchParams({ version: String(version) }),
          key: idempotency,
        },
      ),
    /** The helpers' two settings that aren't numbers (ADR-0034 S6); `403 teams_disabled` while agent teams are off. */
    agentTeams: (signal?: AbortSignal) =>
      client.request(
        settingsEndpoint("adminAgentTeams"),
        (value) => parseAdminApi(value, "AgentTeamSettings"),
        { signal },
      ),
    /** Writes both with the version read. */
    putAgentTeams: (write: AgentTeamSettingsWrite, idempotency: string) =>
      client.request(
        settingsEndpoint("adminAgentTeamsPut"),
        (answer) => parseAdminApi(answer, "AgentTeamSettings"),
        { body: JSON.stringify(write), key: idempotency },
      ),
  };
}

/** Decision engines (ADR-0013 point 6): the key is named by secret ref, never sent or returned. */
function engineRequests(client: ApiClient) {
  return {
    engines: (signal?: AbortSignal) =>
      client.request(
        settingsEndpoint("adminDecisionEngines"),
        (value) => parseAdminApi(value, "AdminDecisionEngines"),
        { signal },
      ),
    /** Creates (no version) or replaces (the version read) one engine. */
    putEngine: (
      name: string,
      fields: EngineFields,
      version: number | undefined,
      key: string,
    ) => {
      const body: AdminDecisionEngineWrite =
        version === undefined ? fields : { ...fields, version };
      return client.request(
        settingsEndpoint("adminDecisionEnginePut"),
        (value) => parseAdminApi(value, "AdminDecisionEngine"),
        { parameters: { name }, body: JSON.stringify(body), key },
      );
    },
    deleteEngine: (name: string, version: number, key: string) =>
      client.request(
        settingsEndpoint("adminDecisionEngineDelete"),
        () => undefined,
        {
          parameters: { name },
          query: new URLSearchParams({ version: String(version) }),
          key,
        },
      ),
  };
}

/** The connector registry (ADR-0031): credentials are named by secret ref, never sent or returned. */
function connectorRequests(client: ApiClient) {
  return {
    connectors: (signal?: AbortSignal) =>
      client.request(
        settingsEndpoint("adminConnectors"),
        (value) => parseAdminApi(value, "AdminConnectors"),
        { signal },
      ),
    /** Creates (no version) or replaces (the version read) one connector. */
    putConnector: (
      name: string,
      fields: ConnectorFields,
      version: number | undefined,
      key: string,
    ) => {
      const body: AdminConnectorWrite =
        version === undefined ? fields : { ...fields, version };
      return client.request(
        settingsEndpoint("adminConnectorPut"),
        (value) => parseAdminApi(value, "AdminConnector"),
        { parameters: { name }, body: JSON.stringify(body), key },
      );
    },
    deleteConnector: (name: string, version: number, key: string) =>
      client.request(
        settingsEndpoint("adminConnectorDelete"),
        () => undefined,
        {
          parameters: { name },
          query: new URLSearchParams({ version: String(version) }),
          key,
        },
      ),
  };
}

/** The environment (template) registry (slice 4, ADR-0032): checks run on the gateway, results come in the list. */
function templateRequests(client: ApiClient) {
  return {
    templates: (signal?: AbortSignal) =>
      client.request(
        settingsEndpoint("adminTemplates"),
        (value) => parseAdminApi(value, "AdminTemplates"),
        { signal },
      ),
    /** Creates (no version) or replaces (the version read) one environment. */
    putTemplate: (
      name: string,
      fields: EnvironmentFields,
      version: number | undefined,
      key: string,
    ) => {
      const body: AdminTemplateWrite =
        version === undefined ? fields : { ...fields, version };
      return client.request(
        settingsEndpoint("adminTemplatePut"),
        (value) => parseAdminApi(value, "AdminTemplate"),
        { parameters: { name }, body: JSON.stringify(body), key },
      );
    },
    deleteTemplate: (name: string, version: number, key: string) =>
      client.request(settingsEndpoint("adminTemplateDelete"), () => undefined, {
        parameters: { name },
        query: new URLSearchParams({ version: String(version) }),
        key,
      }),
    defaultTemplate: (name: string, version: number, key: string) =>
      client.request(
        settingsEndpoint("adminTemplateDefault"),
        (value) => parseAdminApi(value, "AdminTemplate"),
        { parameters: { name }, body: JSON.stringify({ version }), key },
      ),
    checkTemplate: (name: string) =>
      client.request(settingsEndpoint("adminTemplateCheck"), () => undefined, {
        parameters: { name },
      }),
    resetTemplate: (name: string, version: number, key: string) =>
      client.request(
        settingsEndpoint("adminTemplateReset"),
        (value) => parseAdminApi(value, "AdminTemplate"),
        {
          parameters: { name },
          body: JSON.stringify({ version }),
          key,
          timeoutMs: LISTING_TIMEOUT_MS,
        },
      ),
    environmentImages: (signal?: AbortSignal) =>
      client.request(
        settingsEndpoint("adminEnvironmentImages"),
        (value) => parseAdminApi(value, "EnvironmentImages"),
        { signal, timeoutMs: LISTING_TIMEOUT_MS },
      ),
  };
}

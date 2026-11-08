import { api, type ApiClient } from "../../api/client";
import { ApiError } from "../../api/error";
import { pathFor } from "../../api/operations";
import type { SecretConsumer } from "../../protocol/generated/api";
import { settingsEndpoint } from "../operations";

import { adminRequests } from "./client";
import type { ConnectorsApi } from "./connectors";
import type { AdminApi } from "./contract";
import type { EnginesApi } from "./decision-engines";
import type { EgressApi } from "./egress";
import type { LimitsApi } from "./limits";
import type { RoutesApi } from "./routes";
import type { TemplatesApi } from "./templates";

/** Adds one route, connector or engine to who may use a secret, on the secret's current version. */
async function allowFor(
  on: ReturnType<typeof adminRequests>,
  ref: string,
  consumer: SecretConsumer,
) {
  const item = (await on.secrets()).items.find((entry) => entry.ref === ref);
  if (!item) throw new ApiError(404, "not_found", "The secret is gone."); // copy-lint: not user copy
  await on.setConsumers(
    ref.slice("secrets/".length),
    [...(item.consumers ?? []), consumer],
    item.version,
  );
}

/** The usage and secrets pages' API on one client. */
export function gatewayAdminFor(client: ApiClient): AdminApi {
  const on = adminRequests(client);
  return {
    usage: on.usage,
    secrets: on.secrets,
    putSecret: on.putSecret,
    deleteSecret: on.deleteSecret,
    setReminder: on.setReminder,
    setConsumers: on.setConsumers,
    consumerChoices: async (signal) => [
      ...(await on.connectors(signal)).items.map((item): SecretConsumer => ({
        kind: "connector",
        name: item.name,
      })),
      ...(await on.routes(signal)).items.map((item): SecretConsumer => ({
        kind: "route",
        name: item.name,
      })),
      ...(await on.engines(signal)).items.map((item): SecretConsumer => ({
        kind: "engine",
        name: item.name,
      })),
    ],
    secretReads: on.secretReads,
    people: async (signal) =>
      Object.fromEntries(
        (await client.users(signal)).items.map((account) => [
          account.id,
          account.email,
        ]),
      ),
  };
}

/** The Models page's API on one client; the secrets it offers as keys are the shared secrets' refs. */
export function gatewayRoutesFor(client: ApiClient): RoutesApi {
  const on = adminRequests(client);
  return {
    routes: on.routes,
    putRoute: on.putRoute,
    deleteRoute: on.deleteRoute,
    secretRefs: async (signal) =>
      (await on.secrets(signal)).items.map((item) => item.ref),
    createSecret: async (name, value, key) =>
      (await on.putSecret({ name, value }, key)).ref,
    allowSecret: (ref, name) => allowFor(on, ref, { kind: "route", name }),
    exportUrl: () => pathFor(settingsEndpoint("adminRoutesExport")),
    probe: on.probeRoute,
    models: on.routeModels,
  };
}

/** The Egress page's API on one client. */
export function gatewayEgressFor(client: ApiClient): EgressApi {
  const on = adminRequests(client);
  return {
    rules: on.egress,
    putRule: on.putEgress,
    deleteRule: on.deleteEgress,
    presets: on.egressPresets,
    applyPreset: on.applyEgressPreset,
    removePreset: on.removeEgressPreset,
  };
}

/** The Limits page's API on one client. */
export function gatewayLimitsFor(client: ApiClient): LimitsApi {
  const on = adminRequests(client);
  return {
    limits: on.limits,
    putLimit: on.putLimit,
    resetLimit: on.resetLimit,
    agentTeams: on.agentTeams,
    putAgentTeams: on.putAgentTeams,
    routes: on.routes,
  };
}

/** The Connectors page's API on one client; the shared secrets are offered as a connector's credential. */
export function gatewayConnectorsFor(client: ApiClient): ConnectorsApi {
  const on = adminRequests(client);
  return {
    connectors: on.connectors,
    putConnector: on.putConnector,
    deleteConnector: on.deleteConnector,
    secretRefs: async (signal) =>
      (await on.secrets(signal)).items.map((item) => item.ref),
    allowSecret: (ref, name) => allowFor(on, ref, { kind: "connector", name }),
  };
}

/** The Decision engines page's API on one client; the shared secrets are offered as an engine's key. */
export function gatewayEnginesFor(client: ApiClient): EnginesApi {
  const on = adminRequests(client);
  return {
    engines: on.engines,
    putEngine: on.putEngine,
    deleteEngine: on.deleteEngine,
    secretRefs: async (signal) =>
      (await on.secrets(signal)).items.map((item) => item.ref),
    allowSecret: (ref, name) => allowFor(on, ref, { kind: "engine", name }),
    routeNames: async (signal) =>
      (await on.routes(signal)).items.map((item) => item.name),
  };
}

/** The Environments page's API on one client. */
export function gatewayTemplatesFor(client: ApiClient): TemplatesApi {
  const on = adminRequests(client);
  return {
    environments: on.templates,
    putEnvironment: on.putTemplate,
    deleteEnvironment: on.deleteTemplate,
    makeDefault: on.defaultTemplate,
    check: on.checkTemplate,
    images: on.environmentImages,
    reset: on.resetTemplate,
  };
}

/** The administration pages' APIs: the gateway client. Tests pass the fakes in src/test instead. */
export const gatewayAdmin: AdminApi = gatewayAdminFor(api);
export const gatewayRoutes: RoutesApi = gatewayRoutesFor(api);
export const gatewayEgress: EgressApi = gatewayEgressFor(api);
export const gatewayLimits: LimitsApi = gatewayLimitsFor(api);
export const gatewayConnectors: ConnectorsApi = gatewayConnectorsFor(api);
export const gatewayTemplates: TemplatesApi = gatewayTemplatesFor(api);
export const gatewayEngines: EnginesApi = gatewayEnginesFor(api);

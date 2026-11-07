import { api } from "../api/client";

import { gatewayCredentials } from "./connector-credentials";
import type { ChoicesApi } from "./profile-queries";
import { enginesFor, toolServersFor } from "./profiles";

/** What a profile can narrow to, from the gateway: the list and each profile's page ask the same (WEB-U-104). */
export const gatewayProfileChoices: ChoicesApi = {
  models: (signal?: AbortSignal) => api.models(signal),
  templates: (signal?: AbortSignal) => api.templates(signal),
  connectors: (signal?: AbortSignal) => gatewayCredentials.connectors(signal),
  servers: toolServersFor(api),
  engines: enginesFor(api),
};

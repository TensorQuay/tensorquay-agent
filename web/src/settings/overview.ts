import { api } from "../api/client";
import type { AdminRoute } from "../protocol/generated/api";

import { gatewayAdmin, gatewayEgress, gatewayRoutes } from "./admin/gateway";
import { settingsApi } from "./requests";

/** What the settings overview reads (WEB-U-077): each from an endpoint that already exists, loaded on its own. */
export type OverviewApi = {
  readiness(
    signal?: AbortSignal,
  ): Promise<{ ready: boolean; checks: Record<string, boolean> }>;
  routes(signal?: AbortSignal): Promise<AdminRoute[]>;
  /** Active accounts, and invitations still waiting for their person. */
  people(signal?: AbortSignal): Promise<{ active: number; invited: number }>;
  /** Whether internet access is turned off in the gateway's configuration, and how many sites are allowed. */
  internetAccess(
    signal?: AbortSignal,
  ): Promise<{ disabled: boolean; rules: number }>;
  /** Shared keys whose rotation reminder is due. */
  rotationDue(signal?: AbortSignal): Promise<number>;
  /** Whether the install offers an environment with a desktop (ADR-0039): built with its image, or not at all. */
  desktop(signal?: AbortSignal): Promise<boolean>;
};

export const gatewayOverview: OverviewApi = {
  readiness: (signal) => settingsApi.readiness(signal),
  routes: async (signal) => (await gatewayRoutes.routes(signal)).items,
  people: async (signal) => {
    const [accounts, invitations] = await Promise.all([
      api.users(signal),
      settingsApi.invitations(signal),
    ]);
    return {
      active: accounts.items.filter((account) => !account.disabled).length,
      invited: invitations.items.filter((item) => item.status === "pending")
        .length,
    };
  },
  internetAccess: async (signal) => {
    const list = await gatewayEgress.rules(signal);
    return { disabled: list.disabled, rules: list.items.length };
  },
  rotationDue: async (signal) =>
    (await gatewayAdmin.secrets(signal)).items.filter(
      (secret) => secret.rotation_due,
    ).length,
  // The environments New task offers, as the suggestion card reads them: one that declares a desktop.
  desktop: async (signal) =>
    (await api.templates(signal)).items.some((template) =>
      template.capabilities.includes("desktop"),
    ),
};

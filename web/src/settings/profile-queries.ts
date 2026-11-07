"use client";

import { useQuery } from "@tanstack/react-query";

import type {
  McpServers,
  Models,
  Templates,
  User,
  UserConnectors,
} from "../protocol/generated/api";

import type { EngineChoice, ProfilesApi } from "./profiles";
import type { ProfileChoices } from "./ProfileTools";

/** The profiles list's query, shared by the list and each profile's page. */
export const PROFILES_KEY = ["profiles"];
/** What the list says when a page sends the reader back to it ("Saved agent profile …"). */
export const PROFILE_NOTICE_KEY = ["profiles-notice"];

/** Where the form's choices come from: what this person's tasks can use (models, offered environments, connectors). */
export type ChoicesApi = {
  models(signal?: AbortSignal): Promise<Models>;
  templates(signal?: AbortSignal): Promise<Templates>;
  connectors(signal?: AbortSignal): Promise<UserConnectors>;
  /** The administrator's tool servers, asked for administrators only: a person's own profile sets none (WEB-U-059). */
  servers?(signal?: AbortSignal): Promise<McpServers>;
  /** The decision engines' names, asked for administrators only (ADR-0013 point 6). */
  engines?(signal?: AbortSignal): Promise<EngineChoice[]>;
};

/**
 * The profiles and what a profile can narrow to, for the list and each profile's page (WEB-U-104). Tool servers and
 * engines are asked for administrators only, and without holding the page: a failure leaves a shared profile its own.
 */
export function useProfiles(me: User, api: ProfilesApi, choices: ChoicesApi) {
  const admin = me.role === "admin";
  const list = useQuery({
    queryKey: PROFILES_KEY,
    queryFn: ({ signal }) => api.profiles(signal),
    retry: false,
  });
  const options = useQuery({
    queryKey: ["profile-choices"],
    queryFn: async ({ signal }): Promise<ProfileChoices> => {
      const [models, templates, connectors] = await Promise.all([
        choices.models(signal),
        choices.templates(signal),
        choices.connectors(signal),
      ]);
      return {
        routes: models.items.map((model) => ({
          name: model.name,
          label: model.display_name ?? model.name,
        })),
        templates: templates.items.map((template) => ({
          name: template.name,
          label: template.display_name,
        })),
        connectors: connectors.items.map((connector) => ({
          name: connector.name,
          label: connector.display_name ?? connector.name,
        })),
      };
    },
    retry: false,
  });
  const servers = useQuery({
    queryKey: ["profile-tool-servers"],
    queryFn: ({ signal }) => choices.servers?.(signal) ?? { items: [] },
    enabled: admin && choices.servers !== undefined,
    retry: false,
  });
  const engines = useQuery({
    queryKey: ["profile-engines"],
    queryFn: ({ signal }) => choices.engines?.(signal) ?? [],
    enabled: admin && choices.engines !== undefined,
    retry: false,
  });
  /** Everything a profile's page can offer, once the choices are known. */
  const offered = (): ProfileChoices | undefined =>
    options.data && {
      ...withServers(options.data, servers.isError ? "failed" : servers.data),
      ...(engines.isError
        ? { engines: "failed" as const }
        : engines.data
          ? { engines: engines.data }
          : {}),
    };
  return { admin, list, options, offered };
}

/** The profile choices plus the administrator's tool servers once known, their environments by display name. */
function withServers(
  choices: ProfileChoices,
  servers: McpServers | "failed" | undefined,
): ProfileChoices {
  if (servers === "failed") return { ...choices, servers };
  if (!servers) return choices;
  const label = (name: string) =>
    choices.templates.find((template) => template.name === name)?.label ?? name;
  return {
    ...choices,
    servers: servers.items.map((server) => ({
      name: server.name,
      templates: server.templates.map(label),
      byDefault: server.enabled_by_default,
    })),
  };
}

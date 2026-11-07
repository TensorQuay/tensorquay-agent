/**
 * Agent profiles (ADR-0012): a person's own, and the shared ones administrators write. The Profiles page depends on
 * `ProfilesApi`; the gateway client implements it and tests pass an in-memory fake.
 */
import { api, type ApiClient } from "../api/client";
import { profiles } from "../api/profiles";
import type {
  McpServers,
  Profile,
  Profiles,
  ProfileWrite,
} from "../protocol/generated/api";
import { parseApi } from "../protocol/validate";
import { parseAdminApi } from "../protocol/validate-admin";

import { settingsEndpoint } from "./operations";

import "../protocol/generated/validators.settings";

export type { Profile };
/** What a person sets; the version travels separately (none creates, the version read replaces). */
export type ProfileFields = Omit<ProfileWrite, "version">;
export type ProfileTool = NonNullable<ProfileWrite["tools"]>[number];
export type ProfileLimits = NonNullable<ProfileWrite["limits"]>;

/** The runtime's built-in tools, in the order the form lists them (the schema's enum). */
export const TOOLS: readonly ProfileTool[] = [
  "read_file",
  "write_file",
  "edit_file",
  "glob",
  "grep",
  "read_notes",
  "write_notes",
  "shell",
  "skill",
  "fetch_url",
  // The channel tools (ADR-0014): a run has them only while its conversation belongs to a channel.
  "channel_post",
  "channel_read",
  "channel_wait",
  "task_claim",
];
/** The groups the Tools section lists them in (WEB-U-105, the owner's order); every tool is in one. */
export const TOOL_GROUPS: readonly (readonly [
  "files" | "finding" | "commands" | "notes" | "web" | "skills" | "channels",
  readonly ProfileTool[],
])[] = [
  ["files", ["read_file", "write_file", "edit_file"]],
  ["finding", ["glob", "grep"]],
  ["commands", ["shell"]],
  ["notes", ["read_notes", "write_notes"]],
  ["web", ["fetch_url"]],
  ["skills", ["skill"]],
  ["channels", ["channel_post", "channel_read", "channel_wait", "task_claim"]],
];
/** Bounds from `ProfileWrite` and ADR-0012. */
export const NAME = /^[a-z][a-z0-9_-]{0,63}$/;
export const MAX_DISPLAY_NAME = 64;
export const MAX_DESCRIPTION = 512;
export const MAX_INSTRUCTIONS_BYTES = 16 * 1024;
/** Servers and connectors together, the runtime's bound. */
export const MAX_SERVERS_AND_CONNECTORS = 8;
export const MAX_SKILLS = 64;
/** A skill name: checked for syntax only; a name no skill has narrows to nothing. */
export const SKILL = /^[a-z0-9-]{1,64}$/;
export const LIMITS: readonly (keyof ProfileLimits)[] = [
  "max_steps",
  "wall_clock_secs",
  "tool_result_bytes",
  "max_output_tokens",
];

export type ProfilesApi = {
  profiles(signal?: AbortSignal): Promise<Profiles>;
  /** Creates a personal profile, or a shared one with `shared: true` (administrators). */
  createProfile(fields: ProfileFields, key: string): Promise<Profile>;
  /** Replaces the version read; sharing never changes. */
  putProfile(
    id: string,
    fields: ProfileFields,
    version: number,
    key: string,
  ): Promise<Profile>;
  deleteProfile(id: string, version: number, key: string): Promise<void>;
};

/** The administrator's configured tool servers, which a shared profile may bring (WEB-U-059). */
export function toolServersFor(client: ApiClient) {
  return (signal?: AbortSignal): Promise<McpServers> =>
    client.request(
      settingsEndpoint("adminMcpServers"),
      (value) => parseAdminApi(value, "McpServers"),
      { signal },
    );
}

/** A decision engine as a profile's choice shows it: its name, what it sees, and whether it can answer now. */
export type EngineChoice = {
  name: string;
  label: string;
  exposure: "none" | "calls" | "full";
  enabled: boolean;
  keyMissing: boolean;
};

/** The installation's decision engines, which a profile may name (administrators only; ADR-0013 point 6). */
export function enginesFor(client: ApiClient) {
  return async (signal?: AbortSignal): Promise<EngineChoice[]> =>
    (
      await client.request(
        settingsEndpoint("adminDecisionEngines"),
        (value) => parseAdminApi(value, "AdminDecisionEngines"),
        { signal },
      )
    ).items.map((engine) => ({
      name: engine.name,
      label: engine.display_name ?? engine.name,
      exposure: engine.exposure,
      enabled: engine.enabled,
      keyMissing: engine.key_missing,
    }));
}

export function gatewayProfilesFor(client: ApiClient): ProfilesApi {
  return {
    profiles: (signal) => profiles(client, signal),
    createProfile: (fields, key) =>
      client.request(
        settingsEndpoint("profileCreate"),
        (value) => parseApi(value, "Profile"),
        { body: JSON.stringify(fields), key },
      ),
    putProfile: (id, fields, version, key) => {
      const body: ProfileWrite = { ...fields, version };
      return client.request(
        settingsEndpoint("profilePut"),
        (value) => parseApi(value, "Profile"),
        { parameters: { id }, body: JSON.stringify(body), key },
      );
    },
    deleteProfile: (id, version, key) =>
      client.request(settingsEndpoint("profileDelete"), () => undefined, {
        parameters: { id },
        query: new URLSearchParams({ version: String(version) }),
        key,
      }),
  };
}

export const gatewayProfiles: ProfilesApi = gatewayProfilesFor(api);

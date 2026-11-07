import type { Endpoint } from "../api/operations";

/**
 * The settings and channel pages' gateway operations (administration, a person's connector credentials, channels). They
 * live here, not in src/api/operations.ts, so the conversation page's scripts carry none of them (WEB-U-014); the parity
 * check reads both tables against the OpenAPI document.
 */
export const settingsOperations = {
  adminUsage: { method: "GET", path: "/api/v1/admin/usage" },
  adminSecrets: { method: "GET", path: "/api/v1/admin/secrets" },
  adminSecretPut: { method: "PUT", path: "/api/v1/admin/secrets/{name}" },
  adminSecretDelete: {
    method: "DELETE",
    path: "/api/v1/admin/secrets/{name}",
  },
  adminSecretReads: {
    method: "GET",
    path: "/api/v1/admin/secrets/{name}/reads",
  },
  connectorCredentialReads: {
    method: "GET",
    path: "/api/v1/connectors/{name}/credential/reads",
  },
  adminSecretConsumers: {
    method: "PUT",
    path: "/api/v1/admin/secrets/{name}/consumers",
  },
  adminSecretReminder: {
    method: "PUT",
    path: "/api/v1/admin/secrets/{name}/reminder",
  },
  channels: { method: "GET", path: "/api/v1/channels" },
  channelCreate: { method: "POST", path: "/api/v1/channels" },
  channel: { method: "GET", path: "/api/v1/channels/{id}" },
  channelMembers: { method: "GET", path: "/api/v1/channels/{id}/members" },
  channelMessages: { method: "GET", path: "/api/v1/channels/{id}/messages" },
  channelPost: { method: "POST", path: "/api/v1/channels/{id}/messages" },
  channelEvents: { method: "GET", path: "/api/v1/channels/{id}/events" },
  channelTasks: { method: "GET", path: "/api/v1/channels/{id}/tasks" },
  channelProposal: {
    method: "GET",
    path: "/api/v1/channels/{id}/proposals/{seq}",
  },
  channelClaim: {
    method: "POST",
    path: "/api/v1/channels/{id}/tasks/{seq}/claim",
  },
  channelMemberByEmail: {
    method: "PUT",
    path: "/api/v1/channels/{id}/members",
  },
  channelMemberPut: {
    method: "PUT",
    path: "/api/v1/channels/{id}/members/{kind}/{member}",
  },
  channelMemberDelete: {
    method: "DELETE",
    path: "/api/v1/channels/{id}/members/{kind}/{member}",
  },
  channelEnd: { method: "POST", path: "/api/v1/channels/{id}/end" },
  channelDelete: { method: "DELETE", path: "/api/v1/channels/{id}" },
  adminMcpServers: { method: "GET", path: "/api/v1/admin/mcp/servers" },
  adminEgress: { method: "GET", path: "/api/v1/admin/egress" },
  adminEgressPut: { method: "PUT", path: "/api/v1/admin/egress/{host}" },
  adminEgressDelete: {
    method: "DELETE",
    path: "/api/v1/admin/egress/{host}",
  },
  adminEgressPresets: { method: "GET", path: "/api/v1/admin/egress/presets" },
  adminEgressPresetApply: {
    method: "POST",
    path: "/api/v1/admin/egress/presets/{id}",
  },
  adminEgressPresetRemove: {
    method: "DELETE",
    path: "/api/v1/admin/egress/presets/{id}",
  },
  adminConnectors: { method: "GET", path: "/api/v1/admin/connectors" },
  adminConnectorPut: {
    method: "PUT",
    path: "/api/v1/admin/connectors/{name}",
  },
  adminConnectorDelete: {
    method: "DELETE",
    path: "/api/v1/admin/connectors/{name}",
  },
  adminDecisionEngines: {
    method: "GET",
    path: "/api/v1/admin/decision/engines",
  },
  adminDecisionEnginePut: {
    method: "PUT",
    path: "/api/v1/admin/decision/engines/{name}",
  },
  adminDecisionEngineDelete: {
    method: "DELETE",
    path: "/api/v1/admin/decision/engines/{name}",
  },
  adminLimits: { method: "GET", path: "/api/v1/admin/limits" },
  adminLimitPut: { method: "PUT", path: "/api/v1/admin/limits/{key}" },
  adminLimitDelete: {
    method: "DELETE",
    path: "/api/v1/admin/limits/{key}",
  },
  adminAgentTeams: { method: "GET", path: "/api/v1/admin/agent-teams" },
  adminAgentTeamsPut: { method: "PUT", path: "/api/v1/admin/agent-teams" },
  adminRoutes: { method: "GET", path: "/api/v1/admin/routes" },
  adminRoutesExport: { method: "GET", path: "/api/v1/admin/routes/export" },
  adminRoutePut: { method: "PUT", path: "/api/v1/admin/routes/{name}" },
  adminRouteProbe: {
    method: "POST",
    path: "/api/v1/admin/routes/{name}/probe",
  },
  adminRouteModels: {
    method: "GET",
    path: "/api/v1/admin/routes/{name}/models",
  },
  adminRouteDelete: {
    method: "DELETE",
    path: "/api/v1/admin/routes/{name}",
  },
  profileCreate: { method: "POST", path: "/api/v1/profiles" },
  profilePut: { method: "PUT", path: "/api/v1/profiles/{id}" },
  profileDelete: { method: "DELETE", path: "/api/v1/profiles/{id}" },
  userConnectors: { method: "GET", path: "/api/v1/connectors" },
  putConnectorCredential: {
    method: "PUT",
    path: "/api/v1/connectors/{name}/credential",
  },
  deleteConnectorCredential: {
    method: "DELETE",
    path: "/api/v1/connectors/{name}/credential",
  },
  adminTemplates: { method: "GET", path: "/api/v1/admin/templates" },
  adminTemplatePut: {
    method: "PUT",
    path: "/api/v1/admin/templates/{name}",
  },
  adminTemplateDelete: {
    method: "DELETE",
    path: "/api/v1/admin/templates/{name}",
  },
  adminTemplateDefault: {
    method: "POST",
    path: "/api/v1/admin/templates/{name}/default",
  },
  adminTemplateCheck: {
    method: "POST",
    path: "/api/v1/admin/templates/{name}/check",
  },
  adminTemplateReset: {
    method: "POST",
    path: "/api/v1/admin/templates/{name}/reset",
  },
  adminEnvironmentImages: {
    method: "GET",
    path: "/api/v1/admin/environment-images",
  },
  // A person's account, tokens and Git credentials, invitations and the gateway's readiness: settings pages only.
  readiness: { method: "GET", path: "/readyz" },
  changeUser: { method: "PATCH", path: "/api/v1/users/{id}" },
  invitations: { method: "GET", path: "/api/v1/invitations" },
  createInvitation: { method: "POST", path: "/api/v1/invitations" },
  revokeInvitation: { method: "DELETE", path: "/api/v1/invitations/{id}" },
  tokens: { method: "GET", path: "/api/v1/tokens" },
  createToken: { method: "POST", path: "/api/v1/tokens" },
  revokeToken: { method: "DELETE", path: "/api/v1/tokens/{id}" },
  hostKeys: { method: "GET", path: "/api/v1/host-keys" },
  hostKeyAdd: { method: "POST", path: "/api/v1/host-keys" },
  hostKeyPut: { method: "PUT", path: "/api/v1/host-keys/{id}" },
  hostKeyDelete: { method: "DELETE", path: "/api/v1/host-keys/{id}" },
  hostKeysShared: { method: "GET", path: "/api/v1/host-keys/shared" },
  hostKeyReads: { method: "GET", path: "/api/v1/host-keys/{id}/reads" },
  adminHostKeys: { method: "GET", path: "/api/v1/admin/host-keys" },
  adminHostKeyAdd: { method: "POST", path: "/api/v1/admin/host-keys" },
  adminHostKeyPut: { method: "PUT", path: "/api/v1/admin/host-keys/{id}" },
  adminHostKeyDelete: {
    method: "DELETE",
    path: "/api/v1/admin/host-keys/{id}",
  },
  adminHostKeyReads: {
    method: "GET",
    path: "/api/v1/admin/host-keys/{id}/reads",
  },
} as const;

export type SettingsOperation = keyof typeof settingsOperations;

/** One settings operation as an endpoint the shared client sends. */
export const settingsEndpoint = (name: SettingsOperation): Endpoint => ({
  name,
  ...settingsOperations[name],
});

/** Method and path templates are checked against the gateway's OpenAPI document. */
export const operations = {
  setupState: { method: "GET", path: "/api/v1/setup" },
  bootstrap: { method: "POST", path: "/api/v1/auth/bootstrap" },
  login: { method: "POST", path: "/api/v1/auth/login" },
  logout: { method: "POST", path: "/api/v1/auth/logout" },
  me: { method: "GET", path: "/api/v1/auth/me" },
  models: { method: "GET", path: "/api/v1/models" },
  templates: { method: "GET", path: "/api/v1/templates" },
  profiles: { method: "GET", path: "/api/v1/profiles" },
  conversations: { method: "GET", path: "/api/v1/conversations" },
  createConversation: { method: "POST", path: "/api/v1/conversations" },
  conversation: { method: "GET", path: "/api/v1/conversations/{id}" },
  events: { method: "GET", path: "/api/v1/conversations/{id}/events" },
  screen: { method: "GET", path: "/api/v1/conversations/{id}/screen" },
  screenNotices: {
    method: "GET",
    path: "/api/v1/conversations/{id}/screen/notices",
  },
  screenshot: {
    method: "GET",
    path: "/api/v1/conversations/{id}/screenshots/{screenshot}",
  },
  screenSignOut: {
    method: "POST",
    path: "/api/v1/conversations/{id}/screen/sign-out",
  },
  keyUses: { method: "GET", path: "/api/v1/conversations/{id}/key-uses" },
  updateConversation: { method: "PATCH", path: "/api/v1/conversations/{id}" },
  switchEnvironment: {
    method: "PUT",
    path: "/api/v1/conversations/{id}/environment",
  },
  deleteConversation: { method: "DELETE", path: "/api/v1/conversations/{id}" },
  team: { method: "GET", path: "/api/v1/conversations/{id}/team" },
  teamMessages: {
    method: "GET",
    path: "/api/v1/conversations/{id}/team/messages",
  },
  teamAnswer: {
    method: "POST",
    path: "/api/v1/conversations/{id}/team/answers",
  },
  teamStop: { method: "POST", path: "/api/v1/conversations/{id}/team/stop" },
  message: { method: "POST", path: "/api/v1/conversations/{id}/messages" },
  queuedMessages: {
    method: "GET",
    path: "/api/v1/conversations/{id}/messages/queued",
  },
  cancelQueuedMessage: {
    method: "DELETE",
    path: "/api/v1/conversations/{id}/messages/queued/{message_id}",
  },
  cancel: {
    method: "POST",
    path: "/api/v1/conversations/{id}/runs/{run_id}/cancel",
  },
  users: { method: "GET", path: "/api/v1/users" },
  lookupInvitation: { method: "POST", path: "/api/v1/invitations/lookup" },
  acceptInvitation: { method: "POST", path: "/api/v1/invitations/accept" },
  files: { method: "GET", path: "/api/v1/conversations/{id}/files" },
  fileContent: {
    method: "GET",
    path: "/api/v1/conversations/{id}/files/content",
  },
  fileDownload: {
    method: "GET",
    path: "/api/v1/conversations/{id}/files/download",
  },
  fileView: {
    method: "GET",
    path: "/api/v1/conversations/{id}/files/view",
  },
  fileSheet: {
    method: "GET",
    path: "/api/v1/conversations/{id}/files/sheet",
  },
  upload: { method: "POST", path: "/api/v1/conversations/{id}/uploads" },
  workspaces: { method: "GET", path: "/api/v1/workspaces" },
  createWorkspace: { method: "POST", path: "/api/v1/workspaces" },
  workspace: { method: "GET", path: "/api/v1/workspaces/{id}" },
  updateWorkspace: { method: "PATCH", path: "/api/v1/workspaces/{id}" },
  deleteWorkspace: { method: "DELETE", path: "/api/v1/workspaces/{id}" },
  forkWorkspace: { method: "POST", path: "/api/v1/workspaces/{id}/fork" },
  workspaceShares: { method: "GET", path: "/api/v1/workspaces/{id}/shares" },
  shareWorkspaceByEmail: {
    method: "PUT",
    path: "/api/v1/workspaces/{id}/shares",
  },
  shareWorkspace: {
    method: "PUT",
    path: "/api/v1/workspaces/{id}/shares/{user_id}",
  },
  unshareWorkspace: {
    method: "DELETE",
    path: "/api/v1/workspaces/{id}/shares/{user_id}",
  },
  runChanges: {
    method: "GET",
    path: "/api/v1/conversations/{id}/runs/{run_id}/changes",
  },
  runChangeDiff: {
    method: "GET",
    path: "/api/v1/conversations/{id}/runs/{run_id}/changes/diff",
  },
} as const;

export type Operation = keyof typeof operations;
/** An operation outside this table (the settings pages' own, src/settings/operations.ts), sent by the same client. */
export type Endpoint = {
  readonly name: string;
  readonly method: string;
  readonly path: string;
};

const resolve = (operation: Operation | Endpoint) =>
  typeof operation === "string" ? operations[operation] : operation;
/** The operation's name, for the session rules (sign-in, sign-out). */
export const operationName = (operation: Operation | Endpoint) =>
  typeof operation === "string" ? operation : operation.name;
/** The operation as "METHOD /path/{template}", for a diagnostic that names the route. */
export const routeOf = (operation: Operation | Endpoint) =>
  `${resolve(operation).method} ${resolve(operation).path}`;
/** The operation's HTTP method. */
export const methodFor = (operation: Operation | Endpoint) =>
  resolve(operation).method;

export function pathFor(
  operation: Operation | Endpoint,
  parameters: Record<string, string> = {},
  query?: URLSearchParams,
): string {
  const path = resolve(operation).path.replace(
    /\{([^}]+)\}/g,
    (_match: string, name: string) => {
      const value = parameters[name];
      if (value === undefined) throw new Error("Missing API path parameter"); // copy-lint: not user copy
      return encodeURIComponent(value);
    },
  );
  return query?.size ? `${path}?${query.toString()}` : path;
}

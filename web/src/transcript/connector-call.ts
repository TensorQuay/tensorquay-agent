import type { MessageKey } from "../i18n/translate";

/**
 * A connector tool's name, `mcp.<connector>.<tool>` (ADR-0031): split at the first two dots only, since the tool
 * part is the remote server's own name and may hold dots. Null for any other tool.
 */
export function connectorName(
  name: string,
): { connector: string; tool: string } | null {
  const match = /^mcp\.([^.]+)\.(.+)$/.exec(name);
  return match?.[1] && match[2]
    ? { connector: match[1], tool: match[2] }
    : null;
}

/**
 * The failure codes the runtime's MCP client puts before ": " on a failed connector call, each as the activity line's
 * sentence (ADR-0031). A code not listed, or a result without the prefix (the tool's own error),
 * reads as the tool's answer instead. mcp_invalid_params (-32602) is left out on purpose: servers also send it for bad
 * arguments, so a sentence about the tool not being offered would mislead.
 */
export const CONNECTOR_REFUSALS: Readonly<Record<string, MessageKey>> = {
  connector_credential_missing: "activity.connectorCredential",
  // The remote refused the person's own credential (ADR-0031 point 3): check or replace it, on the same page.
  connector_credential_rejected: "activity.connectorCredentialRejected",
  connector_not_allowed: "activity.connectorUnavailable",
  connector_busy: "activity.connectorBusy",
  rate_limited: "activity.connectorBusy",
  connector_timeout: "activity.connectorTimeout",
  timeout: "activity.connectorTimeout",
  connector_unavailable: "activity.connectorUnreachable",
  connector_redirect: "activity.connectorUnreachable",
  connector_answer_too_large: "activity.connectorUnreachable",
  mcp_server_exited: "activity.connectorUnreachable",
  store_unavailable: "activity.connectorUnreachable",
  unauthorized: "activity.connectorUnreachable",
  mcp_error: "activity.connectorUnreachable",
  mcp_method_not_found: "activity.connectorUnreachable",
  // GW-I-147: the connector's key wasn't used, because its use couldn't be recorded.
  key_use_unrecorded: "error.key_use_unrecorded",
  // The gateway's MCP proxy refusing by itself (X-TQA-Gateway-Error): the connector gone, a request over its limit,
  // or a connector key an administrator hasn't set up.
  not_found: "activity.connectorUnavailable",
  body_too_large: "activity.connectorTooLarge",
  secret_unavailable: "activity.connectorKeyMissing",
};

/** A failed connector call's sentence, and whether it links to the person's connector credentials. */
const LINKED: ReadonlySet<MessageKey> = new Set<MessageKey>([
  "activity.connectorCredential",
  "activity.connectorCredentialRejected",
]);

export function connectorRefusal(
  result: string | null,
): { key: MessageKey; link: boolean } | null {
  const code = /^([a-z_]{1,64}): /.exec(result ?? "")?.[1];
  const key =
    code && Object.hasOwn(CONNECTOR_REFUSALS, code)
      ? CONNECTOR_REFUSALS[code]
      : undefined;
  return key ? { key, link: LINKED.has(key) } : null;
}

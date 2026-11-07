import { ApiError } from "../api/error";
import type { MessageValues } from "../i18n/message";
import type { MessageKey } from "../i18n/translate";

import {
  MAX_DISPLAY_NAME,
  MAX_HEADER,
  MAX_TOOL_NAME,
  MAX_TOOLS,
  TIMEOUT,
  type AuthScheme,
  type Connector,
  type ConnectorFields,
} from "./admin/connectors";
import { isKeyShapedName } from "./key-shapes";

export const SCHEMES: readonly (readonly [AuthScheme, MessageKey])[] = [
  ["none", "connectors.schemeNone"],
  ["bearer", "connectors.schemeBearer"],
  ["header", "connectors.schemeHeader"],
];

/** The form's editable text for one connector (null: a new one). */
export type ConnectorDraft = {
  name: string;
  displayName: string;
  url: string;
  scheme: AuthScheme;
  header: string;
  required: boolean;
  secretRef: string;
  toolMode: "all" | "allow" | "deny";
  tools: string;
  timeout: string;
  enabled: boolean;
  byDefault: boolean;
};

export function toDraft(item: Connector | null): ConnectorDraft {
  const listed = item?.tools?.allow ?? item?.tools?.deny ?? [];
  return {
    name: item?.name ?? "",
    displayName: item?.display_name ?? "",
    url: item?.url ?? "",
    scheme: item?.auth.scheme ?? "none",
    header: item?.auth.header ?? "",
    required: item?.auth.required ?? false,
    secretRef: item?.secret_ref ?? "",
    toolMode: item?.tools?.allow ? "allow" : item?.tools?.deny ? "deny" : "all",
    tools: listed.join("\n"),
    timeout: String(item?.call_timeout_seconds ?? TIMEOUT.default),
    enabled: item?.enabled ?? true,
    byDefault: item?.enabled_by_default ?? false,
  };
}

const NAME = /^[a-z](?:[a-z0-9]|_(?!_))*$/;
const HEADER = /^(?:Authorization|X-[A-Za-z0-9-]+)$/;

/** The server's https:// address without a name, password or fragment, as the gateway requires. */
function plainHttps(url: string): boolean {
  if (!/^https:\/\/\S+$/.test(url) || !URL.canParse(url)) return false;
  const parsed = new URL(url);
  return !parsed.username && !parsed.password && !url.includes("#");
}

function toolNames(text: string): string[] | null {
  const names = text
    .split("\n")
    .map((line) => line.trim())
    .filter(Boolean);
  const valid =
    names.length >= 1 &&
    names.length <= MAX_TOOLS &&
    new Set(names).size === names.length &&
    names.every((name) => name.length <= MAX_TOOL_NAME);
  return valid ? names : null;
}

/** The problems the page can see before sending, by form field; the gateway checks everything again. */
function draftProblems(draft: ConnectorDraft, creating: boolean) {
  const problems: Record<string, MessageKey> = {};
  const name = draft.name.trim();
  if (
    creating &&
    !(NAME.test(name) && name.length <= 16 && !name.endsWith("_"))
  )
    problems.name = "connectors.nameInvalid";
  if (!plainHttps(draft.url.trim())) problems.url = "connectors.urlInvalid";
  const header = draft.header.trim();
  if (
    draft.scheme === "header" &&
    !(HEADER.test(header) && header.length <= MAX_HEADER)
  )
    problems.header = "connectors.headerInvalid";
  if (draft.toolMode !== "all" && !toolNames(draft.tools))
    problems.tools = "connectors.toolsInvalid";
  const timeout = Number(draft.timeout);
  if (
    !/^\d+$/.test(draft.timeout.trim()) ||
    timeout < TIMEOUT.min ||
    timeout > TIMEOUT.max
  )
    problems.timeout = "connectors.timeoutInvalid";
  const display = draft.displayName.trim();
  if (display.length > MAX_DISPLAY_NAME || /[\r\n]/.test(draft.displayName))
    problems.displayName = "connectors.displayNameInvalid";
  // A shared key whose name looks like a key itself, as the gateway refuses it (SEC-033, WEB-U-303).
  if (draft.scheme !== "none" && isKeyShapedName(draft.secretRef))
    problems.secretRef = "connectors.secretKeyShaped";
  return problems;
}

/** The fields to send and the problems found; with no credential sent, none is required and no secret is named. */
export function fromDraft(draft: ConnectorDraft, creating: boolean) {
  const sends = draft.scheme !== "none";
  const tools = toolNames(draft.tools);
  const display = draft.displayName.trim();
  const fields: ConnectorFields = {
    display_name: display || null,
    url: draft.url.trim(),
    auth: {
      scheme: draft.scheme,
      ...(draft.scheme === "header" ? { header: draft.header.trim() } : {}),
      required: sends && draft.required,
    },
    ...(sends && draft.secretRef ? { secret_ref: draft.secretRef } : {}),
    ...(draft.toolMode !== "all" && tools
      ? { tools: { [draft.toolMode]: tools } }
      : {}),
    call_timeout_seconds: Number(draft.timeout),
    enabled: draft.enabled,
    enabled_by_default: draft.byDefault,
  };
  return { fields, problems: draftProblems(draft, creating) };
}

/** The tool filter in words: all tools, n allowed or all but n. */
export function toolsSummary(
  tools: Connector["tools"],
): [MessageKey, MessageValues] {
  if (tools?.allow)
    return ["connectors.toolsAllow", { count: tools.allow.length }];
  if (tools?.deny)
    return ["connectors.toolsDeny", { count: tools.deny.length }];
  return ["connectors.toolsAll", {}];
}

/** The gateway's field names as the form's fields. */
const FIELDS: Readonly<Record<string, string>> = {
  secret_ref: "secretRef",
  call_timeout_seconds: "timeout",
  display_name: "displayName",
};
const REASONS: Readonly<Record<string, MessageKey>> = {
  name_invalid: "connectors.nameInvalid",
  base_url_invalid: "connectors.urlInvalid",
  auth_query_invalid: "connectors.urlCredential",
  auth_header_invalid: "connectors.headerInvalid",
  auth_invalid: "connectors.authInvalid",
  secret_ref_invalid: "connectors.secretInvalid",
  secret_ref_reserved: "connectors.secretReserved",
  name_key_shaped: "connectors.secretKeyShaped",
  tool_filter_invalid: "connectors.toolsInvalid",
  limit_out_of_range: "connectors.timeoutInvalid",
  display_name_invalid: "connectors.displayNameInvalid",
  file_only: "connectors.fileOnly",
};

/** The gateway's named field problems (400 details), as catalogue text per form field. */
export function connectorProblems(error: unknown): Record<string, MessageKey> {
  const details =
    error instanceof ApiError && error.status === 400 ? error.details : [];
  return Object.fromEntries(
    details.map(({ field, reason }) => [
      field === "auth"
        ? reason === "auth_header_invalid"
          ? "header"
          : "scheme"
        : (FIELDS[field] ?? field),
      REASONS[reason] ?? "connectors.fieldInvalid",
    ]),
  );
}

/** Catalogue text for a refused write; null when the field messages already say it. */
export function connectorFailure(error: unknown): MessageKey | null {
  if (!(error instanceof ApiError)) return "connectors.saveFailed";
  if (error.status === 400) return null;
  if (error.code === "conflict") return "connectors.conflict";
  if (error.code === "name_taken") return "connectors.nameTaken";
  if (error.code === "connector_limit") return "connectors.full";
  return "connectors.saveFailed";
}

/** Where a connector came from, as on the other settings lists. */
export const SOURCES: Readonly<Record<string, MessageKey>> = {
  current: "connectors.fromSeed",
  removed: "connectors.seedRemoved",
  none: "connectors.addedHere",
};

/** The URL's host for the list; the whole address is in the edit form. */
export const hostOf = (url: string) =>
  URL.canParse(url) ? new URL(url).host : url;

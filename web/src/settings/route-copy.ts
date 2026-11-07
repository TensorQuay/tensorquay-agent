import type { FieldProblem } from "../api/error";
import type { MessageKey } from "../i18n/translate";

import type {
  CacheHint,
  RouteFields,
  RouteProtocol,
  RouteReasoning,
} from "./admin/routes";

/** The route form's editable text: numbers and lists as the user types them, converted on save. */
export type Draft = {
  name: string;
  display_name: string;
  description: string;
  protocol: RouteProtocol;
  base_url: string;
  model: string;
  authKind: RouteFields["auth"]["kind"];
  authName: string;
  key_ref: string;
  context_window: string;
  max_output: string;
  concurrency: string;
  requests_per_minute: string;
  /** How long the gateway waits for the model's first byte, in seconds; empty is its default for where the model runs. */
  first_response_secs: string;
  native_tools: boolean;
  parallel_tools: boolean;
  vision: boolean;
  stream_usage: boolean;
  cache_hint: CacheHint;
  reasoning: boolean;
  reasoningField: string;
  reasoningAllowed: string;
  reasoningDefault: string;
  budgets: string;
  fallbacks: string[];
  headers: string;
  body: string;
  quirks: string;
  /** The tool-schema settings, kept as they are (the form doesn't edit them yet). */
  strict: RouteFields["strict"];
  schema_keywords: RouteFields["schema_keywords"];
  /** Where the instructions go, kept as it is and only when the route sets it (its default depends on the protocol). */
  instructions_role?: RouteFields["instructions_role"];
};
export type Problems = Record<string, MessageKey>;
export const NUMBERS = [
  "context_window",
  "max_output",
  "concurrency",
  "requests_per_minute",
] as const;

const lines = (text: string) =>
  text
    .split("\n")
    .map((line) => line.trim())
    .filter(Boolean);
const list = (text: string) =>
  text
    .split(",")
    .map((item) => item.trim())
    .filter(Boolean);

type Role = NonNullable<RouteFields["instructions_role"]>;
/**
 * Where each protocol can send the merged instructions besides its default: Chat Completions a system
 * (its default) or developer message, Responses the instructions field (its default) or a developer message, Messages
 * only the instructions field, so nothing to choose.
 */
export const ROLES: Readonly<Record<RouteProtocol, readonly Role[]>> = {
  openai_chat: ["system", "developer"],
  openai_responses: ["top_level", "developer"],
  anthropic_messages: [],
};

/** A protocol change: a role the new protocol can't send goes back to its default, and `reset` says so. */
export function withProtocol(
  draft: Draft,
  protocol: RouteProtocol,
): { draft: Draft; reset: boolean } {
  const { instructions_role: role, ...rest } = draft;
  if (role === undefined || ROLES[protocol].includes(role))
    return { draft: { ...draft, protocol }, reset: false };
  return { draft: { ...rest, protocol }, reset: true };
}

/** A shared secret's reference, with the name rule the secrets page checks. */
const REFERENCE = /^secrets\/[A-Za-z0-9_-]{1,120}$/;

export function toDraft(name: string, fields: RouteFields): Draft {
  const auth = fields.auth;
  const reasoning = fields.reasoning;
  return {
    name,
    display_name: fields.display_name ?? "",
    description: fields.description ?? "",
    protocol: fields.protocol,
    base_url: fields.base_url,
    model: fields.model,
    authKind: auth.kind,
    authName: auth.kind === "header" || auth.kind === "query" ? auth.name : "",
    key_ref: fields.key_ref ?? "",
    ...Object.fromEntries(
      NUMBERS.map((key) => [key, fields[key]?.toString() ?? ""]),
    ),
    first_response_secs: fields.first_response_secs?.toString() ?? "",
    native_tools: fields.native_tools,
    parallel_tools: fields.parallel_tools,
    vision: fields.vision,
    stream_usage: fields.stream_usage,
    cache_hint: fields.cache_hint,
    reasoning: reasoning !== null,
    reasoningField: reasoning?.field ?? "",
    reasoningAllowed: reasoning?.allowed.join(", ") ?? "",
    reasoningDefault: reasoning?.default ?? "",
    budgets: Object.entries(reasoning?.budget_tokens ?? {})
      .map(([level, tokens]) => `${level}: ${tokens}`)
      .join("\n"),
    fallbacks: [...fields.fallbacks],
    strict: fields.strict,
    schema_keywords: fields.schema_keywords,
    ...(fields.instructions_role === undefined
      ? {}
      : { instructions_role: fields.instructions_role }),
    headers: Object.entries(fields.extra_headers)
      .map(([header, value]) => `${header}: ${value}`)
      .join("\n"),
    body: Object.keys(fields.extra_body).length
      ? JSON.stringify(fields.extra_body, null, 2)
      : "",
    quirks: fields.quirks.join(", "),
  } as Draft;
}

function pairs(text: string): Record<string, string> | null {
  const entries = lines(text).map((line) => {
    const at = line.indexOf(":");
    return at > 0
      ? [line.slice(0, at).trim(), line.slice(at + 1).trim()]
      : null;
  });
  return entries.every(Boolean)
    ? Object.fromEntries(entries as [string, string][])
    : null;
}

function reasoningOf(draft: Draft, problems: Problems): RouteReasoning | null {
  if (!draft.reasoning) return null;
  const budgets = pairs(draft.budgets);
  const tokens =
    budgets &&
    Object.entries(budgets).map(([level, n]) => [level, Number(n)] as const);
  if (!tokens || tokens.some(([, n]) => !Number.isInteger(n) || n <= 0))
    problems.budget_tokens = "models.budgetInvalid";
  return {
    field: draft.reasoningField.trim(),
    allowed: list(draft.reasoningAllowed),
    default: draft.reasoningDefault.trim(),
    ...(tokens?.length ? { budget_tokens: Object.fromEntries(tokens) } : {}),
  };
}

/** The contract's fields from the draft, or the problems the client can name before sending. */
/** A route's or environment's description as the gateway takes it: empty, or one line of 1 to 160 characters. */
export function descriptionFits(text: string): boolean {
  const value = text.trim();

  return value.length <= 160 && !/[\u0000-\u001f\u007f]/.test(value);
}

/** The name no route may take: the Add route page lives at /settings/models/new (GW-U-098, the gateway's rule). */
export const RESERVED_ROUTE_NAME = "new";

export function fromDraft(
  draft: Draft,
  /** Adding a route: it needs a display name. A stored one without (the README's quick start) may keep none. */
  creating = true,
): {
  fields: RouteFields;
  problems: Problems;
} {
  const problems: Problems = {};
  const number = (key: (typeof NUMBERS)[number]) => {
    const text = draft[key].trim();
    if (!text) return null;
    const value = Number(text);
    if (!Number.isInteger(value) || value <= 0)
      problems[key] = "models.wholeNumber";
    return value;
  };
  // 5 to 900 seconds, as the gateway checks it (first_response_invalid); empty sends none.
  const firstText = draft.first_response_secs.trim();
  const first = firstText ? Number(firstText) : null;
  if (first !== null && (!Number.isInteger(first) || first < 5 || first > 900))
    problems.first_response_secs = "models.firstResponseRange";
  let body: Record<string, unknown> = {};
  try {
    const parsed: unknown = draft.body.trim() ? JSON.parse(draft.body) : {};
    if (!parsed || typeof parsed !== "object" || Array.isArray(parsed))
      throw new Error("not an object");
    body = parsed as Record<string, unknown>;
  } catch {
    problems.extra_body = "models.jsonObject";
  }
  const headers = pairs(draft.headers);
  if (!headers) problems.extra_headers = "models.headerLine";
  const auth: RouteFields["auth"] =
    draft.authKind === "header" || draft.authKind === "query"
      ? { kind: draft.authKind, name: draft.authName.trim() }
      : { kind: draft.authKind };
  // The key is always a stored secret's reference: a pasted key is never sent as one (WEB-U-056).
  const keyRef =
    draft.authKind === "none" ? null : draft.key_ref.trim() || null;
  if (keyRef !== null && !REFERENCE.test(keyRef))
    problems.key_ref = "models.keyRefNotReference";
  // Named here, so a key typed inline is never stored for a route the gateway refuses for want of it (F4).
  if (!draft.model.trim()) problems.model = "models.modelEmpty";
  // One line of up to 160 characters, as the gateway checks it (description_invalid); surrounding spaces are trimmed.
  if (!descriptionFits(draft.description))
    problems.description = "models.descriptionInvalid";
  // The name people pick the route by: required for a new route, though the gateway allows none (F4). A route stored
  // without one is shown by its model's name, and saving another field never asks for one (WEB-U-111).
  if (creating && !draft.display_name.trim())
    problems.display_name = "models.displayNameRequired";
  // The Add route page's address (GW-U-098): the gateway refuses it too.
  if (creating && draft.name.trim() === RESERVED_ROUTE_NAME)
    problems.name = "models.nameReserved";
  const fields: RouteFields = {
    display_name: draft.display_name.trim() || null,
    description: draft.description.trim() || null,
    protocol: draft.protocol,
    base_url: draft.base_url.trim(),
    model: draft.model.trim(),
    auth,
    key_ref: keyRef,
    context_window: number("context_window"),
    max_output: number("max_output"),
    concurrency: number("concurrency"),
    requests_per_minute: number("requests_per_minute"),
    native_tools: draft.native_tools,
    parallel_tools: draft.parallel_tools,
    vision: draft.vision,
    reasoning: reasoningOf(draft, problems),
    cache_hint: draft.cache_hint,
    stream_usage: draft.stream_usage,
    extra_headers: headers ?? {},
    extra_body: body,
    quirks: list(draft.quirks),
    strict: draft.strict,
    schema_keywords: draft.schema_keywords,
    ...(draft.instructions_role === undefined
      ? {}
      : { instructions_role: draft.instructions_role }),
    ...(first === null ? {} : { first_response_secs: first }),
    fallbacks: [...draft.fallbacks],
  };
  return { fields, problems };
}

const REASONS: Readonly<Record<string, MessageKey>> = {
  route_name_invalid: "models.nameInvalid",
  name_reserved: "models.nameReserved",
  model_empty: "models.modelEmpty",
  description_invalid: "models.descriptionInvalid",
  base_url_invalid: "models.baseUrlInvalid",
  key_ref_required: "models.keyRequired",
  key_ref_not_reference: "models.keyRefNotReference",
  // A shared key whose name looks like a key itself (SEC-033, WEB-U-303): the same words as text that isn't a key's name.
  name_key_shaped: "models.keyRefNotReference",
  key_ref_reserved: "models.keyRefReserved",
  strict_invalid: "models.choiceInvalid",
  schema_keywords_invalid: "models.choiceInvalid",
  instructions_role_invalid: "models.choiceInvalid",
  first_response_invalid: "models.firstResponseRange",
  auth_query_invalid: "models.authNameInvalid",
  reasoning_invalid: "models.reasoningInvalid",
  extra_body_reserved: "models.bodyReserved",
  extra_header_reserved: "models.headerReserved",
  extra_header_credential: "models.headerCredential",
  quirk_unsupported: "models.quirkUnknown",
  fallback_invalid: "models.fallbackInvalid",
  budget_invalid: "models.budgetInvalid",
  limit_out_of_range: "models.limitInvalid",
  display_name_invalid: "models.displayNameInvalid",
};

/** The gateway's named field problems, as catalogue text per field. */
export function gatewayProblems(details: readonly FieldProblem[]): Problems {
  return Object.fromEntries(
    details.map((problem) => [
      problem.field,
      REASONS[problem.reason] ?? "models.fieldInvalid",
    ]),
  );
}

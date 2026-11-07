import { ApiError } from "../api/error";
import type { MessageKey } from "../i18n/translate";

import {
  ENGINE_TIMEOUT,
  MAX_ENGINE_NAME,
  MAX_HEADER,
  type Engine,
  type EngineFields,
  type EngineScheme,
  type Exposure,
} from "./admin/decision-engines";
import { isKeyShapedName } from "./key-shapes";

/** What the engine sees, from least to most (ADR-0013 point 5), as the choice reads. */
export const EXPOSURES: readonly (readonly [
  Exposure,
  MessageKey,
  MessageKey,
])[] = [
  ["none", "engines.seesNames", "engines.seesNamesHint"],
  ["calls", "engines.seesCalls", "engines.seesCallsHint"],
  ["full", "engines.seesFull", "engines.seesFullHint"],
];
const RANK: Readonly<Record<Exposure, number>> = { none: 0, calls: 1, full: 2 };

/** Whether saving `next` lets the engine see more than it does now (a new engine: more than tool names). */
export const widens = (current: Exposure | undefined, next: Exposure) =>
  RANK[next] > RANK[current ?? "none"];

/** What the engine sees, in a few words, for the list. */
export const EXPOSURE_SHORT: Readonly<Record<Exposure, MessageKey>> = {
  none: "engines.seesNamesShort",
  calls: "engines.seesCallsShort",
  full: "engines.seesFullShort",
};

/** How the key is sent, once there is one. */
/** What the engine sees, as a phrase inside a sentence ("sees the full conversation"). */
export const EXPOSURE_PHRASE: Readonly<Record<Exposure, MessageKey>> = {
  none: "engines.seesNamesPhrase",
  calls: "engines.seesCallsPhrase",
  full: "engines.seesFullPhrase",
};

export const ENGINE_SCHEMES: readonly (readonly [
  Exclude<EngineScheme, "none">,
  MessageKey,
])[] = [
  ["bearer", "engines.schemeBearer"],
  ["header", "engines.schemeHeader"],
];
/** The key choice meaning the engine needs none. */
export const NO_KEY = "none";

/** The form's editable text for one engine (null: a new one). */
/** The engine kinds, as the add form offers them: an engine speaking `decision/v1`, or a typed-question service. */
export type EngineKind = NonNullable<Engine["protocol"]>;
export const ENGINE_KINDS: readonly (readonly [
  EngineKind,
  MessageKey,
  MessageKey,
])[] = [
  ["decision_v1", "engines.kindDecision", "engines.kindDecisionHint"],
  ["typed_questions_v1", "engines.kindTyped", "engines.kindTypedHint"],
];
export const BUDGET_PERIODS: readonly (readonly [
  "total" | "month",
  MessageKey,
])[] = [
  ["total", "engines.budgetTotal"],
  ["month", "engines.budgetMonth"],
];

export type EngineDraft = {
  kind: EngineKind;
  displayName: string;
  /** The identifier; derived from the display name until edited, fixed once the engine exists. */
  name: string;
  nameEdited: boolean;
  url: string;
  exposure: Exposure;
  /** A secret's ref, `NO_KEY`, or empty when not chosen yet. */
  key: string;
  scheme: Exclude<EngineScheme, "none">;
  header: string;
  /** In seconds, as people read it. */
  timeout: string;
  enabled: boolean;
  /** A typed-question engine's model identifier (its service requires one). */
  model: string;
  /** A typed-question engine's threshold, 0 to 1; empty keeps the default. */
  minConfidence: string;
  /** A multiple-choice service's description of each route, by route name; an empty one is none. */
  hints: Record<string, string>;
  /** The optional budget; both empty means none. */
  budgetCalls: string;
  budgetTokens: string;
  budgetPeriod: "total" | "month";
};

export function toEngineDraft(item: Engine | null): EngineDraft {
  const scheme = item?.auth.scheme ?? "bearer";
  return {
    kind: item?.protocol ?? "decision_v1",
    model: item?.model ?? "",
    minConfidence:
      item?.min_confidence === undefined ? "" : String(item.min_confidence),
    hints: { ...item?.route_hints },
    budgetCalls:
      item?.budget?.calls === undefined ? "" : String(item.budget.calls),
    budgetTokens:
      item?.budget?.tokens === undefined ? "" : String(item.budget.tokens),
    budgetPeriod: item?.budget?.period ?? "total",
    displayName: item?.display_name ?? "",
    name: item?.name ?? "",
    nameEdited: item !== null,
    url: item?.url ?? "",
    exposure: item?.exposure ?? "none",
    key: item ? (scheme === "none" ? NO_KEY : (item.key_ref ?? "")) : "",
    scheme: scheme === "none" ? "bearer" : scheme,
    header: item?.auth.header ?? "",
    timeout: String((item?.timeout_ms ?? ENGINE_TIMEOUT.default) / 1000),
    enabled: item?.enabled ?? true,
  };
}

/** An identifier from a display name: lower-case letters, digits and single dashes, starting with a letter. */
export function identifierFor(display: string): string {
  const slug = display
    .normalize("NFKD")
    .replace(/[\u0300-\u036f]/g, "")
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");
  const named = /^[a-z]/.test(slug) ? slug : `engine-${slug}`.replace(/-$/, "");
  return named.slice(0, MAX_ENGINE_NAME).replace(/-+$/, "");
}

/** The draft's identifier: as edited, or derived from the display name. */
export const draftName = (draft: EngineDraft) =>
  (draft.nameEdited ? draft.name : identifierFor(draft.displayName)).trim();

const NAME = /^[a-z][a-z0-9_-]*$/;
const HEADER = /^(?:Authorization|X-[A-Za-z0-9-]+)$/;
const MAX_DISPLAY_NAME = 80;

/** The engine's https:// address without a name, password or fragment, as the gateway requires. */
function plainHttps(url: string): boolean {
  if (!/^https:\/\/\S+$/.test(url) || !URL.canParse(url)) return false;
  const parsed = new URL(url);
  return !parsed.username && !parsed.password && !url.includes("#");
}

/** The most routes a multiple-choice service's question describes (the gateway's bound). */
export const MAX_ROUTE_HINTS = 16;
/** The longest route description, in characters (the gateway's bound). */
export const MAX_ROUTE_HINT = 200;

/** The descriptions to send: each route's, trimmed, without the empty ones. */
export function routeHints(hints: Record<string, string>) {
  return Object.fromEntries(
    Object.entries(hints)
      .map(([route, hint]) => [route, hint.trim()] as const)
      .filter(([, hint]) => hint !== ""),
  );
}

/** Whether a route description breaks its bound: one line of up to 200 characters. */
export const hintTooLong = (hint: string) =>
  hint.trim().length > MAX_ROUTE_HINT || /[\r\n]/.test(hint);

/** A count as typed: grouping spaces (any width) and commas dropped, so "30 000 000" and "30,000,000" both read. */
export const countDigits = (text: string) => text.replace(/[\s,]/g, "");

const positive = (text: string) =>
  /^\d+$/.test(countDigits(text)) && Number(countDigits(text)) >= 1;

/** The typed-question kind's and the budget's problems. */
function kindProblems(
  draft: EngineDraft,
  problems: Record<string, MessageKey>,
) {
  const typed = draft.kind === "typed_questions_v1";
  if (typed) {
    const model = draft.model.trim();
    if (!model) problems.model = "engines.modelRequired";
    else if (!/^[A-Za-z0-9._:/-]{1,128}$/.test(model))
      problems.model = "engines.modelInvalid";
    const threshold = draft.minConfidence.trim();
    if (threshold && !(/^\d*\.?\d+$/.test(threshold) && Number(threshold) <= 1))
      problems.minConfidence = "engines.thresholdInvalid";
    // Each description names its own field; the section's problem opens More options.
    for (const [route, hint] of Object.entries(draft.hints))
      if (hintTooLong(hint)) problems[`hint:${route}`] = "engines.hintInvalid";
    if (Object.keys(routeHints(draft.hints)).length > MAX_ROUTE_HINTS)
      problems.hints = "engines.hintsTooMany";
    else if (Object.keys(problems).some((field) => field.startsWith("hint:")))
      problems.hints = "engines.hintInvalid";
  }
  // Each limit names its own field, so the wrong one is marked.
  const calls = countDigits(draft.budgetCalls);
  const tokens = typed ? countDigits(draft.budgetTokens) : "";
  if (calls && !positive(calls))
    problems.budgetCalls = "engines.budgetCallsInvalid";
  if (tokens && !positive(tokens))
    problems.budgetTokens = "engines.budgetTokensInvalid";
}

/** The problems the page can see before sending, by form field; the gateway checks everything again. */
function draftProblems(draft: EngineDraft, creating: boolean) {
  const problems: Record<string, MessageKey> = {};
  kindProblems(draft, problems);
  const display = draft.displayName.trim();
  if ((creating || draft.displayName !== "") && !display)
    problems.displayName = "engines.displayNameRequired";
  else if (display.length > MAX_DISPLAY_NAME || /[\r\n]/.test(display))
    problems.displayName = "engines.displayNameInvalid";
  const name = draftName(draft);
  if (
    creating &&
    display &&
    !(NAME.test(name) && name.length <= MAX_ENGINE_NAME)
  )
    problems.name = "engines.nameInvalid";
  if (!plainHttps(draft.url.trim())) problems.url = "engines.urlInvalid";
  if (!draft.key) problems.key = "engines.keyRequired";
  // A shared key whose name looks like a key itself, as the gateway refuses it (SEC-033, WEB-U-303).
  else if (draft.key !== NO_KEY && isKeyShapedName(draft.key))
    problems.key = "engines.secretKeyShaped";
  const header = draft.header.trim();
  if (
    draft.key !== NO_KEY &&
    draft.scheme === "header" &&
    !(HEADER.test(header) && header.length <= MAX_HEADER)
  )
    problems.header = "engines.headerInvalid";
  const seconds = Number(draft.timeout.trim());
  const ms = Math.round(seconds * 1000);
  if (
    !/^\d+(\.\d+)?$/.test(draft.timeout.trim()) ||
    ms < ENGINE_TIMEOUT.min ||
    ms > ENGINE_TIMEOUT.max
  )
    problems.timeout = "engines.timeoutInvalid";
  return problems;
}

/** The fields to send and the problems found; with no key, the engine is sent none and no secret is named. */
export function fromEngineDraft(draft: EngineDraft, creating: boolean) {
  const keyed = draft.key !== "" && draft.key !== NO_KEY;
  const display = draft.displayName.trim();
  const fields: EngineFields = {
    ...(display ? { display_name: display } : {}),
    url: draft.url.trim(),
    auth: keyed
      ? {
          scheme: draft.scheme,
          ...(draft.scheme === "header" ? { header: draft.header.trim() } : {}),
        }
      : { scheme: "none" },
    ...(keyed ? { key_ref: draft.key } : {}),
    timeout_ms: Math.round(Number(draft.timeout.trim()) * 1000),
    exposure: draft.exposure,
    enabled: draft.enabled,
    ...kindFields(draft),
  };
  return { fields, problems: draftProblems(draft, creating) };
}

/** The kind, its own fields, and the budget, as the gateway takes them. */
function kindFields(draft: EngineDraft): Partial<EngineFields> {
  const typed = draft.kind === "typed_questions_v1";
  const calls = countDigits(draft.budgetCalls);
  const tokens = typed ? countDigits(draft.budgetTokens) : "";
  const threshold = draft.minConfidence.trim();
  const hints = typed ? routeHints(draft.hints) : {};
  return {
    protocol: draft.kind,
    ...(typed ? { model: draft.model.trim() } : {}),
    ...(typed && threshold ? { min_confidence: Number(threshold) } : {}),
    ...(Object.keys(hints).length ? { route_hints: hints } : {}),
    ...(calls || tokens
      ? {
          budget: {
            ...(calls ? { calls: Number(calls) } : {}),
            ...(tokens ? { tokens: Number(tokens) } : {}),
            period: draft.budgetPeriod,
          },
        }
      : {}),
  };
}

/** The fields of a stored engine as they are, for a change of one field from the list (turning it on or off). */
export function fieldsOf(item: Engine): EngineFields {
  return {
    ...(item.display_name ? { display_name: item.display_name } : {}),
    url: item.url,
    auth: {
      scheme: item.auth.scheme ?? "none",
      ...(item.auth.header ? { header: item.auth.header } : {}),
    },
    ...(item.key_ref ? { key_ref: item.key_ref } : {}),
    timeout_ms: item.timeout_ms,
    exposure: item.exposure,
    enabled: item.enabled,
    protocol: item.protocol,
    ...(item.model ? { model: item.model } : {}),
    ...(item.min_confidence === undefined
      ? {}
      : { min_confidence: item.min_confidence }),
    ...(item.route_hints && Object.keys(item.route_hints).length
      ? { route_hints: item.route_hints }
      : {}),
    ...(item.budget ? { budget: item.budget } : {}),
  };
}

/** What people call an engine: its display name, or its identifier when it has none. */
export const engineLabel = (item: Pick<Engine, "name" | "display_name">) =>
  item.display_name ?? item.name;

/** A secret's name as people read it: its ref without the store's prefix. */
export const secretName = (ref: string) => ref.replace(/^secrets\//, "");

/** The gateway's field names as the form's fields. */
const FIELDS: Readonly<Record<string, string>> = {
  key_ref: "key",
  timeout_ms: "timeout",
  display_name: "displayName",
  min_confidence: "minConfidence",
  route_hints: "hints",
  budget: "budgetCalls",
};
const REASONS: Readonly<Record<string, MessageKey>> = {
  name_invalid: "engines.nameInvalid",
  base_url_invalid: "engines.urlInvalid",
  auth_query_invalid: "engines.urlCredential",
  auth_header_invalid: "engines.headerInvalid",
  auth_invalid: "engines.authInvalid",
  key_ref_not_reference: "engines.secretInvalid",
  secret_ref_reserved: "engines.secretInvalid",
  name_key_shaped: "engines.secretKeyShaped",
  limit_out_of_range: "engines.timeoutInvalid",
  display_name_invalid: "engines.displayNameInvalid",
  model_empty: "engines.modelRequired",
  budget_invalid: "engines.budgetCallsInvalid",
  route_name_invalid: "engines.hintInvalid",
  file_only: "engines.fileOnly",
};

/** The gateway's named field problems (400 details), as catalogue text per form field. */
export function engineProblems(error: unknown): Record<string, MessageKey> {
  const details =
    error instanceof ApiError && error.status === 400 ? error.details : [];
  return Object.fromEntries(
    details.map(({ field, reason }) => [
      field === "auth"
        ? reason === "auth_header_invalid"
          ? "header"
          : "scheme"
        : (FIELDS[field] ?? field),
      REASONS[reason] ?? "engines.fieldInvalid",
    ]),
  );
}

/** Catalogue text for a refused write or delete; null when the field messages already say it. */
export function engineFailure(error: unknown): MessageKey | null {
  if (!(error instanceof ApiError)) return "engines.saveFailed";
  if (error.status === 400) return null;
  if (error.code === "conflict") return "engines.conflict";
  if (error.code === "decision_engine_limit") return "engines.full";
  if (error.code === "decision_engine_in_use") return "engines.inUse";
  return "engines.saveFailed";
}

/** Where an engine came from, as on the other settings lists. */
/** Where a seeded engine came from; one added here shows no source (it would only repeat on every row). */
export const ENGINE_SOURCES: Readonly<Record<string, MessageKey>> = {
  current: "engines.fromSeed",
  removed: "engines.seedRemoved",
};

/** The URL's host for the list; the whole address is in the edit form. */
export const engineHost = (url: string) =>
  URL.canParse(url) ? new URL(url).host : url;

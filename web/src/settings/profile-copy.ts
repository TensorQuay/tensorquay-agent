import { ApiError } from "../api/error";
import type { MessageKey } from "../i18n/translate";

import {
  LIMITS,
  MAX_DESCRIPTION,
  MAX_DISPLAY_NAME,
  MAX_INSTRUCTIONS_BYTES,
  MAX_SERVERS_AND_CONNECTORS,
  MAX_SKILLS,
  NAME,
  SKILL,
  TOOLS,
  type Profile,
  type ProfileFields,
  type ProfileLimits,
  type ProfileTool,
} from "./profiles";

type BusyPolicy = NonNullable<ProfileFields["busy_policy"]>;

/** The form's editable text for one profile (null: a new one); empty means "not set: the task or the install decides". */
export type ProfileDraft = {
  name: string;
  displayName: string;
  description: string;
  instructions: string;
  route: string;
  template: string;
  /** The decision engine the tasks consult; empty: none, the agent decides alone. Kept as read for members. */
  decisionEngine: string;
  /** The tools ticked; every one of them saves as no allow-list (`tools: null`, WEB-U-105). */
  tools: ProfileTool[];
  skills: "all" | "none" | "named";
  skillNames: string;
  connectors: string[];
  /** Kept as read: there is no list of tool servers to choose from. */
  mcpServers: string[];
  limits: Record<keyof ProfileLimits, string>;
  busyPolicy: "" | BusyPolicy;
  shared: boolean;
};

export function toDraft(profile: Profile | null): ProfileDraft {
  const skills = profile?.skills ?? "all";
  return {
    name: profile?.name ?? "",
    displayName: profile?.display_name ?? "",
    description: profile?.description ?? "",
    instructions: profile?.instructions ?? "",
    route: profile?.model_route ?? "",
    template: profile?.template ?? "",
    decisionEngine: profile?.decision_engine ?? "",
    tools: profile?.tools ?? [...TOOLS],
    skills: Array.isArray(skills) ? "named" : skills,
    skillNames: Array.isArray(skills) ? skills.join("\n") : "",
    connectors: profile?.connectors ?? [],
    mcpServers: profile?.mcp_servers ?? [],
    limits: Object.fromEntries(
      LIMITS.map((key) => [key, String(profile?.limits?.[key] ?? "")]),
    ) as Record<keyof ProfileLimits, string>,
    busyPolicy: profile?.busy_policy ?? "",
    shared: profile?.shared ?? false,
  };
}

/**
 * A copy of a shared profile, to keep as one's own (ADR-0012 D1): every field but the name, sharing and tool servers,
 * which only shared profiles set (WEB-U-059).
 */
export function duplicateDraft(profile: Profile): ProfileDraft {
  return {
    ...toDraft(profile),
    name: "",
    shared: false,
    mcpServers: [],
  };
}

const skillList = (text: string) =>
  text
    .split(/[\s,]+/)
    .map((name) => name.trim())
    .filter(Boolean);

/** The request for a draft, and the page's own problems by form field (the gateway checks everything again). */
export function fromDraft(
  draft: ProfileDraft,
  creating: boolean,
): { fields: ProfileFields; problems: Record<string, MessageKey> } {
  const problems: Record<string, MessageKey> = {};
  const name = draft.name.trim();
  const display = draft.displayName.trim();
  if (creating && !NAME.test(name)) problems.name = "profiles.nameInvalid";
  if (!display || display.length > MAX_DISPLAY_NAME || /[\r\n]/.test(display))
    problems.displayName = "profiles.displayNameInvalid";
  if (draft.description.length > MAX_DESCRIPTION)
    problems.description = "profiles.descriptionTooLong";
  if (
    new TextEncoder().encode(draft.instructions).length >
      MAX_INSTRUCTIONS_BYTES ||
    draft.instructions.includes("\u0000")
  )
    problems.instructions = "profiles.instructionsTooLong";
  const names = skillList(draft.skillNames);
  if (
    draft.skills === "named" &&
    (names.length === 0 ||
      names.length > MAX_SKILLS ||
      new Set(names).size !== names.length ||
      !names.every((skill) => SKILL.test(skill)))
  )
    problems.skills = "profiles.skillsInvalid";
  if (draft.tools.length === 0) problems.tools = "profiles.toolsEmpty";
  if (
    draft.connectors.length + draft.mcpServers.length >
    MAX_SERVERS_AND_CONNECTORS
  )
    problems.connectors = "profiles.tooManyConnectors";
  const limits: ProfileLimits = {};
  for (const key of LIMITS) {
    const text = draft.limits[key].trim();
    if (!text) continue;
    const value = Number(text);
    if (!Number.isSafeInteger(value) || value < 1)
      problems.limits = "profiles.limitInvalid";
    else limits[key] = value;
  }
  const fields: ProfileFields = {
    name,
    display_name: display,
    description: draft.description.trim() || null,
    instructions: draft.instructions.trim() ? draft.instructions : null,
    model_route: draft.route || null,
    template: draft.template || null,
    decision_engine: draft.decisionEngine || null,
    tools: TOOLS.every((tool) => draft.tools.includes(tool))
      ? null
      : TOOLS.filter((tool) => draft.tools.includes(tool)),
    skills: draft.skills === "named" ? names : draft.skills,
    mcp_servers: draft.mcpServers,
    connectors: draft.connectors,
    limits,
    busy_policy: draft.busyPolicy || null,
    ...(creating && draft.shared ? { shared: true } : {}),
  };
  return { fields, problems };
}

/** The gateway's field names as the form's fields. */
const FIELDS: Readonly<Record<string, string>> = {
  display_name: "displayName",
  model_route: "route",
  mcp_servers: "connectors",
  busy_policy: "busyPolicy",
  decision_engine: "decisionEngine",
};
const REASONS: Readonly<Record<string, MessageKey>> = {
  name_invalid: "profiles.nameInvalid",
  display_name_invalid: "profiles.displayNameInvalid",
  value_empty: "profiles.valueEmpty",
  value_too_long: "profiles.valueTooLong",
  value_nul: "profiles.valueTooLong",
  list_invalid: "profiles.listInvalid",
  route_name_invalid: "profiles.unknownReference",
  limit_out_of_range: "profiles.limitInvalid",
  unknown_reference: "profiles.unknownReference",
};

/** The gateway's named field problems (400 details, 422 unknown_reference), as catalogue text per form field. */
export function profileProblems(error: unknown): Record<string, MessageKey> {
  const details =
    error instanceof ApiError && (error.status === 400 || error.status === 422)
      ? error.details
      : [];
  return Object.fromEntries(
    details.map(({ field, reason }) => {
      const base = field.split(".")[0] ?? field;
      return [FIELDS[base] ?? base, REASONS[reason] ?? "profiles.fieldInvalid"];
    }),
  );
}

const FAILURES: Readonly<Record<string, MessageKey>> = {
  conflict: "profiles.conflict",
  name_taken: "profiles.nameTaken",
  forbidden: "profiles.forbidden",
};

/** Catalogue text for a refused write; null when the field messages already say it. */
export function profileFailure(error: unknown): MessageKey | null {
  if (!(error instanceof ApiError)) return "profiles.saveFailed";
  if (error.details.length > 0) return null;
  return FAILURES[error.code] ?? "profiles.saveFailed";
}

/** Why a profile can't be used now, per field: what a person can act on. */
export const UNUSABLE: Readonly<
  Record<NonNullable<Profile["unusable_field"]>, MessageKey>
> = {
  model_route: "profiles.unusableRoute",
  template: "profiles.unusableTemplate",
  mcp_servers: "profiles.unusableServers",
  connectors: "profiles.unusableConnectors",
  decision_engine: "profiles.unusableEngine",
};

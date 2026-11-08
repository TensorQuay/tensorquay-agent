import { ApiError, type FieldProblem } from "../api/error";
import type { MessageKey } from "../i18n/translate";

import type { SecretWrite } from "./admin/contract";
import { isKeyShapedName } from "./key-shapes";

const NAME = /^[A-Za-z0-9_-]{1,120}$/;
const REFERENCE = "secrets/";

/** A shared key's name as the page shows it: its reference without `secrets/`, which only Who may use states (K3). */
export const keyName = (ref: string) =>
  ref.startsWith(REFERENCE) ? ref.slice(REFERENCE.length) : ref;

/** The same checks the gateway makes, so most mistakes are named before anything is sent. */
export function secretProblems(
  write: SecretWrite,
  needsName: boolean,
): FieldProblem[] {
  const problems: FieldProblem[] = [];
  if (needsName && !NAME.test(write.name))
    problems.push({ field: "name", reason: "name_invalid" });
  // Personal Git and connector credentials own these names, in any case.
  else if (needsName && /^(git|connector)-/i.test(write.name))
    problems.push({ field: "name", reason: "reserved_prefix" });
  // A key pasted where its name belongs, as the gateway refuses it on create (SEC-033, WEB-U-303).
  else if (needsName && isKeyShapedName(write.name))
    problems.push({ field: "name", reason: "name_key_shaped" });
  if (write.value.length === 0)
    problems.push({ field: "value", reason: "value_empty" });
  else if (write.value.includes("\0"))
    problems.push({ field: "value", reason: "value_nul" });
  return problems;
}

const REASONS: Readonly<Record<string, MessageKey>> = {
  name_invalid: "secrets.nameInvalid",
  reserved_prefix: "secrets.reservedPrefix",
  value_empty: "secrets.valueEmpty",
  value_too_long: "secrets.valueTooLong",
  value_nul: "secrets.valueNul",
  name_key_shaped: "secrets.nameKeyShaped",
};

/** Catalogue text for one field problem; an unknown reason still names the field's problem plainly. */
export function reasonCopy(reason: string): MessageKey {
  return REASONS[reason] ?? "secrets.fieldInvalid";
}

/** The field problems of a refused write, from the client check or the gateway's `details`. */
export function fieldProblems(error: unknown): readonly FieldProblem[] {
  return error instanceof ApiError && error.status === 400 ? error.details : [];
}

/** Catalogue text for a refused secret write; null when field messages already say it. */
export function writeFailure(error: unknown): MessageKey | null {
  if (!(error instanceof ApiError)) return "secrets.saveFailed";
  if (error.status === 400 && error.details.length) return null;
  switch (error.code) {
    case "conflict":
      return "secrets.conflict";
    case "secret_in_use":
      return "secrets.inUse";
    case "secret_limit":
      return "secrets.limit";
    case "session_required":
      return "secrets.sessionRequired";
    case "not_found":
      return "secrets.gone";
    default:
      return "secrets.saveFailed";
  }
}

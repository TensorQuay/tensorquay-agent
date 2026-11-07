import { ApiError } from "../api/error";
import type { MessageKey } from "../i18n/translate";

import { MAX_PORT, MAX_PORTS } from "./admin/egress";

/** The ports as typed ("443, 8443"): 1 to 16 distinct whole numbers from 1 to 65535, or null. */
export function parsePorts(text: string): number[] | null {
  const parts = text
    .split(",")
    .map((part) => part.trim())
    .filter(Boolean);
  const ports = parts.map((part) => (/^\d+$/.test(part) ? Number(part) : NaN));
  const valid =
    ports.length >= 1 &&
    ports.length <= MAX_PORTS &&
    new Set(ports).size === ports.length &&
    ports.every(
      (port) => Number.isInteger(port) && port >= 1 && port <= MAX_PORT,
    );
  return valid ? ports : null;
}

const REASONS: Readonly<Record<string, MessageKey>> = {
  host_invalid: "egress.hostInvalid",
  ports_invalid: "egress.portsInvalid",
  internal_name_only: "egress.internalNameOnly",
};

/** The gateway's named field problems, as catalogue text per field. */
export function ruleProblems(error: unknown): Record<string, MessageKey> {
  const details =
    error instanceof ApiError && error.status === 400 ? error.details : [];
  return Object.fromEntries(
    details.map((problem) => [
      problem.field,
      REASONS[problem.reason] ?? "egress.fieldInvalid",
    ]),
  );
}

/** Catalogue text for a refused write; null when the field messages already say it. */
export function ruleFailure(error: unknown): MessageKey | null {
  if (!(error instanceof ApiError)) return "egress.saveFailed";
  if (error.status === 400) return null;
  if (error.code === "conflict") return "egress.conflict";
  if (error.code === "egress_rule_limit") return "egress.full";
  if (error.code === "egress_disabled") return "egress.disabledWrite";
  return "egress.saveFailed";
}

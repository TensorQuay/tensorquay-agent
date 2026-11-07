import type { Schemas } from "./generated/api";
import "./generated/validators.admin";
import { parseApiIn } from "./validate";

/**
 * The administration definitions (scripts/admin-schema.mjs): only the admin pages import this module, so the
 * conversation page's scripts never carry their validators (WEB-U-014).
 */
const ADMIN_SCHEMA = "urn:tqa:admin";

/** Validates an administration answer before it reaches a component. */
export function parseAdminApi<Name extends keyof Schemas>(
  value: unknown,
  name: Name,
): Schemas[Name] {
  return parseApiIn(ADMIN_SCHEMA, value, name);
}

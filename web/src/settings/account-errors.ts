import { ApiError } from "../api/error";
import type { MessageKey, Translate } from "../i18n/translate";

/** Account failures the reader can act on, in their words; anything else keeps the generic error copy. */
const COPY: Readonly<Record<string, MessageKey>> = {
  token_name_taken: "tokens.nameTaken",
  token_limit: "tokens.limit",
  last_admin: "users.lastAdmin",
  email_taken: "users.emailTaken",
  invitation_closed: "users.invitationClosed",
  invitation_invalid: "join.invalid",
  forbidden: "users.adminsOnly",
};

export function accountError(error: Error, t: Translate): string | null {
  if (!(error instanceof ApiError)) return null;
  const key = COPY[error.code];
  return key ? t(key) : null;
}

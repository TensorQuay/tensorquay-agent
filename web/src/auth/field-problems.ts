/**
 * The sign-in, setup and join forms' own checks before anything is sent (WEB-U-355): the browser's bubble is off
 * (`noValidate`), so each problem is said under its field in the reader's language (UI-GUIDELINES §5, input errors).
 * The gateway still enforces its rules; these only catch what a person can fix at once.
 */
export type FieldProblem =
  | "auth.emailRequired"
  | "auth.emailInvalid"
  | "password.required"
  | "password.tooShort"
  | "auth.setupTokenRequired";

/** One "@" with something on each side and no spaces: enough to catch a slip, never a full address check. */
export function emailProblem(email: string): FieldProblem | null {
  const value = email.trim();
  if (!value) return "auth.emailRequired";
  return /^[^\s@]+@[^\s@]+$/.test(value) ? null : "auth.emailInvalid";
}

/** Empty always; short only where the password is being chosen (setup and join: at least 12 characters). */
export function passwordProblem(
  password: string,
  choosing: boolean,
): FieldProblem | null {
  if (!password) return "password.required";
  return choosing && Array.from(password).length < 12
    ? "password.tooShort"
    : null;
}

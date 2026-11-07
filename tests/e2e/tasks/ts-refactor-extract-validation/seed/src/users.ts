/** A registered user. */
export interface User {
  email: string;
  handle: string;
  displayName: string;
}

/** A plausible email address: one @, a non-empty local part and a dotted domain. */
export function isEmail(text: string): boolean {
  return /^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(text);
}

/** A handle: 3 to 20 lowercase letters, digits or underscores, starting with a letter. */
export function isHandle(text: string): boolean {
  return /^[a-z][a-z0-9_]{2,19}$/.test(text);
}

/** Creates a user after validating its fields; the display name defaults to the handle. */
export function createUser(email: string, handle: string, displayName?: string): User {
  if (!isEmail(email)) {
    throw new Error(`invalid email: ${email}`);
  }
  if (!isHandle(handle)) {
    throw new Error(`invalid handle: ${handle}`);
  }
  return { email: email.toLowerCase(), handle, displayName: displayName?.trim() || handle };
}

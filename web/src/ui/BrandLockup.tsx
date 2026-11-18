import { APP_NAME } from "../lib/app";

/**
 * The app's one lockup (the 17.2 review's finding 16, decided on 18 Nov 2026): the mark with "TensorQuay Agent", as the
 * sidebar draws it, on the pages before sign-in. The mark is decoration; the name is read as its words.
 */
export function BrandLockup() {
  return (
    <span className="brand-lockup">
      <span className="brand-mark" aria-hidden="true" />
      <span className="type-body">{APP_NAME}</span>
    </span>
  );
}

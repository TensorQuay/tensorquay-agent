import { expect } from "vitest";

/**
 * WEB-U-501 (18.1, the 17.2 review's finding 16, decided on 18 Nov 2026): a page shows the app's one lockup, the mark
 * with "TensorQuay Agent" as the sidebar draws it, read as its words (the mark is decoration); never the condensed
 * wordmark.
 */
export function expectAppLockup() {
  expect(document.querySelector(".brand-wordmark")).toBeNull();
  const lockups = document.querySelectorAll<HTMLElement>(".brand-lockup");
  expect(lockups).toHaveLength(1);
  const lockup = lockups[0]!;
  expect(lockup.querySelector(".brand-mark")).toHaveAttribute(
    "aria-hidden",
    "true",
  );
  expect(lockup).toHaveTextContent(/^TensorQuay Agent$/);
}

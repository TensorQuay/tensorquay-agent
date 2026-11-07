import type { KeyboardEvent } from "react";

/** Keeps Tab navigation inside a modal, including at the browser-chrome boundary. */
export function trapDialogFocus(event: KeyboardEvent<HTMLDialogElement>) {
  if (event.key !== "Tab") return;
  const controls = Array.from(
    event.currentTarget.querySelectorAll<HTMLElement>(
      "button, [href], input, select, textarea, [tabindex]",
    ),
  ).filter((element) => {
    const style = getComputedStyle(element);
    return (
      element.tabIndex >= 0 &&
      !element.matches(":disabled") &&
      !element.closest("[hidden], [inert]") &&
      style.display !== "none" &&
      style.visibility !== "hidden"
    );
  });
  const first = controls[0];
  const last = controls.at(-1);
  const target = event.shiftKey ? last : first;
  const boundary = event.shiftKey ? first : last;
  if (target && document.activeElement === boundary) {
    event.preventDefault();
    target.focus();
  }
}

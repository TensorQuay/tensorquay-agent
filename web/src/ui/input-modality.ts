/**
 * Whether the person's last input was a key (not a pointer), for a control that shows its focus ring only when the
 * keyboard brought it (the title edit, WEB-U-114). Modifier-only and shortcut keys don't count.
 */
let keyboard = false;

if (typeof document !== "undefined") {
  document.addEventListener(
    "keydown",
    (event) => {
      if (!event.metaKey && !event.ctrlKey && !event.altKey) keyboard = true;
    },
    true,
  );
  document.addEventListener(
    "pointerdown",
    () => {
      keyboard = false;
    },
    true,
  );
}

export function lastInputWasKeyboard(): boolean {
  return keyboard;
}

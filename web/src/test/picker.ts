import { fireEvent, screen, within } from "@testing-library/react";

const escape = (text: string) => text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");

/** The standard picker a form labels `label` (the shared Select, WEB-U-134), in `scope` or the page. */
export function picker(label: string, scope: HTMLElement = document.body) {
  return within(scope).getByRole("button", {
    name: new RegExp(`^${escape(label)}: `),
  });
}

/** The words of the choice the picker labelled `label` shows. */
export function chosen(label: string, scope: HTMLElement = document.body) {
  return (
    picker(label, scope)
      .getAttribute("aria-label")
      ?.slice(label.length + 2) ?? ""
  );
}

/** Opens the picker labelled `label` and chooses the choice whose words start with `option`. */
export function choose(
  label: string,
  option: string | RegExp,
  scope: HTMLElement = document.body,
) {
  fireEvent.click(picker(label, scope));
  fireEvent.click(
    screen.getByRole("menuitemradio", {
      name:
        typeof option === "string" ? new RegExp(`^${escape(option)}`) : option,
    }),
  );
}

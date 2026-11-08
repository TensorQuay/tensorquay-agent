"use client";

import {
  useLayoutEffect,
  useRef,
  type ReactNode,
  type KeyboardEvent,
} from "react";

/** One tab stop per menu, with arrows, boundary keys, activation and bounded typeahead. */
export function Menu({
  label,
  id,
  lines,
  children,
}: {
  label: string;
  id?: string;
  /** "one": each choice on one line, its name cut by an ellipsis before its muted line (WEB-U-432). */
  lines?: "one" | undefined;
  children: ReactNode;
}) {
  const menu = useRef<HTMLDivElement>(null);
  const search = useRef({ text: "", at: 0 });
  useLayoutEffect(() => {
    const root = menu.current;
    (
      root?.querySelector<HTMLElement>('[aria-checked="true"]') ??
      root?.querySelector<HTMLElement>('[role^="menuitem"]')
    )?.focus();
  }, []);
  function keyboard(event: KeyboardEvent) {
    const items = Array.from(
      menu.current?.querySelectorAll<HTMLElement>(
        '[role^="menuitem"]:not(:disabled)',
      ) ?? [],
    );
    if (!items.length) return;
    const current = items.findIndex((item) => item === document.activeElement);
    let next = current;
    switch (event.key) {
      case "ArrowDown":
        next = (current + 1) % items.length;
        break;
      case "ArrowUp":
        next = (current + items.length - 1) % items.length;
        break;
      case "Home":
        next = 0;
        break;
      case "End":
        next = items.length - 1;
        break;
      case "Enter":
      case " ":
        event.preventDefault();
        items[current]?.click();
        return;
      default: {
        if (
          event.key.length !== 1 ||
          event.ctrlKey ||
          event.altKey ||
          event.metaKey
        )
          return;
        const now = performance.now();
        const character = event.key.toLocaleLowerCase();
        const previous =
          now - search.current.at < 500 ? search.current.text : "";
        const text =
          previous === character
            ? character
            : (previous + character).slice(0, 64);
        search.current = { text, at: now };
        const offset = text.length > 1 ? 0 : 1;
        const order = items.map(
          (_, index) =>
            (current + offset + index + items.length) % items.length,
        );
        next =
          order.find((index) =>
            items[index]?.textContent
              ?.trim()
              .toLocaleLowerCase()
              .startsWith(text),
          ) ?? current;
      }
    }
    event.preventDefault();
    items[next]?.focus();
  }
  return (
    <div
      className="menu"
      id={id}
      data-lines={lines}
      tabIndex={-1}
      role="menu"
      aria-label={label}
      ref={menu}
      onKeyDown={keyboard}
    >
      {children}
    </div>
  );
}

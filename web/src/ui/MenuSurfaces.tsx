"use client";

import { Check } from "lucide-react";
import Link from "next/link";
import {
  isValidElement,
  useLayoutEffect,
  useRef,
  useState,
  type ReactNode,
  type RefObject,
} from "react";

import { useT } from "../i18n/I18nProvider";

import type { MenuAction } from "./ActionMenu";
import { Icon } from "./Icon";
import { Menu } from "./Menu";
import type { MenuChoice } from "./MenuButton";
import { Popover } from "./Popover";

/**
 * What draws an open menu, loaded when a menu is about to open (`lazy-menus`), never with a page (WEB-U-014's budget):
 * an action menu's list, a picker's list, and the Popover the account menu opens in.
 */
export { Popover };

/** An open action menu (ActionMenu): a line icon before every item, a muted line under it where it says more. */
export function ActionMenuSurface({
  anchor,
  label,
  id,
  actions,
  onClose,
  onChoose,
}: {
  anchor: RefObject<HTMLButtonElement | null>;
  label: string;
  /** The menu's own id, which its items' lines are named from. */
  id: string;
  actions: readonly MenuAction[];
  onClose: () => void;
  /** An available action chosen: the menu closes, focus goes back to the ⋯, and then it runs. */
  onChoose: (action: MenuAction) => void;
}) {
  return (
    <Popover anchor={anchor} label={label} onClose={onClose}>
      <Menu label={label}>
        {actions.map((action) => {
          const reason = action.unavailable ? `${id}-${action.id}` : undefined;
          const note = action.note ? `${id}-${action.id}-note` : undefined;
          const described =
            [reason, note].filter(Boolean).join(" ") || undefined;
          const content = (
            <>
              <Icon glyph={action.icon} className="menu-icon" />
              <span className="action-menu-word">
                <span>{action.label}</span>
                {action.unavailable && (
                  <span id={reason} className="type-small muted">
                    {action.unavailable}
                  </span>
                )}
                {action.note && (
                  <span id={note} className="type-small muted">
                    {action.note}
                  </span>
                )}
              </span>
              {action.checked && <Icon glyph={Check} className="menu-check" />}
            </>
          );
          const shared = {
            role:
              action.checked === undefined ? "menuitem" : "menuitemcheckbox",
            "aria-checked": action.checked,
            tabIndex: -1,
            className: "menu-item action-menu-item",
            "aria-label": action.name ?? action.label,
            "aria-disabled": action.unavailable ? true : undefined,
            "aria-describedby": described,
            "data-danger": action.danger || undefined,
          } as const;
          return action.href && !action.unavailable ? (
            <Link
              key={action.id}
              href={action.href}
              {...shared}
              onClick={onClose}
            >
              {content}
            </Link>
          ) : (
            <button
              key={action.id}
              type="button"
              {...shared}
              onClick={() => {
                if (!action.unavailable) onChoose(action);
              }}
            >
              {content}
            </button>
          );
        })}
      </Menu>
    </Popover>
  );
}

/** The words of a choice's line, for search: its text, however it is wrapped. */
function words(node: ReactNode): string {
  if (typeof node === "string" || typeof node === "number") return String(node);
  if (Array.isArray(node)) return node.map(words).join(" ");
  if (isValidElement<{ children?: ReactNode }>(node))
    return words(node.props.children);
  return "";
}

/** An open picker's list (MenuButton): its choices, the current one checked, a search field over a long list. */
export function PickerSurface({
  anchor,
  id,
  label,
  title,
  placement,
  options,
  value,
  searchable,
  oneLine,
  rowDecorations,
  onClose,
  onChoose,
}: {
  anchor: RefObject<HTMLButtonElement | null>;
  /** The list's id, which its trigger controls. */
  id: string;
  label: string;
  /** The current choice's name over the list, where the trigger shows only its icon (WEB-U-106). */
  title: string | null;
  placement: "below-start" | "below-end" | "above-start";
  options: readonly MenuChoice[];
  value: string;
  searchable: boolean;
  oneLine: boolean;
  rowDecorations: boolean;
  /** Closed without a choice (Esc or a click elsewhere). */
  onClose: () => void;
  /** A choice made (one that says why it can't be chosen does nothing). */
  onChoose: (option: MenuChoice) => void;
}) {
  const t = useT();
  const [query, setQuery] = useState("");
  const find = useRef<HTMLInputElement>(null);
  const needle = query.trim().toLocaleLowerCase();
  const shown = needle
    ? options.filter((option) =>
        `${option.label} ${words(option.detail)}`
          .toLocaleLowerCase()
          .includes(needle),
      )
    : options;
  // With a search field, typing starts there (after the menu has focused its current row).
  useLayoutEffect(() => {
    if (searchable) find.current?.focus();
  }, [searchable]);
  /** The shown choices by group, in their first appearance's order, when any has one. */
  const groups = shown.some((option) => option.group)
    ? [...new Set(shown.map((option) => option.group ?? ""))].map(
        (group) =>
          [
            group,
            shown.filter((option) => (option.group ?? "") === group),
          ] as const,
      )
    : null;
  /** One choice's row in the menu. */
  const row = (option: MenuChoice) => (
    <button
      key={option.id}
      type="button"
      role="menuitemradio"
      tabIndex={-1}
      aria-checked={option.id === value}
      aria-disabled={option.disabled ? true : undefined}
      className="menu-item"
      onClick={() => onChoose(option)}
    >
      <span className="menu-choice">
        <span lang={option.lang}>{option.label}</span>
        {/* A word break for the row's name (label, then its line); the column shows none. */}
        {option.disabled || option.detail ? " " : null}
        {option.disabled ? (
          <span className="type-small muted">{option.disabled}</span>
        ) : (
          option.detail
        )}
      </span>
      {rowDecorations && option.decoration}
      <span className="menu-check">
        {option.id === value && <Icon glyph={Check} size={16} />}
      </span>
    </button>
  );
  return (
    <Popover
      anchor={anchor}
      label={t("menu.choices", { label })}
      placement={placement}
      onClose={onClose}
    >
      {title && (
        <p className="picker-title type-small muted" aria-hidden="true">
          {title}
        </p>
      )}
      {searchable && (
        <div className="menu-find">
          <input
            ref={find}
            type="search"
            className="form-control menu-search"
            aria-label={t("menu.find")}
            placeholder={t("menu.findPlaceholder")}
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "ArrowDown") {
                event.preventDefault();
                document
                  .getElementById(id)
                  ?.querySelector<HTMLElement>('[role="menuitemradio"]')
                  ?.focus();
              } else if (event.key === "Enter") {
                event.preventDefault();
                const first = shown.find((option) => !option.disabled);
                if (first) onChoose(first);
              }
            }}
          />
        </div>
      )}
      {shown.length === 0 && (
        <p className="menu-empty type-small muted">{t("menu.noMatch")}</p>
      )}
      <Menu label={label} id={id} lines={oneLine ? "one" : undefined}>
        {groups
          ? groups.map(([group, members], index) => (
              <div
                key={group}
                role="group"
                aria-labelledby={`${id}-group-${index}`}
                className="menu-group"
              >
                <p
                  id={`${id}-group-${index}`}
                  className="menu-group-label type-label muted"
                >
                  {group}
                </p>
                {members.map(row)}
              </div>
            ))
          : shown.map(row)}
      </Menu>
    </Popover>
  );
}

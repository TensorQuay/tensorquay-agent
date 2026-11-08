"use client";

import { Check, Ellipsis, type LucideIcon } from "lucide-react";
import Link from "next/link";
import {
  useEffect,
  useId,
  useImperativeHandle,
  useRef,
  useState,
  type Ref,
  type RefObject,
} from "react";

import { Icon } from "./Icon";
import { IconAction } from "./IconAction";
import { Menu } from "./Menu";
import { Popover } from "./Popover";

/** One action on the thing a menu belongs to. */
export type MenuAction = {
  id: string;
  /** Its short word, as the menu shows it. */
  label: string;
  /** Its full accessible name when the word alone is ambiguous out of context ("Delete the key for GitHub"). */
  name?: string | undefined;
  /** The line icon before the word (the UI standard: one before every menu item). */
  icon: LucideIcon;
  danger?: boolean | undefined;
  /** Why the action isn't possible now: the item stays reachable by keyboard and says so (UI-GUIDELINES §5b). */
  unavailable?: string | undefined;
  /** A setting turned on and off from the menu: a checkbox item, marked while on. */
  checked?: boolean | undefined;
  /** One muted line under the word on what the item means, such as what a setting costs. */
  note?: string | undefined;
} & (
  { onSelect: () => void; href?: never } | { href: string; onSelect?: never }
);

/**
 * The UI standard's Menu: actions on one thing, from its ⋯ or a right-click on it (`context`), a line icon before every
 * item. It closes after a choice, on Esc or on a click elsewhere, and focus goes back to the ⋯ before the action runs,
 * so a dialog the action opens returns focus there. Every action menu is this one; a screen never builds its own.
 */
export function ActionMenu({
  label,
  actions,
  context,
  className,
  glyph = Ellipsis,
  tip,
  round = false,
  onOpen,
  ref,
}: {
  /** The ⋯'s and the menu's name, after the thing: "More actions for Quarterly report". */
  label: string;
  actions: readonly MenuAction[];
  /** The thing the menu acts on; a right-click on it opens the menu (at the ⋯). */
  context?: RefObject<HTMLElement | null> | undefined;
  /** The ⋯'s own class, where its place needs one (a sidebar row shows it on hover). */
  className?: string | undefined;
  /** The trigger's icon: ⋯ by default; the composer's "+" (Add) for what a new task can use. */
  glyph?: LucideIcon;
  /** The trigger's tooltip, where it says more than the name (what "+" offers); the name otherwise. */
  tip?: string | undefined;
  /** The composer bar's round trigger (IconAction's `round`). */
  round?: boolean;
  /** Called as the menu opens. */
  onOpen?: (() => void) | undefined;
  /** The ⋯ itself, for a caller that returns focus to it. */
  ref?: Ref<HTMLButtonElement | null> | undefined;
}) {
  const [open, setOpen] = useState(false);
  const anchor = useRef<HTMLButtonElement>(null);
  useImperativeHandle(ref, () => anchor.current as HTMLButtonElement);
  const id = useId();
  const show = () => {
    onOpen?.();
    setOpen(true);
  };
  const latest = useRef(show);
  useEffect(() => {
    latest.current = show;
  });
  useEffect(() => {
    const element = context?.current;
    if (!element) return;
    function contextMenu(event: MouseEvent) {
      event.preventDefault();
      latest.current();
    }
    element.addEventListener("contextmenu", contextMenu);
    return () => element.removeEventListener("contextmenu", contextMenu);
  }, [context]);
  const choose = (action: MenuAction) => {
    setOpen(false);
    anchor.current?.focus();
    action.onSelect?.();
  };
  return (
    <>
      <IconAction
        ref={anchor}
        glyph={glyph}
        name={label}
        tip={tip ?? label}
        className={className}
        round={round}
        expanded={open}
        onClick={() => (open ? setOpen(false) : show())}
      />
      {open && (
        <Popover anchor={anchor} label={label} onClose={() => setOpen(false)}>
          <Menu label={label}>
            {actions.map((action) => {
              const reason = action.unavailable
                ? `${id}-${action.id}`
                : undefined;
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
                  {action.checked && (
                    <Icon glyph={Check} className="menu-check" />
                  )}
                </>
              );
              const shared = {
                role:
                  action.checked === undefined
                    ? "menuitem"
                    : "menuitemcheckbox",
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
                  onClick={() => setOpen(false)}
                >
                  {content}
                </Link>
              ) : (
                <button
                  key={action.id}
                  type="button"
                  {...shared}
                  onClick={() => {
                    if (!action.unavailable) choose(action);
                  }}
                >
                  {content}
                </button>
              );
            })}
          </Menu>
        </Popover>
      )}
    </>
  );
}

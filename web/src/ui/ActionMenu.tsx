"use client";

import { Ellipsis, type LucideIcon } from "lucide-react";
import {
  useEffect,
  useId,
  useImperativeHandle,
  useRef,
  useState,
  type Ref,
  type RefObject,
} from "react";

import { IconAction } from "./IconAction";
import { prefetchMenus, useMenus } from "./lazy-menus";

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
  // Its open list's code comes when it is about to open (WEB-U-014's budget); a menu that can't get it closes.
  const menus = useMenus(open, () => setOpen(false));
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
    // A right-click comes with the pointer over the thing: its menu's code is fetched then.
    element.addEventListener("pointerenter", prefetchMenus);
    return () => {
      element.removeEventListener("contextmenu", contextMenu);
      element.removeEventListener("pointerenter", prefetchMenus);
    };
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
        onIntent={prefetchMenus}
      />
      {open && menus && (
        <menus.ActionMenuSurface
          anchor={anchor}
          label={label}
          id={id}
          actions={actions}
          onClose={() => setOpen(false)}
          onChoose={choose}
        />
      )}
    </>
  );
}

"use client";

import type { LucideIcon } from "lucide-react";
import {
  useLayoutEffect,
  useRef,
  useState,
  useSyncExternalStore,
  type MouseEvent,
  type Ref,
} from "react";
import { createPortal } from "react-dom";

import { Button, type ButtonVariant } from "./Button";
import { Icon } from "./Icon";

/**
 * An icon-only action named after its object ("Fork Sales analysis"), with a short visible tooltip on hover and on
 * keyboard focus (mockup change 6). The tooltip repeats the name, so assistive technology reads the name only. It is
 * placed where it fits: centred on the button, moved along to stay inside the window and the nearest
 * `[data-tip-bounds]` panel, above the button unless there is no room there; and never over the button's open menu.
 * It is a layer of its own on the page (in the button's dialog when it is in one, as Popover), never the button's
 * content, so the button's box stays its own size (WEB-U-429).
 * The composer bar's + and Send are the same button, `round` (36 px, a 20 px icon), in their variant and fill
 * (UI-GUIDELINES §7 Composer, WEB-U-428).
 */
export function IconAction({
  glyph,
  name,
  tip,
  onClick,
  expanded,
  controls,
  popup = true,
  className,
  id,
  ref,
  onIntent,
  round = false,
  variant = "ghost",
  type = "button",
  disabled,
  ariaDisabled,
  loading = false,
  fill,
  describedBy,
  onFocusChange,
}: {
  glyph: LucideIcon;
  name: string;
  tip: string;
  onClick: (event: MouseEvent<HTMLButtonElement>) => void;
  expanded?: boolean;
  /** The id of what it opens (a panel it shows and hides). */
  controls?: string | undefined;
  /** Whether what it opens is a menu (the default when `expanded` is given); a panel's toggle isn't. */
  popup?: boolean;
  /** The button's id, for a caller that returns focus to it. */
  id?: string | undefined;
  /** The action's own class, where its place needs one (a sidebar row shows its ⋯ on hover). */
  className?: string | undefined;
  ref?: Ref<HTMLButtonElement>;
  /** The person is about to use it (pointer over or down, focus): a chance to fetch what it opens. */
  onIntent?: (() => void) | undefined;
  /** The composer bar's disc: 36 px round with a 20 px icon; otherwise the 36 px square with a 16 px icon. */
  round?: boolean;
  variant?: ButtonVariant;
  /** A form's submit (the composer's Send), or a plain button. */
  type?: "button" | "submit";
  disabled?: boolean | undefined;
  /** Unavailable while it keeps the focus it has (WEB-U-273), instead of a native disabled that would drop it. */
  ariaDisabled?: boolean | undefined;
  /** Busy: the spinner replaces the icon and the button refuses activation, keeping its name and focus. */
  loading?: boolean;
  /** Send's look: `ink` (filled) while it can send, `outline` (an outline disc, nothing filled) while it can't. */
  fill?: "ink" | "outline" | undefined;
  /** The id of what describes it (the attach limits, Stop's wider reach). */
  describedBy?: string | undefined;
  /** Told whether the button has the focus, by any means (a blocked Send keeps it, WEB-U-273). */
  onFocusChange?: ((focused: boolean) => void) | undefined;
}) {
  const [hover, setHover] = useState(false);
  const [focus, setFocus] = useState(false);
  const holder = useRef<HTMLSpanElement>(null);
  const bubble = useRef<HTMLSpanElement>(null);
  const shown = (hover || focus) && !expanded;
  // Where the tip renders: the button's dialog, else the page; read again once the button has mounted.
  const host = useSyncExternalStore(
    noSubscription,
    () => holder.current?.closest("dialog") ?? document.body,
    () => null,
  );
  useLayoutEffect(() => {
    const own = holder.current;
    const tooltip = bubble.current;
    if (!shown || !own || !tooltip) return;
    // In the window's coordinates, as the button is now.
    const place = () => {
      const button = own.getBoundingClientRect();
      const width = tooltip.offsetWidth;
      const height = tooltip.offsetHeight;
      // The token gap; an environment without the stylesheet (tests) places it flush.
      const gap =
        parseFloat(getComputedStyle(tooltip).getPropertyValue("--space-1")) ||
        0;
      const panel = own.closest("[data-tip-bounds]")?.getBoundingClientRect();
      const low = Math.max(gap, panel ? panel.left + gap : gap);
      const high = Math.min(
        innerWidth - gap,
        panel ? panel.right - gap : innerWidth - gap,
      );
      const centred = button.left + button.width / 2 - width / 2;
      tooltip.style.left = `${Math.max(low, Math.min(centred, high - width))}px`;
      const above = button.top - height - gap >= 0;
      tooltip.style.top = `${above ? button.top - height - gap : button.bottom + gap}px`;
    };
    place();
    // A browser may send no leave: after a Send clicked with the pointer left on it through Sending, the run and Send
    // again, none came (WEB-U-429). A pointer move anywhere outside the button ends the tip the pointer brought.
    const moved = (event: PointerEvent) => {
      if (!(event.target instanceof Node) || !own.contains(event.target))
        setHover(false);
    };
    addEventListener("scroll", place, true);
    addEventListener("resize", place);
    addEventListener("pointermove", moved);
    return () => {
      removeEventListener("scroll", place, true);
      removeEventListener("resize", place);
      removeEventListener("pointermove", moved);
    };
  }, [shown, tip]);
  return (
    <span
      ref={holder}
      className={["icon-action", className].filter(Boolean).join(" ")}
      onPointerEnter={() => {
        setHover(true);
        onIntent?.();
      }}
      onPointerDown={onIntent}
      onPointerLeave={() => setHover(false)}
      onFocus={(event) => {
        setFocus(event.target.matches(":focus-visible"));
        onFocusChange?.(true);
        onIntent?.();
      }}
      onBlur={() => {
        setFocus(false);
        onFocusChange?.(false);
      }}
    >
      <Button
        ref={ref}
        variant={variant}
        className="icon-button"
        id={id}
        type={type}
        data-shape={round ? "round" : undefined}
        data-fill={fill}
        disabled={disabled}
        aria-disabled={ariaDisabled || undefined}
        loading={loading}
        aria-label={name}
        aria-describedby={describedBy}
        aria-haspopup={expanded === undefined || !popup ? undefined : "menu"}
        aria-expanded={expanded}
        aria-controls={controls}
        onClick={onClick}
      >
        <Icon glyph={glyph} size={round ? 20 : 16} stroke={round ? 2 : 1.5} />
      </Button>
      {shown &&
        host &&
        createPortal(
          <span
            ref={bubble}
            className="action-tooltip type-small"
            aria-hidden="true"
          >
            {tip}
          </span>,
          host,
        )}
    </span>
  );
}

function noSubscription() {
  return () => {};
}

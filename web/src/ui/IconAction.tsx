"use client";

import type { LucideIcon } from "lucide-react";
import { useLayoutEffect, useRef, useState, type Ref } from "react";

import { Button } from "./Button";
import { Icon } from "./Icon";

/**
 * An icon-only action named after its object ("Fork Sales analysis"), with a short visible tooltip on hover and on
 * keyboard focus (mockup change 6). The tooltip repeats the name, so assistive technology reads the name only. It is
 * placed where it fits: centred on the button, moved along to stay inside the window and the nearest
 * `[data-tip-bounds]` panel, above the button unless there is no room there; and never over the button's open menu.
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
}: {
  glyph: LucideIcon;
  name: string;
  tip: string;
  onClick: () => void;
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
}) {
  const [hover, setHover] = useState(false);
  const [focus, setFocus] = useState(false);
  const holder = useRef<HTMLSpanElement>(null);
  const bubble = useRef<HTMLSpanElement>(null);
  const shown = (hover || focus) && !expanded;
  useLayoutEffect(() => {
    const own = holder.current;
    const tooltip = bubble.current;
    if (!shown || !own || !tooltip) return;
    const button = own.getBoundingClientRect();
    const width = tooltip.offsetWidth;
    const height = tooltip.offsetHeight;
    // The token gap; an environment without the stylesheet (tests) places it flush.
    const gap =
      parseFloat(getComputedStyle(tooltip).getPropertyValue("--space-1")) || 0;
    const panel = own.closest("[data-tip-bounds]")?.getBoundingClientRect();
    const low = Math.max(gap, panel ? panel.left + gap : gap);
    const high = Math.min(
      innerWidth - gap,
      panel ? panel.right - gap : innerWidth - gap,
    );
    const centred = button.left + button.width / 2 - width / 2;
    const left = Math.max(low, Math.min(centred, high - width));
    tooltip.style.left = `${left - button.left}px`;
    const above = button.top - height - gap >= 0;
    tooltip.style.top = above
      ? `${-height - gap}px`
      : `${button.height + gap}px`;
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
        onIntent?.();
      }}
      onBlur={() => setFocus(false)}
    >
      <Button
        ref={ref}
        variant="ghost"
        className="icon-button"
        id={id}
        aria-label={name}
        aria-haspopup={expanded === undefined || !popup ? undefined : "menu"}
        aria-expanded={expanded}
        aria-controls={controls}
        onClick={onClick}
      >
        <Icon glyph={glyph} />
      </Button>
      {shown && (
        <span
          ref={bubble}
          className="action-tooltip type-small"
          aria-hidden="true"
        >
          {tip}
        </span>
      )}
    </span>
  );
}

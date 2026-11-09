"use client";

import { useRef, type KeyboardEvent } from "react";

/** One view a tab list offers: its value and its words. */
export interface TabView<T extends string> {
  value: T;
  label: string;
}

/** A tab panel's id and its label, named from the tab list's `id` as its tabs name the panel they control. */
export function tabPanelIds(id: string, value: string) {
  return {
    id: `${id}-${value}-panel`,
    "aria-labelledby": `${id}-${value}-tab`,
  } as const;
}

/**
 * The one tab list (UI-GUIDELINES §7 Tabs): a few views of one place, the chosen one a soft filled pill, the others
 * plain muted labels. WAI-ARIA tabs that choose as the focus moves: the chosen tab is the list's one Tab stop; the
 * arrow keys choose the next or previous tab and wrap, Home and End the first and last. A list wider than its place
 * scrolls sideways inside itself.
 */
export function TabList<T extends string>({
  id,
  label,
  views,
  current,
  onChoose,
  className,
}: {
  /** What the tabs' ids and their panels' ids (`tabPanelIds`) are made from. */
  id: string;
  /** What the views are of, for assistive technology ("Task views"). */
  label: string;
  views: readonly TabView<T>[];
  current: T;
  onChoose: (value: T) => void;
  /** The place's own layout (its width in a heading), never the tabs' look. */
  className?: string | undefined;
}) {
  const tabs = useRef<(HTMLButtonElement | null)[]>([]);
  // The chosen tab is the Tab stop, or the first when none of them is chosen, so the list is never out of reach.
  const stop = Math.max(
    0,
    views.findIndex((view) => view.value === current),
  );
  function choose(event: KeyboardEvent, index: number) {
    const last = views.length - 1;
    const targets: Record<string, number> = {
      ArrowRight: index === last ? 0 : index + 1,
      ArrowLeft: index === 0 ? last : index - 1,
      Home: 0,
      End: last,
    };
    const to = targets[event.key];
    const view = to === undefined ? undefined : views[to];
    if (to === undefined || !view) return;
    // The page stays still: Home and End would scroll it, an arrow the strip.
    event.preventDefault();
    onChoose(view.value);
    tabs.current[to]?.focus();
  }
  return (
    <div
      role="tablist"
      aria-label={label}
      className={className ? `tab-list ${className}` : "tab-list"}
    >
      {views.map((view, index) => (
        <button
          key={view.value}
          ref={(node) => {
            tabs.current[index] = node;
          }}
          type="button"
          role="tab"
          id={`${id}-${view.value}-tab`}
          aria-controls={tabPanelIds(id, view.value).id}
          aria-selected={view.value === current}
          tabIndex={index === stop ? 0 : -1}
          className="tab"
          onClick={() => onChoose(view.value)}
          onKeyDown={(event) => choose(event, index)}
        >
          {view.label}
        </button>
      ))}
    </div>
  );
}

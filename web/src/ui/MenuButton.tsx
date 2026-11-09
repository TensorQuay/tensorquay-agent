"use client";

import { ChevronDown, type LucideIcon } from "lucide-react";
import { useId, useRef, useState, type ReactNode } from "react";

import { useT } from "../i18n/I18nProvider";

import { Button } from "./Button";
import { Icon } from "./Icon";
import { prefetchMenus, useMenus } from "./lazy-menus";

/** Identity stays separate from the human label; decorations may show declared capabilities. */
export type MenuChoice = {
  id: string;
  label: string;
  decoration?: ReactNode;
  detail?: ReactNode;
  /** Why it can't be chosen now, shown as its line (the UI standard: a disabled choice says why). */
  disabled?: string | undefined;
  /** The language its label is in (a language picker's own names), for assistive technology. */
  lang?: string | undefined;
  /** The named group it is listed under (the Settings sections on a phone); choices of one group come together. */
  group?: string | undefined;
};

/** More choices than this get a search field (the UI standard's Popover). */
export const SEARCH_FROM = 8;

/** A focusable picker whose disabled explanation appears on hover or focus, never in the transcript. */
export function MenuButton({
  label,
  options,
  value,
  onChange,
  disabled = false,
  disabledReason,
  fallback,
  placement = "below-start",
  defaultOpen = false,
  onDismiss,
  placeholder,
  icon,
  field = false,
  invalid = false,
  describedBy,
  buttonId,
  oneLine = false,
  rowDecorations = true,
}: {
  label: string;
  options: readonly MenuChoice[];
  value: string;
  onChange: (value: string) => void;
  disabled?: boolean;
  disabledReason?: string | undefined;
  fallback?: string;
  /** Where its choices open: below (the default), below aligned to its end (the header's), or above. */
  placement?: "below-start" | "below-end" | "above-start";
  /** Open at once (a chip New task's "+" just added). */
  defaultOpen?: boolean;
  /** Closed without a choice (Esc or a click elsewhere): the chip that opened it goes away. */
  onDismiss?: (() => void) | undefined;
  /** What it says while nothing is chosen yet; it opens all the same (a picker for a new thing's value). */
  placeholder?: string;
  /**
   * What it is, as an icon: where the stylesheet has no room for the chosen name (a narrow task header) it shows this
   * instead, never a cut name; the name is then in its tooltip and, while the icon shows, its menu's title (WEB-U-106).
   */
  icon?: LucideIcon;
  /** In a form (the shared Select, WEB-U-134): the trigger looks like a field, its label shown above it. */
  field?: boolean;
  /** A form refused its value: marked for FormRefusal and assistive technology. */
  invalid?: boolean;
  /** The ids of its hint and error, read after its name. */
  describedBy?: string | undefined;
  /** Its button's id, for what focuses it (a check's suggested setting, WEB-U-058). */
  buttonId?: string | undefined;
  /** Each choice on one line: its name cut by an ellipsis, then its muted line (the Changes panel's runs, WEB-U-432). */
  oneLine?: boolean;
  /** Whether a choice's decoration shows on its row as well as on the button (a run's Latest: the button only, once). */
  rowDecorations?: boolean;
}) {
  const t = useT();
  const [expanded, setExpanded] = useState(defaultOpen);
  // Whether its icon stands for its name as it opens (the stylesheet decides where): its menu then names the choice.
  const [iconic, setIconic] = useState(false);
  const anchor = useRef<HTMLButtonElement>(null);
  const id = useId();
  const current = options.find((option) => option.id === value);
  const unavailable =
    disabled ||
    Boolean(disabledReason) ||
    (!current && placeholder === undefined);
  const open = expanded && !unavailable;
  // Its list's code comes when it is about to open (WEB-U-014's budget); a picker that can't get it closes.
  const menus = useMenus(open, setExpanded, anchor);
  const shownName =
    current?.label ?? placeholder ?? fallback ?? t("menu.unavailable");
  const name = t("menu.current", { label, value: shownName });
  function choose(option: MenuChoice) {
    if (option.disabled) return;
    if (option.id !== value) onChange(option.id);
    setExpanded(false);
    anchor.current?.focus();
  }
  return (
    <div className="menu-picker">
      <Button
        ref={anchor}
        id={buttonId}
        variant="ghost"
        className={field ? "picker-button picker-field" : "picker-button"}
        aria-label={name}
        aria-haspopup="menu"
        aria-expanded={open}
        aria-controls={open && menus ? id : undefined}
        aria-disabled={unavailable || undefined}
        aria-invalid={invalid || undefined}
        aria-describedby={
          [disabledReason ? `${id}-reason` : undefined, describedBy]
            .filter(Boolean)
            .join(" ") || undefined
        }
        onPointerEnter={prefetchMenus}
        onFocus={prefetchMenus}
        onClick={() => {
          if (unavailable) return;
          if (!open) setIconic(showsIcon(anchor.current));
          setExpanded(!open);
        }}
        onKeyDown={(event) => {
          if (!unavailable && ["ArrowDown", "ArrowUp"].includes(event.key)) {
            event.preventDefault();
            setIconic(showsIcon(anchor.current));
            setExpanded(true);
          }
        }}
      >
        {icon && <Icon glyph={icon} size={16} className="picker-icon" />}
        <span className="picker-name">{shownName}</span>
        {current?.decoration}
        <Icon glyph={ChevronDown} size={16} />
      </Button>
      {(disabledReason || icon) && (
        <span
          className={[
            "picker-tooltip type-small",
            disabledReason ? null : "picker-tooltip-name",
          ]
            .filter(Boolean)
            .join(" ")}
        >
          {/* It repeats the button's name, so assistive technology reads it once. */}
          {icon && (
            <span className="picker-name-tip" aria-hidden="true">
              {name}
            </span>
          )}
          {disabledReason && (
            <span role="tooltip" id={`${id}-reason`}>
              {disabledReason}
            </span>
          )}
        </span>
      )}
      {open && menus && (
        <menus.PickerSurface
          anchor={anchor}
          id={id}
          label={label}
          title={icon && iconic ? name : null}
          placement={placement}
          options={options}
          value={value}
          searchable={options.length >= SEARCH_FROM}
          oneLine={oneLine}
          rowDecorations={rowDecorations}
          onClose={() => {
            setExpanded(false);
            onDismiss?.();
          }}
          onChoose={choose}
        />
      )}
    </div>
  );
}

/** Whether a picker's icon is showing in place of its name (the stylesheet hides one or the other). */
function showsIcon(button: HTMLElement | null): boolean {
  const icon = button?.querySelector(".picker-icon");
  const name = button?.querySelector<HTMLElement>(".picker-name");
  if (!icon || !name) return false;
  return getComputedStyle(name).display === "none";
}

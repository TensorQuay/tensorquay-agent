import { LoaderCircle } from "lucide-react";
import type { ComponentPropsWithRef } from "react";

import { Icon } from "./Icon";

/** The supported semantic button treatments. */
export type ButtonVariant = "primary" | "secondary" | "ghost" | "danger";
type Props = ComponentPropsWithRef<"button"> & {
  variant?: ButtonVariant;
  loading?: boolean;
};

/** A native button whose loading state preserves its name and width. */
export function Button({
  variant = "primary",
  loading = false,
  disabled,
  children,
  className = "",
  type = "button",
  onClick,
  ...props
}: Props) {
  // Loading keeps the button focusable (a native `disabled` would drop its focus to the page, WEB-U-007) and refuses
  // activation instead, including the form submission that Enter in a field sends through it.
  const busy = loading && !disabled;
  return (
    <button
      {...props}
      type={type}
      className={`button ${className}`}
      data-variant={variant}
      disabled={disabled}
      aria-disabled={busy || props["aria-disabled"]}
      aria-busy={loading || undefined}
      onClick={(event) => {
        if (busy) {
          event.preventDefault();
          return;
        }
        onClick?.(event);
      }}
    >
      <span className="button-label" data-loading={loading}>
        {children}
      </span>
      {loading && (
        <span className="button-spinner">
          <Icon glyph={LoaderCircle} size={16} />
        </span>
      )}
    </button>
  );
}

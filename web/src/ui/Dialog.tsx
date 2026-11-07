"use client";

import { X } from "lucide-react";
import {
  useEffect,
  useId,
  useRef,
  type ReactNode,
  type RefObject,
} from "react";

import { useT } from "../i18n/I18nProvider";

import { Button } from "./Button";
import { trapDialogFocus } from "./dialog-focus";
import { Icon } from "./Icon";
import "../i18n/deferred-catalogue";

/** A controlled native modal with keyboard focus containment and trigger restoration. */
export function Dialog({
  open,
  onClose,
  onCancel,
  title,
  children,
  actions,
  preview = false,
  className = "",
  initialFocus,
  placement,
}: {
  open: boolean;
  onClose: () => void;
  /** Escape (the native cancel), when it means something else than the close button; `onClose` by default. */
  onCancel?: () => void;
  title: string;
  children: ReactNode;
  actions?: ReactNode;
  preview?: boolean;
  className?: string;
  /** Focused once the dialog opens, instead of its first control (a search overlay's box). */
  initialFocus?: RefObject<HTMLElement | null>;
  /**
   * At an edge, the dialog is a sheet (UI-GUIDELINES' Side sheet, WEB-U-230): full height, sliding in from that edge.
   * The navigation drawer is one at the start; a side sheet (All helpers) at the end. Without it, the middle.
   */
  placement?: "start" | "end" | undefined;
}) {
  const t = useT();
  const ref = useRef<HTMLDialogElement>(null);
  const titleId = useId();
  useEffect(() => {
    const dialog = ref.current;
    if (!open || !dialog || preview) return;
    const trigger = document.activeElement;
    dialog.showModal();
    initialFocus?.current?.focus();
    return () => {
      dialog.close();
      if (trigger instanceof HTMLElement && trigger.isConnected)
        trigger.focus();
    };
  }, [open, preview, initialFocus]);
  return (
    <dialog
      ref={ref}
      open={preview ? open : undefined}
      className={`dialog ${placement ? "dialog-sheet" : ""} ${preview ? "dialog-preview" : ""} ${className}`}
      data-placement={placement}
      aria-labelledby={titleId}
      onKeyDown={preview ? undefined : trapDialogFocus}
      onCancel={(event) => {
        event.preventDefault();
        (onCancel ?? onClose)();
      }}
    >
      <div className="dialog-header">
        <h2 id={titleId} className="type-heading">
          {title}
        </h2>
        <Button
          variant="ghost"
          className="icon-button"
          aria-label={t("dialog.close")}
          onClick={onClose}
        >
          <Icon glyph={X} />
        </Button>
      </div>
      <div className="dialog-content">{children}</div>
      {actions && <div className="dialog-actions">{actions}</div>}
    </dialog>
  );
}

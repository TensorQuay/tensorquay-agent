"use client";

import { Check, ChevronDown } from "lucide-react";
import { useId, useLayoutEffect, useRef, useState } from "react";

import { useT } from "../i18n/I18nProvider";
import type { Workspace } from "../protocol/generated/api";
import { Button } from "../ui/Button";
import { Icon } from "../ui/Icon";
import { Menu } from "../ui/Menu";
import { Popover } from "../ui/Popover";

import {
  matches,
  pickable,
  recentFirst,
  workspaceName,
} from "./workspace-state";
import "../i18n/deferred-catalogue";

/** Recent workspaces shown without a search; a search field appears once there are more. */
const RECENT = 8;
/** Matches shown for a search. */
const MATCHES = 20;

/**
 * The workspace choice in the task bar (task 076): "New workspace" (the value "") first, then recent workspaces with
 * their owner and state. A read-only share or a fork being copied is listed but disabled, with the reason (Q7).
 */
export function WorkspacePicker({
  workspaces,
  value,
  onChange,
  disabled = false,
  describedBy,
  defaultOpen = false,
  onDismiss,
  placement,
}: {
  workspaces: readonly Workspace[] | undefined;
  value: string;
  onChange: (id: string) => void;
  disabled?: boolean;
  /** The id of a hint that says what a workspace is, where the picker needs one (finding N2). */
  describedBy?: string;
  /** Open at once (a chip New task's "+" just added). */
  defaultOpen?: boolean;
  /** Closed without a choice (Esc or a click elsewhere): the chip that opened it goes away. */
  onDismiss?: (() => void) | undefined;
  /** Where the choices open (the composer's open above). */
  placement?: "below-start" | "above-start" | undefined;
}) {
  const t = useT();
  const [expanded, setExpanded] = useState(defaultOpen);
  const [query, setQuery] = useState("");
  const anchor = useRef<HTMLButtonElement>(null);
  const search = useRef<HTMLInputElement>(null);
  const id = useId();
  const all = (workspaces ?? []).filter(
    (workspace) => workspace.state !== "deleted",
  );
  const current = all.find((workspace) => workspace.id === value);
  const label = current ? workspaceName(current, t) : t("workspaces.new");
  const searching = all.length > RECENT;
  const shown = [...all]
    .sort(recentFirst)
    .filter((workspace) => matches(workspace, query, t))
    .slice(0, query.trim() ? MATCHES : RECENT);
  const open = expanded && !disabled;
  function close() {
    setExpanded(false);
    setQuery("");
    anchor.current?.focus();
  }
  // The menu focuses its checked item as it mounts; with a search field, typing comes first.
  useLayoutEffect(() => {
    if (open && searching) search.current?.focus();
  }, [open, searching]);
  const detail = (workspace: Workspace) => {
    const parts = [
      workspace.access === "owner"
        ? t("workspaces.you")
        : workspace.owner.email,
    ];
    const reason = pickable(workspace);
    if (reason === "viewOnly") parts.push(t("workspaces.viewOnly"));
    else if (reason === "copying") parts.push(t("workspaces.beingCopied"));
    else if (reason === "failed") parts.push(t("workspaces.copyFailedShort"));
    else if (workspace.lease) parts.push(t("workspaces.inUse"));
    return parts.join(" · ");
  };
  return (
    <div className="menu-picker workspace-picker">
      <Button
        ref={anchor}
        variant="ghost"
        className="picker-button"
        aria-label={t("menu.current", {
          label: t("picker.workspace"),
          value: label,
        })}
        aria-haspopup="menu"
        aria-expanded={open}
        aria-controls={open ? id : undefined}
        aria-disabled={disabled || undefined}
        aria-describedby={describedBy}
        data-busy={current?.lease ? true : undefined}
        onClick={() => {
          if (!disabled) setExpanded(!open);
        }}
        onKeyDown={(event) => {
          if (!disabled && ["ArrowDown", "ArrowUp"].includes(event.key)) {
            event.preventDefault();
            setExpanded(true);
          }
        }}
      >
        {current?.lease && (
          <span className="lease-dot" data-lease="held" aria-hidden="true" />
        )}
        <span className="picker-name">{label}</span>
        <Icon glyph={ChevronDown} size={16} />
      </Button>
      {open && (
        <Popover
          anchor={anchor}
          label={t("menu.choices", { label: t("picker.workspace") })}
          {...(placement ? { placement } : {})}
          onClose={() => {
            setExpanded(false);
            setQuery("");
            onDismiss?.();
          }}
        >
          {searching && (
            <input
              ref={search}
              type="search"
              className="form-control workspace-picker-search"
              aria-label={t("workspaces.search")}
              placeholder={t("workspaces.search")}
              autoComplete="off"
              value={query}
              onChange={(event) => setQuery(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === "ArrowDown") {
                  event.preventDefault();
                  document
                    .getElementById(id)
                    ?.querySelector<HTMLElement>(
                      '[role^="menuitem"]:not(:disabled)',
                    )
                    ?.focus();
                }
              }}
            />
          )}
          <Menu label={t("picker.workspace")} id={id}>
            {[null, ...shown].map((workspace) => {
              const choice = workspace?.id ?? "";
              const reason = workspace ? pickable(workspace) : "ok";
              return (
                <button
                  key={choice || "new"}
                  type="button"
                  role="menuitemradio"
                  tabIndex={-1}
                  aria-checked={choice === value}
                  disabled={reason !== "ok"}
                  className="menu-item"
                  onClick={() => {
                    if (choice !== value) onChange(choice);
                    else onDismiss?.();
                    close();
                  }}
                >
                  <span className="menu-choice">
                    <span className="picker-name">
                      {workspace
                        ? workspaceName(workspace, t)
                        : t("workspaces.new")}
                    </span>
                    {workspace && (
                      <span className="type-small muted">
                        {detail(workspace)}
                      </span>
                    )}
                  </span>
                  <span className="menu-check">
                    {choice === value && <Icon glyph={Check} size={16} />}
                  </span>
                </button>
              );
            })}
          </Menu>
          {shown.length === 0 && query.trim() && (
            <p className="type-small muted workspace-picker-empty">
              {t("workspaces.pickerNoMatch")}
            </p>
          )}
        </Popover>
      )}
    </div>
  );
}

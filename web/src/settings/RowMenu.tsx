"use client";

import {
  Bell,
  CircleCheck,
  ClipboardPaste,
  Copy,
  Download,
  Ellipsis,
  History,
  Pencil,
  Star,
  Trash2,
  Users,
  type LucideIcon,
} from "lucide-react";

import { ActionMenu } from "../ui/ActionMenu";

/** One of a row's secondary actions: its short word, its full accessible name, and why it can't run, if it can't. */
export type RowAction = {
  id: string;
  label: string;
  name: string;
  danger?: boolean;
  /** Why the action isn't possible now: the item stays reachable by keyboard and says so (UI-GUIDELINES §5b). */
  unavailable?: string | undefined;
  onSelect: () => void;
};

/** The line icon for each Settings row action (the UI standard: one before every menu item). */
const ACTION_ICONS: Readonly<Record<string, LucideIcon>> = {
  edit: Pencil,
  download: Download,
  duplicate: Copy,
  delete: Trash2,
  remove: Trash2,
  reads: History,
  bindings: Users,
  check: CircleCheck,
  default: Star,
  paste: ClipboardPaste,
  reminder: Bell,
};

/** A Settings table row's "…" menu (UI-GUIDELINES §7, one actions column), on the shared action menu. */
export function RowMenu({
  label,
  actions,
}: {
  label: string;
  actions: readonly RowAction[];
}) {
  return (
    <ActionMenu
      label={label}
      actions={actions.map((action) => ({
        ...action,
        icon: ACTION_ICONS[action.id] ?? Ellipsis,
      }))}
    />
  );
}

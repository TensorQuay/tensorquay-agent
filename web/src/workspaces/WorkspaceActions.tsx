"use client";

import "./catalogue";
import {
  CopyPlus,
  FolderOpen,
  Pencil,
  Share2,
  SquarePen,
  Trash2,
  type LucideIcon,
} from "lucide-react";

import { useT } from "../i18n/I18nProvider";
import type { Workspace } from "../protocol/generated/api";
import { ActionMenu } from "../ui/ActionMenu";
import { Button } from "../ui/Button";
import { IconAction } from "../ui/IconAction";

import { canWrite, workspaceName } from "./workspace-state";

export type WorkspaceAction =
  "open" | "newTask" | "fork" | "share" | "rename" | "delete";

const LABELS = {
  open: "workspaces.open",
  newTask: "workspaces.newTaskHere",
  fork: "workspaces.fork",
  share: "workspaces.share",
  rename: "workspaces.rename",
  delete: "workspaces.delete",
} as const satisfies Record<WorkspaceAction, string>;
const ICONS: Readonly<Record<WorkspaceAction, LucideIcon>> = {
  open: FolderOpen,
  newTask: SquarePen,
  fork: CopyPlus,
  share: Share2,
  rename: Pencil,
  delete: Trash2,
};

/**
 * A workspace's actions by access (UI-GUIDELINES §7): one Open action, and the shared action menu named after the workspace
 * with New task here (write access), Fork (anyone who can read), and Share, Rename and Delete (the owner). Nothing
 * while a fork is copied; only Delete when the copy failed. In the table the row itself opens, so Open is the
 * menu's first item there (`openInMenu`), never a button on every row.
 */
export function WorkspaceActions({
  workspace,
  onAction,
  openInMenu = false,
}: {
  workspace: Workspace;
  onAction: (action: WorkspaceAction) => void;
  openInMenu?: boolean;
}) {
  const t = useT();
  const name = workspaceName(workspace, t);
  const owner = workspace.access === "owner";
  if (workspace.state === "forking") return null;
  if (workspace.state !== "ready")
    return owner ? (
      <div className="workspace-actions">
        <IconAction
          glyph={Trash2}
          name={t("workspaces.deleteName", { name })}
          tip={t("workspaces.delete")}
          onClick={() => onAction("delete")}
        />
      </div>
    ) : null;
  const actions = (
    [
      ["open", openInMenu],
      ["newTask", canWrite(workspace)],
      ["fork", true],
      ["share", owner],
      ["rename", owner],
      ["delete", owner],
    ] as const
  )
    .filter(([, allowed]) => allowed)
    .map(([action]) => action);
  const label = t("workspaces.more", { name });
  return (
    <div className="workspace-actions">
      {!openInMenu && (
        <Button
          variant="secondary"
          aria-label={t("workspaces.openName", { name })}
          onClick={() => onAction("open")}
        >
          {t("workspaces.open")}
        </Button>
      )}
      <ActionMenu
        label={label}
        actions={actions.map((action) => ({
          id: action,
          label: t(LABELS[action]),
          icon: ICONS[action],
          danger: action === "delete",
          onSelect: () => onAction(action),
        }))}
      />
    </div>
  );
}

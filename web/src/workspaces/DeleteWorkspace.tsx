"use client";

import "./catalogue";
import { useQueryClient } from "@tanstack/react-query";

import { api, ApiError } from "../api/client";
import { useRequest } from "../api/use-request";
import { useT } from "../i18n/I18nProvider";
import type { Workspace } from "../protocol/generated/api";
import { Button } from "../ui/Button";
import { ConfirmDialog } from "../ui/ConfirmDialog";
import { Dialog } from "../ui/Dialog";

import { taskFilter } from "./task-filter";
import { deletePlan, workspaceName } from "./workspace-state";

/**
 * The delete confirmation from the gateway's rules: it names the workspace and how many tasks go
 * with it; when it cannot be deleted, only the reason and no delete button.
 */
export function DeleteWorkspace({
  workspace,
  mine,
  onClose,
  onDeleted,
}: {
  workspace: Workspace;
  /** The reader's active tasks on it, or null when the task list is not fully loaded. */
  mine: number | null;
  onClose: () => void;
  /** It was deleted: where the focus goes once its card is gone (WEB-U-275). */
  onDeleted?: (() => void) | undefined;
}) {
  const t = useT();
  const cache = useQueryClient();
  const request = useRequest();
  const name = workspaceName(workspace, t);
  const plan = deletePlan(workspace, mine);
  if (plan.kind === "blocked")
    return (
      <Dialog
        open
        title={t("workspaces.deleteTitle")}
        onClose={onClose}
        actions={
          <Button variant="secondary" onClick={onClose}>
            {t("common.close")}
          </Button>
        }
      >
        <p>
          {plan.reason === "working"
            ? t("workspaces.deleteWorking", { name })
            : plan.reason === "pinned"
              ? // The way out only where the reader can take it: the title is theirs alone (their own task).
                workspace.lease?.title
                ? t("workspaces.deletePinned", {
                    name,
                    task: workspace.lease.title,
                  })
                : t("workspaces.deletePinnedOther", { name })
              : plan.reason === "othersTasks"
                ? t("workspaces.deleteOthers", { name })
                : t("workspaces.deleteNotReady", { name })}
        </p>
      </Dialog>
    );
  const withTasks = plan.kind === "withTasks";
  async function remove() {
    const result = await request.run((signal) =>
      api.deleteWorkspace(workspace.id, withTasks, signal),
    );
    if (!result.ok) return;
    if (taskFilter.current()?.id === workspace.id) taskFilter.clear();
    cache.setQueryData<{ items: Workspace[] }>(["workspaces"], (current) =>
      current
        ? {
            items: current.items.filter((item) => item.id !== workspace.id),
          }
        : current,
    );
    await Promise.all([
      cache.invalidateQueries({ queryKey: ["workspaces"] }),
      withTasks
        ? cache.invalidateQueries({ queryKey: ["conversations"] })
        : null,
    ]);
    onDeleted?.();
    onClose();
  }
  return (
    <ConfirmDialog
      title={t("workspaces.deleteTitle")}
      confirm={t("workspaces.delete")}
      pending={request.pending}
      error={request.error}
      errorCopy={(error) =>
        error instanceof ApiError && error.status === 409
          ? // A task in it leads helpers: the way through is that task's delete with them, never the raw code.
            error.code === "has_helpers"
            ? t("workspaces.deleteHasHelpers")
            : t("workspaces.deleteRefused")
          : null
      }
      onConfirm={() => {
        void remove();
      }}
      onClose={onClose}
    >
      {withTasks
        ? t("workspaces.deleteWithTasks", { name, count: plan.count })
        : t("workspaces.deleteEmpty", { name })}
    </ConfirmDialog>
  );
}

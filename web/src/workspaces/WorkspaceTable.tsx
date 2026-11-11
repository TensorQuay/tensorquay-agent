"use client";

import "./catalogue";
import { useT } from "../i18n/I18nProvider";
import type { Workspace } from "../protocol/generated/api";
import { TableScroll } from "../ui/TableScroll";

import { RenameField, type Rename } from "./RenameField";
import { workspaceName } from "./workspace-state";
import { WorkspaceActions, type WorkspaceAction } from "./WorkspaceActions";
import { LeaseLine, ShareCount, SizeMeter } from "./WorkspaceParts";

/**
 * The compact list for many workspaces: §7 table rows with the same states and actions as the cards. The state says
 * when an idle workspace was last used, so no column repeats it.
 */
export function WorkspaceTable({
  label,
  workspaces,
  sources,
  onAction,
  rename,
}: {
  label: string;
  workspaces: readonly Workspace[];
  sources: ReadonlyMap<string, string>;
  onAction: (workspace: Workspace, action: WorkspaceAction) => void;
  /** The workspace renamed in place, if any, and how. */
  rename?: (Rename & { id: string }) | undefined;
}) {
  const t = useT();
  // A column appears only when a row fills it: the owner, only when someone else's workspace is listed.
  const others = workspaces.some((workspace) => workspace.access !== "owner");
  return (
    <TableScroll label={label}>
      <table className="data-table workspace-table" aria-label={label}>
        <thead className="type-label">
          <tr>
            <th scope="col">{t("workspaces.name")}</th>
            {others && (
              <th scope="col" className="workspace-owner">
                {t("workspaces.ownerColumn")}
              </th>
            )}
            <th scope="col">{t("workspaces.state")}</th>
            <th scope="col">{t("workspaces.size")}</th>
            <th scope="col">
              <span className="sr-only">{t("common.actions")}</span>
            </th>
          </tr>
        </thead>
        <tbody>
          {workspaces.map((workspace) => {
            const ready = workspace.state === "ready";
            const open = () => onAction(workspace, "open");
            return (
              <tr
                key={workspace.id}
                data-state={workspace.state}
                className={ready ? "workspace-row" : undefined}
                // The row opens the workspace: one main action per view, not an Open button on every row.
                tabIndex={ready ? 0 : undefined}
                title={
                  ready
                    ? t("workspaces.openName", {
                        name: workspaceName(workspace),
                      })
                    : undefined
                }
                onClick={(event) => {
                  if (
                    ready &&
                    !(event.target as Element).closest("button, a, input")
                  )
                    open();
                }}
                onKeyDown={(event) => {
                  if (
                    ready &&
                    event.key === "Enter" &&
                    event.target === event.currentTarget
                  ) {
                    event.preventDefault();
                    open();
                  }
                }}
              >
                <td>
                  <div className="data-name">
                    {rename?.id === workspace.id ? (
                      <RenameField
                        name={workspaceName(workspace)}
                        rename={rename}
                      />
                    ) : (
                      <span title={workspaceName(workspace)}>
                        {workspaceName(workspace)}
                      </span>
                    )}
                    <ShareCount workspace={workspace} />
                  </div>
                </td>
                {others && (
                  <td className="type-small workspace-owner">
                    <span className="cell-whole">
                      {workspace.access === "owner"
                        ? t("workspaces.ownerYou")
                        : t("workspaces.ownerAccess", {
                            owner: workspace.owner.email,
                            access: t(
                              workspace.access === "write"
                                ? "workspaces.canEdit"
                                : "workspaces.canView",
                            ),
                          })}
                    </span>
                  </td>
                )}
                <td>
                  {workspace.state === "forking" ? (
                    <span className="type-small muted">
                      {t("workspaces.copying", {
                        source:
                          sources.get(workspace.fork?.from ?? "") ??
                          t("workspaces.anotherWorkspace"),
                      })}
                    </span>
                  ) : ready ? (
                    <LeaseLine workspace={workspace} />
                  ) : (
                    <span className="type-small workspace-failed">
                      {t("workspaces.copyFailed")}
                    </span>
                  )}
                </td>
                <td className="workspace-size-cell">
                  {ready && <SizeMeter workspace={workspace} inTable />}
                </td>
                <td className="data-cell-action">
                  <WorkspaceActions
                    workspace={workspace}
                    openInMenu
                    onAction={(action) => onAction(workspace, action)}
                  />
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </TableScroll>
  );
}

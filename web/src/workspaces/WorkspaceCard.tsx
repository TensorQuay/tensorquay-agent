"use client";

import "./catalogue";
import { useId } from "react";

import { useT } from "../i18n/I18nProvider";
import type { Workspace } from "../protocol/generated/api";
import { Skeleton } from "../ui/Skeleton";

import { RenameField, type Rename } from "./RenameField";
import { workspaceName } from "./workspace-state";
import { WorkspaceActions, type WorkspaceAction } from "./WorkspaceActions";
import { LeaseLine, SizeMeter, WorkspaceBadge } from "./WorkspaceParts";

/**
 * A workspace card (UI-GUIDELINES §7): name and badge; the owner on a share; the lease; size against the quota; the
 * actions. A fork being copied shows a shimmer in place of the meter (mockup change 2); `source` names its origin.
 */
export function WorkspaceCard({
  workspace,
  source,
  onAction,
  rename,
}: {
  workspace: Workspace;
  source?: string | undefined;
  onAction: (action: WorkspaceAction) => void;
  /** Renaming in place (the UI standard's In-place edit): the name is the field. */
  rename?: Rename | undefined;
}) {
  const t = useT();
  const titleId = useId();
  const name = workspaceName(workspace, t);
  return (
    <article
      className="card workspace-card"
      aria-labelledby={titleId}
      data-state={workspace.state}
    >
      <div className="workspace-card-head">
        <h3 id={titleId} className="type-body workspace-name" title={name}>
          {rename ? <RenameField name={name} rename={rename} /> : name}
        </h3>
        <WorkspaceBadge workspace={workspace} />
      </div>
      {workspace.access !== "owner" && (
        <p className="type-small muted workspace-owner">
          {t("workspaces.owner", { email: workspace.owner.email })}
        </p>
      )}
      {workspace.state === "forking" ? (
        <div className="workspace-copying">
          <p className="type-small muted">
            {source
              ? t("workspaces.copying", { source })
              : t("workspaces.copyingUnknown")}
          </p>
          <Skeleton label={t("workspaces.copyingLabel")} lines={1} />
        </div>
      ) : workspace.state === "ready" ? (
        <>
          <LeaseLine workspace={workspace} />
          <SizeMeter workspace={workspace} />
        </>
      ) : (
        <p className="type-small workspace-failed">
          {t("workspaces.copyFailed")}
        </p>
      )}
      <WorkspaceActions workspace={workspace} onAction={onAction} />
    </article>
  );
}

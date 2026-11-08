"use client";

import "./catalogue";
import { Users } from "lucide-react";

import { RelativeTime } from "../conversations/RelativeTime";
import { useT } from "../i18n/I18nProvider";
import { formatSize } from "../i18n/message";
import type { Workspace } from "../protocol/generated/api";
import { Icon } from "../ui/Icon";

import { leaseState, meterLevel } from "./workspace-state";

// The relative time is a live element inside a sentence; translations never contain this marker.
const TIME_MARK = "\u0000";

/** "Shared · 2" on mine, "Can edit" or "Can view" on a share, "Copy" on a copy: the standard pill (change 5). */
export function WorkspaceBadge({ workspace }: { workspace: Workspace }) {
  const t = useT();
  const label =
    workspace.access === "write"
      ? t("workspaces.canEdit")
      : workspace.access === "read"
        ? t("workspaces.canView")
        : workspace.share_count
          ? t("workspaces.badgeShared", { count: workspace.share_count })
          : workspace.fork
            ? t("workspaces.forkBadge")
            : null;
  if (!label) return null;
  return (
    <span
      className="status-pill type-label"
      data-tone={
        workspace.access === "owner" && workspace.share_count ? "info" : "muted"
      }
    >
      {label}
    </span>
  );
}

/**
 * The table's sharing mark: a small people icon and the count after the name of a workspace you share, said in words
 * for assistive technology; a copy keeps its Fork pill. Access to one shared with you is in the owner's cell.
 */
export function ShareCount({ workspace }: { workspace: Workspace }) {
  const t = useT();
  if (workspace.access === "owner" && workspace.share_count) {
    const words = t("workspaces.sharedWith", { count: workspace.share_count });
    return (
      <span className="share-count type-small muted" title={words}>
        <Icon glyph={Users} size={16} />
        <span aria-hidden="true">{workspace.share_count}</span>
        <span className="sr-only">{words}</span>
      </span>
    );
  }
  return workspace.fork ? (
    <span className="status-pill type-label" data-tone="muted">
      {t("workspaces.forkBadge")}
    </span>
  ) : null;
}

/** The lease line: one blue state for "in use" (pulsing only while a run holds it), grey when idle (change 1). */
export function LeaseLine({ workspace }: { workspace: Workspace }) {
  const t = useT();
  const state = leaseState(workspace);
  let text;
  const { lease } = workspace;
  // In use only while the holder's run is going; a done task whose environment is kept is where it's open (the 13h UI
  // review, #3).
  if (lease?.running)
    text =
      lease.title === null
        ? t("workspaces.inUseOther")
        : t("workspaces.inUseBy", { title: lease.title });
  else if (lease)
    text =
      lease.title === null
        ? t("workspaces.openInOther")
        : t("workspaces.openIn", { title: lease.title });
  else if (workspace.last_used_at) {
    const [before, after] = t("workspaces.idleUsed", {
      time: TIME_MARK,
    }).split(TIME_MARK);
    text = (
      <>
        {before}
        <RelativeTime value={workspace.last_used_at} />
        {after}
      </>
    );
  } else text = t("workspaces.idleNever");
  return (
    <p
      className={`workspace-lease type-small${state === "held" ? " muted" : ""}`}
      data-lease={state}
      // The whole line on hover where the table truncates it.
      title={typeof text === "string" ? text : undefined}
    >
      <span className="lease-dot" aria-hidden="true" />
      <span className="workspace-lease-text">{text}</span>
    </p>
  );
}

/** Size against the quota: a thin meter that warns at 80 % and turns danger at 95 %; the size alone without a quota. */
export function SizeMeter({
  workspace,
  inTable = false,
}: {
  workspace: Workspace;
  /** In the workspaces table, when it was measured is the cell's one muted line (17.3, UI-GUIDELINES §7 Table). */
  inTable?: boolean;
}) {
  const t = useT();
  const { size_bytes: size, quota_bytes: quota } = workspace;
  if (size === null)
    return (
      <p className="workspace-size type-small muted">
        {t("workspaces.notMeasured")}
      </p>
    );
  const level = meterLevel(size, quota);
  if (quota === null || level === null)
    return (
      <div className="workspace-size">
        <p className="type-small muted">{formatSize(size, t.locale)}</p>
        <Measured at={workspace.size_measured_at} note={inTable} />
      </div>
    );
  const usage = t("workspaces.sizeOf", {
    size: formatSize(size, t.locale),
    quota: formatSize(quota, t.locale),
  });
  return (
    <div className="workspace-size" data-level={level}>
      <div
        className="workspace-meter"
        role="meter"
        aria-label={t("workspaces.size")}
        aria-valuemin={0}
        aria-valuemax={quota}
        aria-valuenow={Math.min(size, quota)}
        aria-valuetext={usage}
        data-level={level}
      >
        <span
          className="workspace-meter-fill"
          style={{ inlineSize: `${Math.min(100, (size / quota) * 100)}%` }}
        />
      </div>
      <p className="type-small workspace-usage">
        {level === "danger" ? t("workspaces.almostFull", { usage }) : usage}
      </p>
      <Measured at={workspace.size_measured_at} note={inTable} />
    </div>
  );
}

/**
 * When the size was measured: it is a past measurement, taken while the workspace was free or during a run, so it never
 * shows alone (the 13h UI review, #4: "0 B" for a workspace holding a file was taken before the file existed).
 */
function Measured({ at, note }: { at: string | null; note: boolean }) {
  const t = useT();
  if (at === null) return null;
  const [before, after] = t("workspaces.measured", { time: TIME_MARK }).split(
    TIME_MARK,
  );
  return (
    <p
      className={`type-small muted workspace-measured${note ? " cell-note" : ""}`}
    >
      {before}
      <RelativeTime value={at} />
      {after}
    </p>
  );
}

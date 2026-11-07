import { english, type Translate } from "../i18n/translate";
import type { Workspace } from "../protocol/generated/api";

/** The size meter's colour: warning from 80 % of the quota, danger from 95 % (task 076). */
export type MeterLevel = "ok" | "warn" | "danger";
const WARN_SHARE = 0.8;
const DANGER_SHARE = 0.95;

/** `null` when there is no meter: the quota is unlimited or the size was never measured. */
export function meterLevel(
  size: number | null,
  quota: number | null,
): MeterLevel | null {
  if (size === null || quota === null || quota <= 0) return null;
  const share = size / quota;
  return share >= DANGER_SHARE ? "danger" : share >= WARN_SHARE ? "warn" : "ok";
}

/** Idle, held by a task with a sandbox, or held while that task's run is in progress (the dot pulses). */
export type LeaseState = "idle" | "held" | "running";
export function leaseState(workspace: Workspace): LeaseState {
  if (!workspace.lease) return "idle";
  return workspace.lease.running ? "running" : "held";
}

/**
 * The delete confirmation, from the gateway's rules (answers-076 Q9): refused while a task's run works in it or
 * while it is being copied; bound tasks go with it only when all are the owner's. `mine` is the reader's own active tasks on it,
 * or `null` when the task list is not fully loaded (the gateway still refuses other people's tasks).
 */
export type DeletePlan =
  | { kind: "empty" }
  | { kind: "withTasks"; count: number }
  | {
      kind: "blocked";
      reason: "working" | "pinned" | "othersTasks" | "notReady";
    };
export function deletePlan(
  workspace: Workspace,
  mine: number | null,
): DeletePlan {
  if (workspace.state === "forking")
    return { kind: "blocked", reason: "notReady" };
  // Only a going run blocks it: a done task's kept, idle environment is retired by the delete (the 13h UI review, #3).
  if (workspace.lease?.running) return { kind: "blocked", reason: "working" };
  // A pinned task keeps its environment on (FR-33): the delete doesn't retire it.
  if (workspace.lease?.pinned) return { kind: "blocked", reason: "pinned" };
  const count = workspace.task_count ?? 0;
  if (count === 0) return { kind: "empty" };
  if (mine !== null && mine < count)
    return { kind: "blocked", reason: "othersTasks" };
  return { kind: "withTasks", count };
}

/** Whether a new task may start on it: binding needs write access and a ready workspace. */
export type Pickable = "ok" | "viewOnly" | "copying" | "failed";
export function pickable(workspace: Workspace): Pickable {
  if (workspace.state === "forking") return "copying";
  if (workspace.state !== "ready") return "failed";
  return workspace.access === "read" ? "viewOnly" : "ok";
}

/** Owner or write share: may start tasks, fork is open to readers too. */
export const canWrite = (workspace: Workspace) =>
  workspace.access === "owner" || workspace.access === "write";

const recency = (workspace: Workspace) =>
  Date.parse(workspace.last_used_at ?? workspace.created_at);

/** Most recently used (or created, when never used) first. */
export function recentFirst(a: Workspace, b: Workspace) {
  return recency(b) - recency(a);
}

/**
 * The name shown for a workspace; an administrator's metadata view has none. A helper's own copy of its lead's files
 * (kind helper_copy) says so in the reader's words, whatever its name (the 14b page pass, H4).
 */
export function workspaceName(
  workspace: Workspace,
  t: Translate = english,
): string {
  const name = workspace.name ?? "";
  return workspace.kind === "helper_copy" && name
    ? t("workspaces.helperCopyName", { name })
    : name;
}

/** Case-insensitive name match for the page and picker searches, on the name as shown. */
export function matches(
  workspace: Workspace,
  query: string,
  t: Translate = english,
) {
  const needle = query.trim().toLocaleLowerCase();
  return (
    !needle || workspaceName(workspace, t).toLocaleLowerCase().includes(needle)
  );
}

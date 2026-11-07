import type { Conversation } from "../protocol/generated/api";

/** What the sidebar's dot shows for a task's run; a quiet task (idle, done, stopped) shows none. */
export type TaskStatus = "working" | "failed";

/**
 * The dot for a listed task's `run_status`; an unknown value shows none. Nothing waits for the reader (PRD FR-10,
 * superseded: no approvals), so there is no "needs you".
 */
export function taskStatus(
  run: string | undefined,
  team: Conversation["team"] = null,
): TaskStatus | null {
  switch (run) {
    case "running":
      return "working";
    case "failed":
      // A lead's failed run while its team runs isn't the task's outcome: a helper's report wakes it (WEB-U-409, c17
      // U4; its page says it waits for its helpers).
      return team?.role === "lead" && team.state === "running"
        ? null
        : "failed";
    default:
      return null;
  }
}

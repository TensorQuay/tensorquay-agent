import type { TaskState } from "./contract";

/**
 * A to-do still to finish: a place is free, or a claimant's latest report is not "completed" (none yet, failed or
 * blocked). The header counts these (the owner's choice of 22 Nov 2026: a channel whose to-dos were all claimed showed
 * "To-dos 0" while the work was still going on).
 */
export function unfinished(task: TaskState): boolean {
  if (task.claims.length < task.limit) return true;
  return task.claims.some((claim) => {
    const report = task.reports.findLast(
      (one) =>
        one.author.kind === claim.author.kind &&
        one.author.id === claim.author.id,
    );
    return report?.status !== "completed";
  });
}

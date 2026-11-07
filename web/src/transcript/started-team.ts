import type { Transcript } from "./types";

/**
 * The task's last run, when it started an agent team: an ok start_member in it (WEB-U-416). The gateway answers the call
 * only once the team is in, so this is the page's sign of its new team at once, in the same render as the run's end;
 * its first team.changed comes from a once-a-second flush, and a lead's first run can end before it. Null otherwise.
 */
export function startedTeam(transcript: Transcript): string | null {
  const id = transcript.run.id;
  if (id === null) return null;
  const started = transcript.items.some(
    (item) =>
      item.kind === "step" &&
      item.runId === id &&
      Object.values(item.tools).some(
        (tool) => tool.name === "start_member" && tool.status === "ok",
      ),
  );
  return started ? id : null;
}

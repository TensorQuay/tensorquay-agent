import type { MessageKey } from "../i18n/translate";

import { argument } from "./tool-results";
import type { ToolView } from "./types";

/**
 * Why a helper couldn't start, in plain words under its step (WEB-U-399; c16k real-world B5: the page showed the
 * runtime's line "unknown_reference: unknown reference: profile"). The runtime's line stays folded in the step's
 * Result; a code without words here adds nothing beyond the step's own line.
 */
export function teamFailure(
  tool: ToolView,
): { key: MessageKey; values?: Record<string, string> } | null {
  if (tool.name !== "start_member" || tool.status !== "error") return null;
  const text = tool.result ?? tool.output ?? "";
  const code = /^([a-z0-9_]+):/.exec(text)?.[1] ?? "";
  if (code === "unknown_reference" && /\bprofile\b/.test(text)) {
    const name = argument(tool, "profile");
    return typeof name === "string" && name
      ? { key: "activity.teamNoProfile", values: { name } }
      : null;
  }
  if (code === "member_limit") return { key: "activity.teamMemberLimit" };
  if (code === "team_ended") return { key: "activity.teamEnded" };
  return null;
}

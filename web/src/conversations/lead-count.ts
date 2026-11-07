import { formatNumber } from "../i18n/message";
import type { Translate } from "../i18n/translate";

/** A team part's tone, as the stylesheet's tokens name them. */
export type Tone = "info" | "muted" | "danger" | "success";

/**
 * The end of a lead task's row in the task list (UI-GUIDELINES §13b, WEB-U-220; in the task list's own code, so the sidebar loads none
 * of the team's parts until a lead's row opens): its helper count in muted, or, when
 * a helper failed or is stuck, that in the count's place in danger (failed first: it won't recover on its own). `label`
 * says it all for assistive technology. Null with no helpers left.
 */
export function leadCount(
  team: {
    helpers: number;
    attention: { stuck: number; failed: number };
    /** The team's state: once it no longer runs, nothing in it needs the person (the c16 UX review, #2). */
    state?: string;
  },
  t: Translate,
): { words: string; label: string; tone: Tone } | null {
  const { helpers } = team;
  const attention =
    team.state === undefined || team.state === "running"
      ? team.attention
      : { stuck: 0, failed: 0 };
  if (helpers <= 0) return null;
  const all = t("team.helpersCount", { count: helpers });
  const trouble = [
    attention.failed > 0 && t("team.failedCount", { count: attention.failed }),
    attention.stuck > 0 && t("team.stuckCount", { count: attention.stuck }),
  ].filter((part): part is string => Boolean(part));
  if (trouble.length === 0)
    return {
      words: formatNumber(helpers, t.locale),
      label: all,
      tone: "muted",
    };
  return {
    words: trouble[0] ?? "",
    label: [all, ...trouble].join(", "),
    tone: "danger",
  };
}

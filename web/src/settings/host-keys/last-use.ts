import type { MessageKey, Translate } from "../../i18n/translate";

import type { HostKeyUse } from "./contract";

/** A refused use in words (ADR-0037 §8's outcomes; the Keys sketch §8), never its code. */
export const REFUSALS: Readonly<
  Record<Exclude<HostKeyUse["outcome"], "ok">, MessageKey>
> = {
  foreign_run: "hostKeys.refusedForeignRun",
  foreign_sandbox: "hostKeys.refusedForeignSandbox",
  expired: "hostKeys.refusedExpired",
  wrong_host: "hostKeys.refusedWrongHost",
  wrong_position: "hostKeys.refusedWrongPosition",
  not_allowed: "hostKeys.refusedNotAllowed",
  absent: "hostKeys.refusedAbsent",
  revoked: "hostKeys.refusedRevoked",
  unrecorded: "hostKeys.refusedUnrecorded",
  unavailable: "hostKeys.refusedUnavailable",
};

/**
 * When a key was last used and nothing more, "Today, 14:02", "Sep 28" or "Never used": a member's own key in an
 * administrator's list, where the member's task, and whether the try was refused, are theirs (least exposure).
 */
export function lastUseTime(
  use: Pick<HostKeyUse, "at"> | null,
  t: Translate,
  now: Date,
  timeZone?: string,
): string {
  if (!use) return t("hostKeys.neverUsed");
  const { today, time, date } = moment(use.at, t, now, timeZone);
  return today ? t("hostKeys.usedTodayAt", { time }) : date;
}

/** A use's day and time as a row says them: whether it was today, its time, and its date (with the year if not this one). */
function moment(stamp: string, t: Translate, now: Date, timeZone?: string) {
  const at = new Date(stamp);
  const zone = timeZone ? { timeZone } : {};
  const day = (date: Date) =>
    new Intl.DateTimeFormat("en-CA", { ...zone, dateStyle: "short" }).format(
      date,
    );
  const time = new Intl.DateTimeFormat(t.locale, {
    ...zone,
    hour: "2-digit",
    minute: "2-digit",
    hourCycle: "h23",
  }).format(at);
  const sameYear =
    new Intl.DateTimeFormat("en", { ...zone, year: "numeric" }).format(at) ===
    new Intl.DateTimeFormat("en", { ...zone, year: "numeric" }).format(now);
  const date = new Intl.DateTimeFormat(
    t.locale,
    sameYear
      ? { ...zone, month: "short", day: "numeric" }
      : { ...zone, dateStyle: "medium" },
  ).format(at);
  return { at, zone, today: day(at) === day(now), time, date };
}

/**
 * A key's last use as its row says it, "Today, 14:02 · Sales report", "Sep 28 · Q3 planning" or "Never used", or
 * its last refused try in words; and the whole, for the row's tooltip.
 */
export function lastUseWords(
  use: HostKeyUse | null,
  t: Translate,
  now: Date,
  timeZone?: string,
): { text: string; title: string | null } {
  if (!use) return { text: t("hostKeys.neverUsed"), title: null };
  const { at, zone, today, time, date } = moment(use.at, t, now, timeZone);
  const task = use.task_title ?? t("hostKeys.noTask");
  // Who used it, in an administrator's view of a shared key.
  const by = use.user ? t("hostKeys.usedBy", { person: use.user.email }) : "";
  const title = `${t("hostKeys.usedTitle", {
    count: use.count,
    task,
    when: t("hostKeys.dateTime", {
      date: new Intl.DateTimeFormat(t.locale, {
        ...zone,
        dateStyle: "medium",
      }).format(at),
      time,
    }),
  })}${by}`;
  if (use.outcome !== "ok")
    return {
      text: t("hostKeys.refused", { reason: t(REFUSALS[use.outcome]) }),
      title,
    };
  return {
    text: today
      ? t("hostKeys.usedToday", { time, task })
      : t("hostKeys.usedOn", { date, task }),
    title,
  };
}

"use client";

import { useId } from "react";

import { useT } from "../i18n/I18nProvider";
import { formatNumber, formatSize } from "../i18n/message";
import type { MessageKey } from "../i18n/translate";
import { DeferredMarkdown } from "../transcript/DeferredMarkdown";

import type { TeamEndView } from "./team-view";
import "./catalogue";

const REASONS: Readonly<Record<string, MessageKey>> = {
  done: "team.endDone",
  stopped: "team.endStopped",
  budget: "team.endBudget",
  deadline: "team.endDeadline",
  idle: "team.endIdle",
  person_disabled: "team.endDisabled",
};

/**
 * The team's end note in the lead task's transcript (ADR-0034 point 10): why it stopped, the lead's summary, each one's
 * share of the team's use (in percent: the word tokens never appears, PRD §12 item 11), their reports and the files
 * they handed back. Every task stays a task after it.
 */
/** Each helper's share of the team's use, then the lead task's (the rest), in whole percent. */
function shares(note: TeamEndView, locale: string, lead: string) {
  const { total, members } = note.tokens;
  const percent = (part: number) =>
    `${formatNumber(Math.round((part / total) * 100), locale)} %`;
  const helpers = members.reduce((sum, member) => sum + member.tokens, 0);
  return [
    ...members.map((member) => `${member.name} ${percent(member.tokens)}`),
    `${lead} ${percent(Math.max(total - helpers, 0))}`,
  ].join(", ");
}

export function TeamEndNote({ note }: { note: TeamEndView }) {
  const t = useT();
  const id = useId();
  const reports = new Map<string, number>();
  for (const report of note.reports)
    reports.set(report.member, (reports.get(report.member) ?? 0) + 1);
  return (
    <section className="team-end" aria-labelledby={id}>
      <p id={id} className="type-body-strong">
        {t(REASONS[note.reason] ?? "team.endOther")}
      </p>
      {note.reason === "budget" && (
        // The goal may not be met: the next step, in words (WEB-U-394, real-world use before 17.0).
        <p className="type-small">{t("team.endBudgetNext")}</p>
      )}
      {/* The lead's own words, formatted as its reply is: never raw Markdown marks (WEB-U-396). */}
      {note.summary && (
        <div className="markdown-answer markdown-body message-text">
          <DeferredMarkdown text={note.summary} />
        </div>
      )}
      {note.tokens.total > 0 && (
        <p className="type-small muted">
          {t("team.endShares", {
            shares: shares(note, t.locale, t("team.endLead")),
          })}
        </p>
      )}
      {reports.size > 0 && (
        <p className="type-small muted">
          {new Intl.ListFormat(t.locale, { type: "conjunction" }).format(
            [...reports].map(([member, count]) =>
              t("team.endReports", { count, member }),
            ),
          )}
        </p>
      )}
      {note.files.length > 0 && (
        <p className="type-small muted">
          {t("team.endFiles", {
            files: note.files
              .map((file) =>
                t("team.endFile", {
                  name: file.name,
                  size: formatSize(file.size, t.locale),
                  member: file.member,
                }),
              )
              // The reader's own separator: "、" in Chinese, where each file's parentheses use "，" (WEB-U-375).
              .join(t("team.endFilesSeparator")),
          })}
        </p>
      )}
    </section>
  );
}

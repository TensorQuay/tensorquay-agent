"use client";

import Link from "next/link";

import { useT } from "../i18n/I18nProvider";

import type { TeamView } from "./team-view";
import { budgetLine, endedLine, helperStateWords } from "./team-words";
import "./catalogue";

/** The most helper chips the line shows; the rest are "+N", which opens All helpers (UI-GUIDELINES §13b). */
const CHIPS = 4;

/**
 * The team strip (UI-GUIDELINES §13b): one quiet line under the lead task's bar, a chip per helper (its state's dot
 * and its name, opening its page), then the budget used and the time left with a thin meter, the exact figures in the
 * tooltip. On a phone it is one line, "3 helpers · Budget 20 % used ▸", that opens All helpers. Both are rendered; the
 * stylesheet shows one by the strip's width, and the hidden one leaves the accessibility tree with it.
 */
export function TeamStrip({
  team,
  now,
  onAll,
}: {
  team: TeamView;
  now: number;
  onAll: () => void;
}) {
  const t = useT();
  const budget = budgetLine(team, now, t);
  // An ended team says how it ended, with no time left to count and no meter (the c16 UX review, #1).
  const ended = endedLine(team, t);
  const shown = team.helpers.slice(0, CHIPS);
  const more = team.helpers.length - shown.length;
  return (
    <section
      className="helpers-strip type-small"
      aria-label={t("team.helpers")}
    >
      <div className="helpers-strip-wide">
        <ul className="helpers-chips">
          {shown.map((helper) => {
            const state = helperStateWords(helper, t);
            return (
              <li key={helper.conversation}>
                <Link
                  href={`/c/${helper.conversation}`}
                  className="helpers-chip"
                  data-tone={state.tone}
                  aria-label={t("team.chip", {
                    name: helper.name,
                    state: state.words,
                  })}
                  title={`${helper.name}: ${state.words}`}
                >
                  <span className="helpers-dot" aria-hidden="true" />
                  <span className="helpers-chip-name">{helper.name}</span>
                </Link>
              </li>
            );
          })}
        </ul>
        {more > 0 && (
          <button
            type="button"
            className="helpers-more muted"
            aria-label={t("team.allMore", { count: more })}
            onClick={onAll}
          >
            {t("team.more", { count: more })}
          </button>
        )}
        {ended ? (
          <span className="helpers-budget muted" title={ended.exact}>
            <span>{ended.short}</span>
          </span>
        ) : (
          <span className="helpers-budget muted" title={budget.exact}>
            <span>{budget.short}</span>
            <span
              className="helpers-meter"
              role="meter"
              aria-label={t("team.budgetMeter")}
              aria-valuemin={0}
              aria-valuemax={100}
              aria-valuenow={budget.percent}
            >
              <span style={{ inlineSize: `${budget.percent}%` }} />
            </span>
          </span>
        )}
        {/* §13b: the strip's "All helpers" opens the sheet whatever the count; "+N" is a second way in. */}
        <button type="button" className="helpers-all muted" onClick={onAll}>
          {t("team.all")}
        </button>
      </div>
      <button
        type="button"
        className="helpers-strip-narrow muted"
        onClick={onAll}
      >
        <span>
          {t("team.narrow", {
            count: team.helpers.length,
            budget:
              ended?.short ?? t("team.budgetUsed", { percent: budget.percent }),
          })}
        </span>
        <span aria-hidden="true"> ▸</span>
      </button>
    </section>
  );
}

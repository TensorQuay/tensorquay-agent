"use client";

import { useQueries } from "@tanstack/react-query";
import Link from "next/link";
import { useId, useState } from "react";

import { modelName } from "../conversations/model-name";
import { useModels } from "../conversations/use-models";
import { useT } from "../i18n/I18nProvider";
import { formatNumber } from "../i18n/message";
import type { Translate } from "../i18n/translate";
import type { User } from "../protocol/generated/api";
import { Button } from "../ui/Button";
import { Select } from "../ui/Select";
import { Skeleton } from "../ui/Skeleton";
import { TableScroll } from "../ui/TableScroll";

import type {
  AdminApi,
  UsageFigures,
  UsageGroup,
  UsageReport,
  UsageRow,
} from "./admin/contract";

const PERIODS = ["month", "lastMonth", "7", "90"] as const;
type Period = (typeof PERIODS)[number];
type Range = { from: string; to: string };
const DAY_MS = 86_400_000;
/** How many people or models each ranking shows before Show all. */
const TOP = 5;

const day = (value: Date) => value.toISOString().slice(0, 10);
const utc = (year: number, month: number, date: number) =>
  new Date(Date.UTC(year, month, date));

/**
 * A period and the one it is compared with, as the contract's inclusive UTC dates: this month against the same days
 * of last month, last month against the month before, and a run of days against as many days just before it.
 */
function ranges(period: Period, now: Date) {
  const [year, month, date] = [
    now.getUTCFullYear(),
    now.getUTCMonth(),
    now.getUTCDate(),
  ];
  const today = utc(year, month, date);
  if (period === "month") {
    // The same days of last month, as far as it has them (the 31st of March compares with the whole of February).
    const last = utc(year, month, 0).getUTCDate();
    return {
      current: { from: day(utc(year, month, 1)), to: day(today) },
      before: {
        from: day(utc(year, month - 1, 1)),
        to: day(utc(year, month - 1, Math.min(date, last))),
      },
      month: utc(year, month - 1, 1),
    };
  }
  if (period === "lastMonth")
    return {
      current: {
        from: day(utc(year, month - 1, 1)),
        to: day(utc(year, month, 0)),
      },
      before: {
        from: day(utc(year, month - 2, 1)),
        to: day(utc(year, month - 1, 0)),
      },
      month: utc(year, month - 2, 1),
    };
  const days = Number(period);
  const from = new Date(today.getTime() - (days - 1) * DAY_MS);
  return {
    current: { from: day(from), to: day(today) },
    before: {
      from: day(new Date(from.getTime() - days * DAY_MS)),
      to: day(new Date(from.getTime() - DAY_MS)),
    },
    month: null,
  };
}

const tokens = (values: UsageFigures) =>
  values.prompt_tokens + values.completion_tokens;

/** Token usage for administrators (FR-21): the period's total and its change, then who and which model. */
export function UsageSettings({
  me,
  api,
  clock = () => new Date(),
}: {
  me: User;
  api: AdminApi;
  clock?: () => Date;
}) {
  const t = useT();
  if (me.role !== "admin")
    return <p className="muted">{t("usage.adminsOnly")}</p>;
  return <Usage api={api} clock={clock} />;
}

function Usage({ api, clock }: { api: AdminApi; clock: () => Date }) {
  const t = useT();
  const [period, setPeriod] = useState<Period>("month");
  const range = ranges(period, clock());
  const ask = (dates: Range, group: UsageGroup) => ({
    queryKey: ["admin-usage", dates, group],
    queryFn: ({ signal }: { signal: AbortSignal }) =>
      api.usage({ ...dates, group }, signal),
    retry: false,
  });
  const reports = useQueries({
    queries: [
      ask(range.current, "day"),
      ask(range.current, "user"),
      ask(range.current, "route"),
      ask(range.before, "day"),
    ],
  });
  const [days, people, models, before] = reports;
  return (
    <div className="stack">
      <p className="type-small muted">{t("usage.intro")}</p>
      <div className="settings-controls">
        <Select
          label={t("usage.period")}
          value={period}
          onChange={(choice) => setPeriod(choice as Period)}
        >
          {PERIODS.map((value) => (
            <option key={value} value={value}>
              {value === "month"
                ? t("usage.thisMonth")
                : value === "lastMonth"
                  ? t("usage.lastMonth")
                  : t("usage.lastDays", { count: Number(value) })}
            </option>
          ))}
        </Select>
      </div>
      {reports.some((report) => report.isError) ? (
        <div className="stack">
          <p>{t("usage.failed")}</p>
          <div>
            <Button
              variant="secondary"
              onClick={() => reports.forEach((report) => void report.refetch())}
            >
              {t("common.tryAgain")}
            </Button>
          </div>
        </div>
      ) : days?.data && people?.data && models?.data && before?.data ? (
        <Report
          days={days.data}
          people={people.data}
          models={models.data}
          before={before.data}
          range={range.current}
          against={against(t, period, range.month)}
        />
      ) : (
        <Skeleton label={t("usage.loading")} />
      )}
    </div>
  );
}

/** What the period is compared with, in words: "the same days of August", "July", "the 7 days before". */
function against(t: Translate, period: Period, month: Date | null) {
  const name = month
    ? new Intl.DateTimeFormat(t.locale, {
        month: "long",
        timeZone: "UTC",
      }).format(month)
    : "";
  if (period === "month") return t("usage.againstSameDays", { month: name });
  if (period === "lastMonth") return t("usage.againstMonth", { month: name });
  return t("usage.againstDays", { count: Number(period) });
}

function Report({
  days,
  people,
  models,
  before,
  range,
  against,
}: {
  range: Range;
  days: UsageReport;
  people: UsageReport;
  models: UsageReport;
  before: UsageReport;
  against: string;
}) {
  const t = useT();
  const number = (value: number) => formatNumber(value, t.locale);
  // A route without a display name reads as the model picker names it (found before 17.0: not "local").
  const catalogue = useModels();
  const routeName = (row: UsageRow) =>
    row.label ??
    (() => {
      const known = catalogue.data?.items.find((item) => item.name === row.key);
      return known ? modelName(known) : row.key;
    })();
  const totals = days.totals;
  if (totals.requests === 0) return <p className="muted">{t("usage.empty")}</p>;
  const now = tokens(totals);
  const then = tokens(before.totals);
  // A request whose server never reported usage is estimated if it streamed anything (a step stopped mid-reply),
  // otherwise not counted at all.
  const unreported = totals.requests - totals.reported - totals.estimated;
  const detail = (value: number | null) =>
    value === null ? t("usage.notReportedShort") : number(value);
  return (
    <div className="stack">
      {/* The one number first: the period's tokens and their change. */}
      <section className="usage-headline" aria-label={t("usage.total")}>
        {/* Written as every other number on the page (found before 17.0: no "59.2K" above "59 200"). */}
        <p className="type-display">
          {t("usage.headline", { tokens: number(now) })}
        </p>
        <p className="type-body">
          {then === 0
            ? t("usage.noComparison", { against })
            : t("usage.change", {
                change: change(now, then, t.locale),
                against,
              })}
        </p>
        <p className="type-small muted">
          {t("usage.summary", {
            requests: totals.requests,
            input: totals.prompt_tokens,
            output: totals.completion_tokens,
          })}
        </p>
        {/* Only when a model server reported either: a line of "not reported" says nothing. */}
        {(totals.cached_tokens !== null ||
          totals.reasoning_tokens !== null) && (
          <p className="type-small muted">
            {t("usage.details", {
              cached: detail(totals.cached_tokens),
              reasoning: detail(totals.reasoning_tokens),
            })}
          </p>
        )}
        {totals.estimated > 0 && (
          <p className="type-small muted">
            {t("usage.estimated", { count: totals.estimated })}
          </p>
        )}
        {unreported > 0 && (
          <p className="type-small muted">
            {t("usage.unreported", { count: unreported })}
          </p>
        )}
      </section>
      <DayBars report={days} range={range} />
      <div className="usage-rankings">
        <Ranking
          report={people}
          total={now}
          kind="person"
          name={(row) => row.label ?? t("usage.removedAccount")}
        />
        <Ranking report={models} total={now} kind="model" name={routeName} />
      </div>
      <EnginesLine />
      <EachDay report={days} />
    </div>
  );
}

/** "Decision engines have their own budgets.", its first words the link to their page, in any language's order. */
function EnginesLine() {
  const t = useT();
  const [before, after] = t("usage.engines", { link: LINK_MARK }).split(
    LINK_MARK,
  );
  return (
    <p className="type-small muted">
      {before}
      <Link href="/settings/decision-engines">
        {t("settings.decisionEngines")}
      </Link>
      {after}
    </p>
  );
}
const LINK_MARK = "\u0000";

/** "+18%", "−5%" or "0%": the change from `then` to `now`, rounded to a whole percent. */
function change(now: number, then: number, locale: string) {
  const percent = Math.round(((now - then) / then) * 100);
  const sign = percent > 0 ? "+" : percent < 0 ? "−" : "";
  return `${sign}${formatNumber(Math.abs(percent), locale)}%`;
}

/**
 * Every day of the period as a bar, newest on the right; a day with no tokens is a gap, never a zero-height bar. Its
 * first and last dates sit under it and its busiest day is its caption, so one day's bar reads on day one.
 */
function DayBars({ report, range }: { report: UsageReport; range: Range }) {
  const t = useT();
  const caption = useId();
  const byDay = new Map(report.rows.map((row) => [row.key, tokens(row)]));
  const all: [string, number][] = [];
  for (
    let at = new Date(`${range.from}T00:00:00Z`).getTime();
    at <= new Date(`${range.to}T00:00:00Z`).getTime();
    at += DAY_MS
  ) {
    const key = day(new Date(at));
    all.push([key, byDay.get(key) ?? 0]);
  }
  // The busiest day, the latest of equals, names the chart for a screen reader.
  const busiest = all.reduce((best, entry) =>
    entry[1] >= best[1] ? entry : best,
  );
  const dates = new Intl.DateTimeFormat(t.locale, {
    dateStyle: "medium",
    timeZone: "UTC",
  });
  const first = dates.format(new Date(`${range.from}T00:00:00Z`));
  const last = dates.format(new Date(`${range.to}T00:00:00Z`));
  return (
    <figure className="usage-chart">
      <div className="usage-days" role="img" aria-labelledby={caption}>
        {all.map(([key, value]) => (
          <span
            key={key}
            className="usage-day"
            data-day={key}
            data-empty={value === 0 || undefined}
            title={`${dates.format(new Date(`${key}T00:00:00Z`))}: ${formatNumber(value, t.locale)}`}
            style={{
              // A share of the busiest day's height; the stylesheet turns it into the bar.
              ["--share" as string]: busiest[1] ? value / busiest[1] : 0,
            }}
          />
        ))}
      </div>
      <div className="usage-axis type-small muted" aria-hidden="true">
        <span>{first}</span>
        {last !== first && <span>{last}</span>}
      </div>
      <figcaption id={caption} className="type-small muted">
        {t("usage.chart", {
          day: dates.format(new Date(`${busiest[0]}T00:00:00Z`)),
          tokens: busiest[1],
        })}
      </figcaption>
    </figure>
  );
}

/** The two rankings' table names, column headings and Show all buttons. */
const RANKINGS = {
  person: {
    title: "usage.tablePerson",
    heading: "usage.person",
    all: "usage.showAllPeople",
  },
  model: {
    title: "usage.tableModel",
    heading: "usage.model",
    all: "usage.showAllModels",
  },
} as const;

/** People or models by tokens, largest first: the top five, the rest on request, each with its share of the total. */
function Ranking({
  report,
  total,
  kind,
  name,
}: {
  report: UsageReport;
  total: number;
  kind: keyof typeof RANKINGS;
  name: (row: UsageRow) => string;
}) {
  const t = useT();
  const { title, heading, all } = RANKINGS[kind];
  const [open, setOpen] = useState(false);
  const rows = [...report.rows].sort((a, b) => tokens(b) - tokens(a));
  const shown = open ? rows : rows.slice(0, TOP);
  return (
    <div className="stack usage-ranking">
      <TableScroll label={t(title)}>
        <table className="data-table" aria-label={t(title)}>
          <thead className="type-label">
            <tr>
              <th scope="col">{t(heading)}</th>
              <th scope="col" className="numeric">
                {t("usage.tokens")}
              </th>
              <th scope="col" className="numeric">
                {t("usage.share")}
              </th>
            </tr>
          </thead>
          <tbody>
            {shown.map((row) => {
              const share = total ? tokens(row) / total : 0;
              return (
                <tr key={row.key}>
                  <td className="cell-wrap">{name(row)}</td>
                  <td className="numeric">
                    {formatNumber(tokens(row), t.locale)}
                  </td>
                  <td className="numeric">
                    <span
                      className="usage-share"
                      style={{ ["--share" as string]: share }}
                    >
                      {`${formatNumber(Math.round(share * 100), t.locale)}%`}
                    </span>
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </TableScroll>
      {!open && rows.length > TOP && (
        <div>
          <Button variant="ghost" onClick={() => setOpen(true)}>
            {t(all, { count: rows.length })}
          </Button>
        </div>
      )}
      {report.truncated && (
        <p className="type-small muted">
          {t("usage.truncated", { count: report.rows.length })}
        </p>
      )}
    </div>
  );
}

/** The daily table, on request only: newest first, days without requests left out. */
function EachDay({ report }: { report: UsageReport }) {
  const t = useT();
  const [open, setOpen] = useState(false);
  const dates = new Intl.DateTimeFormat(t.locale, {
    dateStyle: "medium",
    timeZone: "UTC",
  });
  if (!open)
    return (
      <div>
        <Button variant="secondary" onClick={() => setOpen(true)}>
          {t("usage.showEachDay")}
        </Button>
      </div>
    );
  const rows = report.rows
    .filter((row) => row.requests > 0)
    .sort((a, b) => b.key.localeCompare(a.key));
  return (
    <TableScroll label={t("usage.tableDay")}>
      <table className="data-table" aria-label={t("usage.tableDay")}>
        <thead className="type-label">
          <tr>
            <th scope="col">{t("usage.day")}</th>
            <th scope="col" className="numeric">
              {t("usage.requests")}
            </th>
            <th scope="col" className="numeric">
              {t("usage.input")}
            </th>
            <th scope="col" className="numeric">
              {t("usage.output")}
            </th>
          </tr>
        </thead>
        <tbody>
          {rows.map((row) => (
            <tr key={row.key}>
              <td>{dates.format(new Date(`${row.key}T00:00:00Z`))}</td>
              <td className="numeric">
                {formatNumber(row.requests, t.locale)}
              </td>
              <td className="numeric">
                {formatNumber(row.prompt_tokens, t.locale)}
              </td>
              <td className="numeric">
                {formatNumber(row.completion_tokens, t.locale)}
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </TableScroll>
  );
}

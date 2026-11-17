"use client";

import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Boxes } from "lucide-react";
import { useId, useState } from "react";

import { newActionKey } from "../api/action-key";
import { ApiError } from "../api/error";
import { RelativeTime } from "../conversations/RelativeTime";
import { useT } from "../i18n/I18nProvider";
import { formatNumber } from "../i18n/message";
import type { MessageKey, Translate } from "../i18n/translate";
import type { User } from "../protocol/generated/api";
import { copyText } from "../transcript/copy-text";
import { Button } from "../ui/Button";
import { ConfirmDialog } from "../ui/ConfirmDialog";
import { EmptyState } from "../ui/EmptyState";
import { Skeleton } from "../ui/Skeleton";
import { TableScroll } from "../ui/TableScroll";

import type {
  LeftBehind,
  SandboxListing,
  SandboxRow,
  SandboxesApi,
} from "./admin/sandboxes";
import { LoadFailure } from "./LoadFailure";
import { RowMenu } from "./RowMenu";

/** How often an open Sandboxes page reads the listing again: the sampler's own period. */
export const SANDBOXES_REFRESH_MS = 10_000;

const GIB = 2 ** 30;

/** Cores from millicores, to a tenth. */
const cores = (millicores: number, locale: string) =>
  formatNumber(Math.round(millicores / 100) / 10, locale);
/** Gigabytes (binary) from bytes, to a tenth. */
const gigabytes = (bytes: number, locale: string) =>
  formatNumber(Math.round((bytes / GIB) * 10) / 10, locale);

const STATE_LABEL: Readonly<Record<SandboxRow["state"], MessageKey>> = {
  starting: "sandboxes.stateStarting",
  working: "sandboxes.stateWorking",
  idle: "sandboxes.stateIdle",
  stopping: "sandboxes.stateStopping",
};
const STATE_TONE: Readonly<Record<SandboxRow["state"], string>> = {
  starting: "info",
  working: "info",
  idle: "muted",
  stopping: "muted",
};
const REFUSAL_LABEL: Readonly<
  Record<NonNullable<SandboxRow["stop_refusal"]>, MessageKey>
> = {
  starting: "sandboxes.refusedStarting",
  working: "sandboxes.refusedWorking",
  stopping: "sandboxes.refusedStopping",
  work_waiting: "sandboxes.refusedWaiting",
};

/** The sentence for a stop the gateway refused, or null for a failure the generic words cover. */
export function stopRefusal(
  error: Error,
  email: string,
  t: Translate,
): string | null {
  if (!(error instanceof ApiError)) return null;
  if (error.code === "working") return t("sandboxes.stopWorking", { email });
  if (error.code === "work_waiting")
    return t("sandboxes.stopWaiting", { email });
  if (error.code === "not_found") return t("sandboxes.stopGone");
  return null;
}

/** "About 6 min" or "about 7 h" until `at`, never a countdown to the second (the note's §3.1); null once past. */
function about(at: string, now: number, t: Translate): string | null {
  const minutes = Math.round((Date.parse(at) - now) / 60_000);
  if (minutes < 1) return null;
  return minutes < 60
    ? t("sandboxes.stopsInMinutes", { count: minutes })
    : t("sandboxes.stopsInHours", { count: Math.round(minutes / 60) });
}

/** The running sandboxes for administrators (FR-22; the Sandboxes page design note): who runs what, and Stop now. */
export function SandboxesSettings({
  me,
  api,
  clock = () => Date.now(),
}: {
  me: User;
  api: SandboxesApi;
  clock?: () => number;
}) {
  const t = useT();
  if (me.role !== "admin")
    return <p className="muted">{t("sandboxes.adminsOnly")}</p>;
  return <Sandboxes me={me} api={api} clock={clock} />;
}

function Sandboxes({
  me,
  api,
  clock,
}: {
  me: User;
  api: SandboxesApi;
  clock: () => number;
}) {
  const t = useT();
  const queries = useQueryClient();
  const listing = useQuery({
    queryKey: ["admin-sandboxes"],
    queryFn: ({ signal }) => api.sandboxes(undefined, signal),
    retry: false,
    refetchInterval: SANDBOXES_REFRESH_MS,
  });
  // Pages past the first, read on Show more and kept until the first page is read again.
  const [more, setMore] = useState<SandboxRow[]>([]);
  const [moreAfter, setMoreAfter] = useState<string | null>(null);
  const [stopping, setStopping] = useState<SandboxRow | null>(null);
  const [status, setStatus] = useState("");
  const stop = useMutation({
    mutationFn: (row: SandboxRow) =>
      api.stop(row.conversation_id, newActionKey()),
    onSuccess: (_, row) => {
      setStatus(t("sandboxes.stopped", { email: row.owner.email }));
      setStopping(null);
      void queries.invalidateQueries({ queryKey: ["admin-sandboxes"] });
    },
    onError: (error) => {
      // Gone already: the list is read again, and the dialog says so meanwhile.
      if (error instanceof ApiError && error.code === "not_found")
        void queries.invalidateQueries({ queryKey: ["admin-sandboxes"] });
    },
  });
  const nextPage = useMutation({
    mutationFn: (after: string) => api.sandboxes(after),
    onSuccess: (page) => {
      setMore((rows) => [...rows, ...page.items]);
      setMoreAfter(page.next_after);
    },
  });
  if (listing.isPending) return <Skeleton label={t("sandboxes.loading")} />;
  if (listing.isError && !listing.data)
    return (
      <LoadFailure
        message={t("sandboxes.failed")}
        onRetry={() => void listing.refetch()}
      />
    );
  const data = listing.data;
  // The first page's next token until Show more has read on from it.
  const after = more.length > 0 ? moreAfter : data.next_after;
  const rows = [...data.items, ...more];
  const now = clock();
  return (
    <div className="stack">
      <Summary data={data} t={t} />
      {/* Quiet, always there and live, so a failed 10 s read is heard once, as Status does it. */}
      <p role="status" className="type-small muted">
        {listing.isError ? t("sandboxes.readFailed") : null}
      </p>
      {data.left_behind.length > 0 && (
        <LeftBehindSection items={data.left_behind} />
      )}
      {rows.length === 0 ? (
        <EmptyState icon={Boxes} message={t("sandboxes.empty")} action={null} />
      ) : (
        <TableScroll label={t("sandboxes.tableLabel")}>
          <table className="data-table" aria-label={t("sandboxes.tableLabel")}>
            <thead className="type-label">
              <tr>
                <th scope="col">{t("sandboxes.owner")}</th>
                <th scope="col">{t("sandboxes.state")}</th>
                <th scope="col">{t("sandboxes.environment")}</th>
                <th scope="col" className="numeric">
                  {t("sandboxes.cpu")}
                </th>
                <th scope="col" className="numeric">
                  {t("sandboxes.memory")}
                </th>
                <th scope="col">{t("sandboxes.started")}</th>
                <th scope="col">{t("sandboxes.lastActivity")}</th>
                <th scope="col">
                  <span className="sr-only">{t("common.actions")}</span>
                </th>
              </tr>
            </thead>
            <tbody>
              {rows.map((row) => (
                <Row
                  key={row.conversation_id}
                  row={row}
                  own={row.owner.id === me.id}
                  measured={data.totals.usage !== null}
                  pending={
                    stop.isPending &&
                    stop.variables?.conversation_id === row.conversation_id
                  }
                  now={now}
                  onStop={() => {
                    stop.reset();
                    setStopping(row);
                  }}
                />
              ))}
            </tbody>
          </table>
        </TableScroll>
      )}
      {after !== null && (
        <div className="stack">
          <p className="type-small muted">
            {t("sandboxes.showing", {
              shown: rows.length,
              total: data.totals.in_use,
            })}
          </p>
          <div>
            <Button
              variant="secondary"
              loading={nextPage.isPending}
              onClick={() => nextPage.mutate(after)}
            >
              {t("sandboxes.showMore")}
            </Button>
          </div>
        </div>
      )}
      {/* What the last stop did, for a screen reader and for the eye, under the table. */}
      <p role="status" className="type-small muted">
        {status}
      </p>
      {stopping && (
        <ConfirmDialog
          title={
            stopping.owner.id === me.id
              ? t("sandboxes.stopOwnTitle")
              : t("sandboxes.stopTitle", { email: stopping.owner.email })
          }
          confirm={t("sandboxes.stopConfirm")}
          pending={stop.isPending}
          error={stop.error}
          errorCopy={(error) => stopRefusal(error, stopping.owner.email, t)}
          onConfirm={() => stop.mutate(stopping)}
          onClose={() => setStopping(null)}
        >
          {t("sandboxes.stopBody")}
          {stopping.kept_on ? ` ${t("sandboxes.stopKeptOn")}` : ""}
          {stopping.kept_on && stopping.environment.desktop
            ? ` ${t("sandboxes.stopKeptSignIn")}`
            : ""}
        </ConfirmDialog>
      )}
    </div>
  );
}

/** The headline and the three lines under it (the note's §4.1): running of slots, waiting, desktops, in use, how. */
function Summary({ data, t }: { data: SandboxListing; t: Translate }) {
  const { totals, limits } = data;
  const full = totals.in_use >= limits.slots;
  const used = totals.usage;
  return (
    <div className="stack sandboxes-summary">
      <p
        className="type-display sandboxes-headline"
        data-tone={full ? "warn" : undefined}
      >
        {t("sandboxes.headline", {
          running: totals.in_use,
          slots: limits.slots,
          full: full ? "yes" : "no",
        })}
      </p>
      {totals.waiting > 0 && (
        <p>{t("sandboxes.waiting", { count: totals.waiting })}</p>
      )}
      {totals.desktops > 0 && (
        <p className="type-small">
          {t("sandboxes.desktops", {
            running: totals.desktops,
            slots: limits.desktops,
          })}
        </p>
      )}
      <p className="type-small">
        {used
          ? t("sandboxes.inUse", {
              cores: cores(used.cpu_millicores, t.locale),
              reservedCores: cores(totals.reserved.cpu_millicores, t.locale),
              memory: gigabytes(used.memory_bytes, t.locale),
              reservedMemory: gigabytes(totals.reserved.memory_bytes, t.locale),
            })
          : t("sandboxes.reserved", {
              cores: cores(totals.reserved.cpu_millicores, t.locale),
              memory: gigabytes(totals.reserved.memory_bytes, t.locale),
            })}
      </p>
      <p className="type-small muted">
        {t(
          data.backend === "firecracker"
            ? "sandboxes.runsMicroVm"
            : "sandboxes.runsDocker",
        )}
      </p>
    </div>
  );
}

function Row({
  row,
  own,
  measured,
  pending,
  now,
  onStop,
}: {
  row: SandboxRow;
  own: boolean;
  /** Whether any sandbox has been measured: before the first sample every row says "Measuring…". */
  measured: boolean;
  pending: boolean;
  now: number;
  onStop: () => void;
}) {
  const t = useT();
  const email = own ? t("sandboxes.you") : row.owner.email;
  const stops = row.stops_at ? about(row.stops_at, now, t) : null;
  const unavailable = pending
    ? t("sandboxes.stopping")
    : row.stop_refusal
      ? t(REFUSAL_LABEL[row.stop_refusal])
      : undefined;
  return (
    <tr>
      <td>
        <div className="data-name data-name-wrap" title={row.owner.email}>
          <span className="cell-whole">{email}</span>
        </div>
      </td>
      <td>
        <span
          className="status-pill type-label"
          data-tone={STATE_TONE[row.state]}
        >
          {pending ? t("sandboxes.stateStopping") : t(STATE_LABEL[row.state])}
        </span>
        {row.kept_on ? (
          <span className="cell-note" title={t("sandboxes.keptOn")}>
            {t("sandboxes.keptOn")}
          </span>
        ) : stops ? (
          <span className="cell-note" title={stops}>
            {stops}
          </span>
        ) : null}
      </td>
      <td>
        <span className="cell-whole">{row.environment.display_name}</span>
      </td>
      <td className="numeric">
        <Measure
          usage={row.usage?.cpu_millicores ?? null}
          size={row.size.cpu_millicores}
          unit="cores"
          measured={measured}
        />
      </td>
      <td className="numeric">
        <Measure
          usage={row.usage?.memory_bytes ?? null}
          size={row.size.memory_bytes}
          unit="memory"
          measured={measured}
        />
      </td>
      <td>
        <RelativeTime value={row.started_at} />
      </td>
      <td>
        <RelativeTime value={row.last_activity_at} />
      </td>
      <td className="data-cell-action">
        <RowMenu
          label={t("sandboxes.moreName", { email: row.owner.email })}
          actions={[
            {
              id: "stop",
              label: t("sandboxes.stopNow"),
              name: t("sandboxes.stopNowName", { email: row.owner.email }),
              danger: true,
              unavailable,
              onSelect: onStop,
            },
          ]}
        />
      </td>
    </tr>
  );
}

/** "0.2 of 2 cores" or "0.4 of 4 GB": the use against the size, the size alone until measured. */
function Measure({
  usage,
  size,
  unit,
  measured,
}: {
  usage: number | null;
  size: number | null;
  unit: "cores" | "memory";
  measured: boolean;
}) {
  const t = useT();
  const format = unit === "cores" ? cores : gigabytes;
  const key: MessageKey =
    unit === "cores" ? "sandboxes.ofCores" : "sandboxes.ofGb";
  if (usage !== null && size !== null)
    return (
      <span>
        {t(key, {
          used: format(usage, t.locale),
          size: format(size, t.locale),
        })}
      </span>
    );
  return (
    <span>
      <span className="muted">
        {measured ? t("sandboxes.notAvailable") : t("sandboxes.measuring")}
      </span>
      {size !== null && (
        <span className="cell-note">
          {t(unit === "cores" ? "sandboxes.sizeCores" : "sandboxes.sizeGb", {
            size: format(size, t.locale),
          })}
        </span>
      )}
    </span>
  );
}

/** What survived a sweep (the note's §3.6): each by kind, name, age and reason, with the command that removes it. */
function LeftBehindSection({ items }: { items: readonly LeftBehind[] }) {
  const t = useT();
  const heading = useId();
  const [copied, setCopied] = useState<string | null>(null);
  return (
    <section
      className="stack sandboxes-left-behind"
      aria-labelledby={heading}
      data-tone="warn"
    >
      <h2 id={heading} className="type-heading">
        {t("sandboxes.leftBehindTitle")}
      </h2>
      <p className="type-small">{t("sandboxes.leftBehindIntro")}</p>
      <ul className="stack">
        {items.map((item) => (
          <li key={`${item.kind}-${item.name}`} className="stack">
            <span>
              {t("sandboxes.leftBehindItem", {
                kind: item.kind,
                name: item.name,
                age: Math.max(1, Math.round(item.age_seconds / 60)),
                reason: item.reason,
              })}
            </span>
            <span className="sandboxes-remove">
              <code className="type-code code-wrap">{item.remove}</code>
              <Button
                variant="secondary"
                onClick={() => {
                  void copyText(item.remove).then(() => setCopied(item.remove));
                }}
              >
                {copied === item.remove ? t("common.copied") : t("common.copy")}
              </Button>
            </span>
          </li>
        ))}
      </ul>
    </section>
  );
}

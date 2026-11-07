"use client";

import { useInfiniteQuery } from "@tanstack/react-query";
import Link from "next/link";

import { ApiError } from "../api/error";
import { useT } from "../i18n/I18nProvider";
import type { MessageKey } from "../i18n/translate";
import type { SecretRead, SecretReadPage } from "../protocol/generated/api";
import { Button } from "../ui/Button";
import { Dialog } from "../ui/Dialog";
import { Skeleton } from "../ui/Skeleton";
import { TableScroll } from "../ui/TableScroll";

import { DateLabel } from "./DateLabel";
import { LoadFailure } from "./LoadFailure";

const OUTCOMES: Readonly<
  Record<SecretRead["outcome"], { copy: MessageKey; tone: string }>
> = {
  ok: { copy: "reads.ok", tone: "muted" },
  absent: { copy: "reads.absent", tone: "warn" },
  unavailable: { copy: "reads.unavailable", tone: "danger" },
  foreign_run: { copy: "reads.foreignRun", tone: "warn" },
  foreign_sandbox: { copy: "reads.foreignSandbox", tone: "warn" },
  expired: { copy: "reads.expired", tone: "warn" },
  wrong_host: { copy: "reads.wrongHost", tone: "warn" },
  wrong_position: { copy: "reads.wrongPosition", tone: "warn" },
  not_allowed: { copy: "reads.notAllowed", tone: "warn" },
  revoked: { copy: "reads.revoked", tone: "warn" },
  unrecorded: { copy: "reads.unrecorded", tone: "danger" },
};
const CONSUMERS: Readonly<Record<SecretRead["consumer_kind"], MessageKey>> = {
  route: "reads.route",
  connector: "reads.connector",
  git: "reads.saving",
  engine: "reads.engine",
  host: "reads.host",
};

/**
 * Who read a secret, for which task and with what outcome, newest first, 50 at a time (vault slice 2; WEB-U-053). There
 * is never a value to show. `emails` is given only to administrators, who see each read's person.
 */
export function SecretReads({
  title,
  load,
  emails,
  missing,
  onClose,
}: {
  title: string;
  load: (before?: string, signal?: AbortSignal) => Promise<SecretReadPage>;
  emails?: Record<string, string>;
  /** What a 404 means on a person's own view: they hold no credential there. */
  missing?: MessageKey;
  onClose: () => void;
}) {
  const t = useT();
  const reads = useInfiniteQuery({
    queryKey: ["secret-reads", title],
    queryFn: ({ pageParam, signal }) => load(pageParam, signal),
    initialPageParam: undefined as string | undefined,
    getNextPageParam: (page) => page.next ?? undefined,
    retry: false,
  });
  let body;
  if (reads.isPending) body = <Skeleton label={t("reads.loading")} />;
  else if (reads.isError && !reads.data)
    body =
      missing &&
      reads.error instanceof ApiError &&
      reads.error.status === 404 ? (
        <p className="muted">{t(missing)}</p>
      ) : (
        <LoadFailure
          message={t("reads.failed")}
          onRetry={() => void reads.refetch()}
        />
      );
  else {
    const items = reads.data?.pages.flatMap((page) => page.items) ?? [];
    body =
      items.length === 0 ? (
        <p className="muted">{t("reads.none")}</p>
      ) : (
        <div className="stack">
          <ReadsTable items={items} emails={emails} />
          {reads.isFetchNextPageError && (
            <p className="field-error">{t("reads.olderFailed")}</p>
          )}
          {reads.hasNextPage && (
            <div>
              <Button
                variant="secondary"
                loading={reads.isFetchingNextPage}
                onClick={() => void reads.fetchNextPage()}
              >
                {t("reads.older")}
              </Button>
            </div>
          )}
        </div>
      );
  }
  return (
    <Dialog open title={title} className="reads-dialog" onClose={onClose}>
      {body}
    </Dialog>
  );
}

function ReadsTable({
  items,
  emails,
}: {
  items: SecretRead[];
  emails: Record<string, string> | undefined;
}) {
  const t = useT();
  // A column shows only when a row fills it (UI-GUIDELINES §7, WEB-U-183): a read's task, or a run it didn't name
  // (Run-ID 2; a connector's read never names one). Without any task, "No task" isn't repeated on every row.
  const unknownRun = (item: SecretRead) =>
    item.run_id === null && item.consumer_kind !== "connector";
  const tasks = items.some((item) => item.conversation_id !== null);
  const column = tasks || items.some(unknownRun);
  return (
    <TableScroll label={t("reads.list")}>
      <table className="data-table" aria-label={t("reads.list")}>
        <thead className="type-label">
          <tr>
            <th scope="col">{t("reads.when")}</th>
            <th scope="col">{t("reads.usedBy")}</th>
            {column && <th scope="col">{t("reads.task")}</th>}
            {emails && <th scope="col">{t("reads.person")}</th>}
            <th scope="col">{t("reads.outcome")}</th>
          </tr>
        </thead>
        <tbody>
          {items.map((item) => {
            const outcome = OUTCOMES[item.outcome];
            return (
              <tr key={item.id}>
                <td>
                  <span className="stack route-name">
                    <DateLabel value={item.last_at} />
                    {item.count > 1 && (
                      <span className="type-small muted">
                        {t("reads.since", {
                          count: item.count,
                          date: new Intl.DateTimeFormat(t.locale, {
                            dateStyle: "medium",
                          }).format(new Date(item.first_at)),
                        })}
                      </span>
                    )}
                  </span>
                </td>
                <td>
                  {t(CONSUMERS[item.consumer_kind], { name: item.consumer })}
                </td>
                {column && (
                  <td>
                    <span className="stack route-name">
                      {item.conversation_id ? (
                        <Link href={`/c/${item.conversation_id}`}>
                          {t("reads.openTask")}
                        </Link>
                      ) : (
                        tasks && (
                          <span className="muted">{t("reads.noTask")}</span>
                        )
                      )}
                      {/* Run-ID (2): the read named no run, or not the task's own. A connector's never carries one (ADR-0035). */}
                      {unknownRun(item) && (
                        <span className="type-small muted">
                          {t("reads.runUnknown")}
                        </span>
                      )}
                    </span>
                  </td>
                )}
                {emails && (
                  <td className="read-person">
                    {item.user_id ? (
                      (emails[item.user_id] ?? (
                        <span className="muted">{t("reads.removedUser")}</span>
                      ))
                    ) : (
                      <span className="muted">{t("reads.noPerson")}</span>
                    )}
                  </td>
                )}
                <td>
                  <span
                    className="status-pill type-label"
                    data-tone={outcome.tone}
                  >
                    {t(outcome.copy)}
                  </span>
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </TableScroll>
  );
}

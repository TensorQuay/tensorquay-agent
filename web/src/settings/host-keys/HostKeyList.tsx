"use client";

import { useQuery, useQueryClient } from "@tanstack/react-query";
import { KeyRound, Plus } from "lucide-react";
import Link from "next/link";
import { useEffect, useId, useState } from "react";

import { ApiError } from "../../api/error";
import { useT } from "../../i18n/I18nProvider";
import { EmptyState } from "../../ui/EmptyState";
import { Icon } from "../../ui/Icon";
import { Skeleton } from "../../ui/Skeleton";
import { TableScroll } from "../../ui/TableScroll";
import { LoadFailure } from "../LoadFailure";

import { formatAddress, isIpAddress } from "./address";
import type { HostKey, HostKeysApi } from "./contract";
import { lastUseWords } from "./last-use";

export const HOST_KEYS = ["host-keys"];
/** What the Add page just did, for the list to say once. */
export const HOST_KEYS_NOTICE = ["host-keys-notice"];
export const KEYS_PAGE = "/settings/keys";
export const ADD_KEY = `${KEYS_PAGE}/new`;

/**
 * Your account › Keys › Mine, the per-service keys (ADR-0037; the reviewed Keys sketch §2): each key by the name its
 * tasks read, its web address, and when it was last used and by which task; a quiet warning when tasks can't reach its
 * host. A gateway without host keys answers 404, and the list leaves itself out.
 */
export function HostKeyList({
  api,
  admin,
  now = () => new Date(),
  timeZone,
}: {
  api: HostKeysApi;
  /** An administrator can allow a host; a member is told who can. */
  admin: boolean;
  now?: () => Date;
  /** The reader's time zone unless given (tests pass one). */
  timeZone?: string;
}) {
  const t = useT();
  const cache = useQueryClient();
  const heading = useId();
  // What the Add page just did, said here once it sent the person back.
  const [notice] = useState<string | null>(
    () => cache.getQueryData<string>(HOST_KEYS_NOTICE) ?? null,
  );
  useEffect(() => {
    cache.removeQueries({ queryKey: HOST_KEYS_NOTICE, exact: true });
  }, [cache]);
  const keys = useQuery({
    queryKey: HOST_KEYS,
    queryFn: ({ signal }) => api.keys(signal),
    retry: false,
  });
  if (keys.error instanceof ApiError && keys.error.status === 404) return null;
  const add = (
    <Link href={ADD_KEY} className="button" data-variant="secondary">
      <span className="button-label">
        <Icon glyph={Plus} />
        {t("hostKeys.add")}
      </span>
    </Link>
  );
  let body;
  if (keys.isPending) body = <Skeleton label={t("hostKeys.loading")} />;
  else if (keys.isError)
    body = (
      <LoadFailure
        message={t("hostKeys.failed")}
        onRetry={() => void keys.refetch()}
      />
    );
  else if (keys.data.items.length === 0)
    body = (
      <EmptyState icon={KeyRound} message={t("hostKeys.empty")} action={add} />
    );
  else
    body = (
      <>
        <div>{add}</div>
        <TableScroll label={t("hostKeys.list")}>
          <table className="data-table" aria-label={t("hostKeys.list")}>
            <thead className="type-label">
              <tr>
                <th scope="col">{t("hostKeys.name")}</th>
                <th scope="col">{t("hostKeys.address")}</th>
                <th scope="col">{t("hostKeys.lastUsed")}</th>
              </tr>
            </thead>
            <tbody>
              {keys.data.items.map((key) => (
                <KeyRow
                  key={key.id}
                  item={key}
                  admin={admin}
                  now={now()}
                  timeZone={timeZone}
                />
              ))}
            </tbody>
          </table>
        </TableScroll>
      </>
    );
  return (
    <section className="stack" aria-labelledby={heading}>
      <h2 id={heading} className="type-body-strong">
        {t("hostKeys.title")}
      </h2>
      {/* What the section is for, beside the connectors' keys below it (the c14b first hour, F1; WEB-U-262). */}
      <p className="type-small muted">{t("hostKeys.intro")}</p>
      {notice && (
        <p className="type-small" role="status">
          {notice}
        </p>
      )}
      {body}
    </section>
  );
}

function KeyRow({
  item,
  admin,
  now,
  timeZone,
}: {
  item: HostKey;
  admin: boolean;
  now: Date;
  timeZone: string | undefined;
}) {
  const t = useT();
  const use = lastUseWords(item.last_use, t, now, timeZone);
  // Only a rule for exactly that address lets tasks reach an address (ADR-0037 Amendment 2).
  const address = isIpAddress(item.host);
  return (
    <tr>
      <td>
        <span className="stack route-name">
          {/* The name as the task reads it: the person's own word, in the code font; it opens the key's page. */}
          <Link href={`${KEYS_PAGE}/${item.id}`}>
            <code className="type-code">{item.name}</code>
          </Link>
          {item.check_name && (
            <span className="status-pill type-label" data-tone="warn">
              {t("hostKeys.checkName")}
            </span>
          )}
        </span>
      </td>
      <td className="cell-wrap">
        <span className="stack route-name">
          {formatAddress(item)}
          {!item.allowed &&
            (admin ? (
              <span className="type-small muted">
                {t(
                  address
                    ? "hostKeys.notAllowedAddress"
                    : "hostKeys.notAllowed",
                  {
                    host: item.host,
                  },
                )}{" "}
                <Link
                  // Internet access opens Add a site ready (WEB-U-150); a port other than 443 goes too, so the rule
                  // matches the key's.
                  href={`/settings/internet-access?host=${encodeURIComponent(item.host)}${
                    item.port === 443 ? "" : `&port=${String(item.port)}`
                  }`}
                >
                  {t("hostKeys.allowHost", { host: item.host })}
                </Link>
              </span>
            ) : (
              <span className="type-small muted">
                {t(
                  address
                    ? "hostKeys.notAllowedAddressMember"
                    : "hostKeys.notAllowedMember",
                  { host: item.host },
                )}
              </span>
            ))}
        </span>
      </td>
      <td>
        <span title={use.title ?? undefined}>{use.text}</span>
      </td>
    </tr>
  );
}

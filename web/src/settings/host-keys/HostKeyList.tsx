"use client";

import { useQuery, useQueryClient } from "@tanstack/react-query";
import { KeyRound, Plus } from "lucide-react";
import Link from "next/link";
import { useEffect, useId, useState } from "react";

import { ApiError } from "../../api/error";
import { useT } from "../../i18n/I18nProvider";
import { CellNote } from "../../ui/CellNote";
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
 * Your account › Keys › Mine, the per-service keys (ADR-0037): each key by the name its tasks read, its web address,
 * and when it was last used and by which task; a quiet warning when tasks can't reach its
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
  // The view's one primary (UI-GUIDELINES §7), also as its empty state's one action: the connectors' Add below is
  // secondary (17.3).
  const add = (
    <Link href={ADD_KEY} className="button" data-variant="primary">
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
      {/* What the section is for, beside the connectors' keys below it (a first-hour check before 17.0; WEB-U-262). */}
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
      <td>
        {/* The address, then, while tasks can't reach it, why in one muted line: an administrator's ends with where to
            allow it, never cut (17.3, UI-GUIDELINES §7 Table). */}
        <span className="stack route-name">
          <span className="cell-cut" title={formatAddress(item)}>
            {formatAddress(item)}
          </span>
          {!item.allowed &&
            (admin ? (
              <CellNote
                before={t(
                  address
                    ? "hostKeys.notAllowedAddress"
                    : "hostKeys.notAllowed",
                  { host: item.host },
                )}
                // Internet access opens Add a site ready (WEB-U-150); a port other than 443 goes too, so the rule
                // matches the key's.
                href={`/settings/internet-access?host=${encodeURIComponent(item.host)}${
                  item.port === 443 ? "" : `&port=${String(item.port)}`
                }`}
              >
                {t("hostKeys.allowHost", { host: item.host })}
              </CellNote>
            ) : (
              <CellNote>
                {t(
                  address
                    ? "hostKeys.notAllowedAddressMember"
                    : "hostKeys.notAllowedMember",
                  { host: item.host },
                )}
              </CellNote>
            ))}
        </span>
      </td>
      <td>
        {/* One line, cut where its column ends: the tooltip has it whole, then, on its own line, the use's details (17.3). */}
        <span
          className="cell-cut"
          title={[use.text, use.title].filter(Boolean).join("\n")}
        >
          {use.text}
        </span>
      </td>
    </tr>
  );
}

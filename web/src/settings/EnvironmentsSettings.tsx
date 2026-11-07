"use client";

import { useQuery, useQueryClient } from "@tanstack/react-query";
import Link from "next/link";
import { useEffect, useRef, useState } from "react";

import { capabilities, type Capability } from "../conversations/capabilities";
import { useLanguage, useT } from "../i18n/I18nProvider";
import { formatMib } from "../i18n/message";
import type { MessageKey } from "../i18n/translate";
import type { User } from "../protocol/generated/api";
import { Icon } from "../ui/Icon";
import { Skeleton } from "../ui/Skeleton";
import { TableScroll } from "../ui/TableScroll";

import {
  checking,
  ENVIRONMENTS_KEY,
  NOTICE_KEY,
  offered,
  type EnvironmentItem,
  type TemplatesApi,
} from "./admin/templates";
import {
  checkState,
  reasonKey,
  unavailableKey,
  unavailableReason,
} from "./environment-copy";
import {
  DeleteEnvironment,
  MakeDefault,
  ResetEnvironment,
} from "./EnvironmentDialogs";
import { EnvironmentRootfsMissing } from "./EnvironmentRootfsMissing";
import { LoadFailure } from "./LoadFailure";
import { RowMenu } from "./RowMenu";
import { hasScanLimit, SecretFiles } from "./SecretFiles";

const KEY = ENVIRONMENTS_KEY;
/** While a check is pending, the list asks again: often at first, then less, and not past the checker's own backoff. */
const POLL_FAST_MS = 5_000;
const POLL_SLOW_MS = 30_000;
const POLL_FAST_FOR_MS = 60_000;
const POLL_STOP_AFTER_MS = 10 * 60_000;
const SOURCES: Readonly<Record<string, MessageKey>> = {
  current: "environments.fromSeed",
  removed: "environments.seedRemoved",
  none: "environments.addedHere",
};
const STATES: Readonly<
  Record<ReturnType<typeof checkState>, [MessageKey, string]>
> = {
  verified: ["environments.verified", "ok"],
  refused: ["environments.refused", "danger"],
  checking: ["environments.checking", "info"],
  unchecked: ["environments.unchecked", "muted"],
};
type Open =
  | { remove: EnvironmentItem }
  | { reset: EnvironmentItem }
  | { makeDefault: EnvironmentItem }
  | null;

/** The environments (templates) for administrators (ADR-0032): what tasks run in, and whether each image passed. */
export function EnvironmentsSettings({
  me,
  api,
}: {
  me: User;
  api: TemplatesApi;
}) {
  const t = useT();
  if (me.role !== "admin")
    return <p className="muted">{t("environments.adminsOnly")}</p>;
  return <Environments api={api} />;
}

function Environments({ api }: { api: TemplatesApi }) {
  const t = useT();
  const cache = useQueryClient();
  const pendingSince = useRef<number | null>(null);
  const list = useQuery({
    queryKey: KEY,
    queryFn: ({ signal }) => api.environments(signal),
    retry: false,
    refetchInterval: (query) => {
      if (!query.state.data?.items.some(checking)) {
        pendingSince.current = null;
        return false;
      }
      pendingSince.current ??= Date.now();
      const waited = Date.now() - pendingSince.current;
      if (waited > POLL_STOP_AFTER_MS) return false;
      return waited < POLL_FAST_FOR_MS ? POLL_FAST_MS : POLL_SLOW_MS;
    },
  });
  // What an environment's page just did, said here once it sent the administrator back.
  const [notice, setNotice] = useState<string | null>(
    () => cache.getQueryData<string>(NOTICE_KEY) ?? null,
  );
  useEffect(() => {
    cache.removeQueries({ queryKey: NOTICE_KEY, exact: true });
  }, [cache]);
  const [dialog, setDialog] = useState<Open>(null);
  const open = (next: Open) => {
    setNotice(null);
    setDialog(next);
  };
  const changed = (copy: string) => {
    setDialog(null);
    setNotice(copy);
    pendingSince.current = null;
    void cache.invalidateQueries({ queryKey: KEY });
  };
  if (list.isPending) return <Skeleton label={t("environments.loading")} />;
  if (list.isError)
    return (
      <LoadFailure
        message={t("environments.failed")}
        onRetry={() => void list.refetch()}
      />
    );
  const items = list.data.items;
  const stranded = items.find((item) => item.is_default && !offered(item));
  const names = new Map(items.map((item) => [item.name, item.display_name]));
  async function check(item: EnvironmentItem) {
    try {
      await api.check(item.name);
      changed(t("environments.checkAsked", { name: item.display_name }));
    } catch {
      setNotice(t("environments.checkFailed"));
    }
  }
  return (
    <div className="stack">
      <p className="type-small muted">{t("environments.intro")}</p>
      <div className="settings-controls">
        <Link
          href="/settings/environments/new"
          className="button"
          data-variant="primary"
        >
          {t("environments.add")}
        </Link>
      </div>
      {stranded && <DefaultNotice item={stranded} />}
      {notice && (
        <p className="type-small" role="status">
          {notice}
        </p>
      )}
      {items.length === 0 ? (
        <p className="muted">{t("environments.empty")}</p>
      ) : (
        <TableScroll label={t("environments.list")}>
          <table
            className="data-table environments-table"
            aria-label={t("environments.list")}
          >
            <thead className="type-label">
              <tr>
                <th scope="col">{t("environments.environment")}</th>
                <th scope="col">{t("environments.tools")}</th>
                <th scope="col">{t("environments.status")}</th>
                <th scope="col">
                  <span className="sr-only">{t("common.actions")}</span>
                </th>
              </tr>
            </thead>
            <tbody>
              {items.map((item) => (
                <EnvironmentRow
                  key={item.name}
                  item={item}
                  // A seeded base names itself (ADR-0032): it is built in, not derived.
                  base={
                    item.base && item.base !== item.name
                      ? (names.get(item.base) ?? item.base)
                      : null
                  }
                  onDelete={() => open({ remove: item })}
                  onReset={() => open({ reset: item })}
                  onDefault={() => open({ makeDefault: item })}
                  onCheck={() => void check(item)}
                />
              ))}
            </tbody>
          </table>
        </TableScroll>
      )}
      {dialog && "reset" in dialog && (
        <ResetEnvironment
          item={dialog.reset}
          api={api}
          onClose={() => setDialog(null)}
          onDone={() =>
            changed(
              t("environments.resetDone", { name: dialog.reset.display_name }),
            )
          }
        />
      )}
      {dialog && "remove" in dialog && (
        <DeleteEnvironment
          item={dialog.remove}
          api={api}
          onClose={() => setDialog(null)}
          onDeleted={() =>
            changed(
              t("environments.deleted", { name: dialog.remove.display_name }),
            )
          }
        />
      )}
      {dialog && "makeDefault" in dialog && (
        <MakeDefault
          item={dialog.makeDefault}
          api={api}
          onClose={() => setDialog(null)}
          onDone={() =>
            changed(
              t("environments.defaultDone", {
                name: dialog.makeDefault.display_name,
              }),
            )
          }
        />
      )}
    </div>
  );
}

const rowId = (name: string) => `environment-${name}`;

/**
 * The default that people cannot pick (ADR-0032): new tasks that don't choose an environment are refused, so the page
 * says so first, with the gateway's reason and a way to its row. A standing state, so not an alert.
 */
function DefaultNotice({ item }: { item: EnvironmentItem }) {
  const t = useT();
  return (
    <section
      className="settings-callout stack"
      data-tone="danger"
      aria-labelledby="environments-default-notice"
    >
      <h2 id="environments-default-notice" className="type-heading">
        {t("environments.defaultNoticeTitle")}
      </h2>
      <p>{t("environments.defaultNoticeBody", { name: item.display_name })}</p>
      <p className="type-small">{t(unavailableKey(unavailableReason(item)))}</p>
      <p className="type-small">
        <a
          href={`#${rowId(item.name)}`}
          onClick={(event) => {
            event.preventDefault();
            document.getElementById(rowId(item.name))?.focus();
          }}
        >
          {t("environments.defaultNoticeLink", { name: item.display_name })}
        </a>
      </p>
    </section>
  );
}

function EnvironmentRow({
  item,
  base,
  onDelete,
  onReset,
  onDefault,
  onCheck,
}: {
  item: EnvironmentItem;
  base: string | null;
  onDelete: () => void;
  onReset: () => void;
  onDefault: () => void;
  onCheck: () => void;
}) {
  const t = useT();
  const { language } = useLanguage();
  const name = item.display_name;
  /** A size the ceiling lowered, marked, with what it was when the gateway lowered it at start. */
  const lowered = (field: "cpus" | "memory_mib", text: string) => {
    if (!item.clamped.includes(field)) return text;
    const before = item.clamped_from?.[field];
    if (before === undefined)
      return t("environments.loweredShort", { value: text });
    return t("environments.loweredShortFrom", {
      value: text,
      from:
        field === "cpus"
          ? t("limits.cpus", { cpus: before })
          : formatMib(before, language),
    });
  };
  const origin = base
    ? t("environments.derivedFrom", { base })
    : t("environments.builtIn");
  // A built-in the gateway registered itself was found on this host, not added here (the 14b page pass, D04).
  const source = t(
    item.seed === null && item.last_set_by === "builtin"
      ? "environments.foundOnHost"
      : (SOURCES[item.seed ?? "none"] ?? "environments.addedHere"),
  );
  const known = item.capabilities.filter(
    (value): value is Capability => value in capabilities,
  );
  // Why it can't be deleted now; the task count matters only here, so the row no longer shows it.
  const blocked = item.is_default
    ? t("environments.deleteDefault")
    : item.bound_conversations > 0
      ? t("environments.deleteBound", { count: item.bound_conversations })
      : undefined;
  return (
    <tr id={rowId(item.name)} tabIndex={-1}>
      <td>
        {/* The display name only: the internal name is not the reader's (as on Connectors and Agent profiles). */}
        <div className="stack route-name">
          <span>{name}</span>
          {item.description && (
            <span className="type-small">{item.description}</span>
          )}
          {/* Its own size, when it has one (the owner's direction: sizes belong to each environment). */}
          {(item.cpus !== null || item.memory_mib !== null) && (
            <span className="type-small muted">
              {[
                item.cpus === null
                  ? null
                  : lowered("cpus", t("limits.cpus", { cpus: item.cpus })),
                item.memory_mib === null
                  ? null
                  : lowered("memory_mib", formatMib(item.memory_mib, language)),
              ]
                .filter(Boolean)
                .join(" · ")}
            </span>
          )}
          <span className="type-small muted">{`${origin} · ${source}`}</span>
          {/* WEB-U-098: an edited built-in is the administrator's now; Reset in the menu hands it back. */}
          {item.customised && (
            <span className="type-small">{t("environments.customised")}</span>
          )}
        </div>
      </td>
      <td>
        <ToolList values={known} />
      </td>
      <td>
        <EnvironmentStatus item={item} />
      </td>
      {/* One actions column (UI-GUIDELINES §7, as on Models and Keys): Edit, and the rest in the row's menu. */}
      <td className="data-cell-action">
        <Link
          href={`/settings/environments/${encodeURIComponent(item.name)}`}
          className="button"
          data-variant="ghost"
          aria-label={t("environments.editName", { name })}
        >
          {t("environments.edit")}
        </Link>
        <RowMenu
          label={t("environments.more", { name })}
          actions={[
            {
              id: "check",
              label: t("environments.check"),
              name: t("environments.checkName", { name }),
              onSelect: onCheck,
            },
            ...(item.customised
              ? [
                  {
                    id: "reset",
                    label: t("environments.reset"),
                    name: t("environments.resetName", { name }),
                    onSelect: onReset,
                  },
                ]
              : []),
            ...(!item.is_default && offered(item)
              ? [
                  {
                    id: "default",
                    label: t("environments.makeDefault"),
                    name: t("environments.makeDefaultName", { name }),
                    onSelect: onDefault,
                  },
                ]
              : []),
            {
              id: "delete",
              label: t("environments.delete"),
              name: t("environments.deleteName", { name }),
              danger: true,
              unavailable: blocked,
              onSelect: onDelete,
            },
          ]}
        />
      </td>
    </tr>
  );
}

/** The tools an environment declares, each by name beside its icon: never an unlabelled icon (WEB-U-047). */
function ToolList({ values }: { values: readonly Capability[] }) {
  const t = useT();
  if (!values.length)
    return <span className="type-small muted">{t("capability.none")}</span>;
  return (
    <ul
      className="environment-tools type-small"
      aria-label={t("environments.tools")}
    >
      {values.map((value) => (
        <li key={value}>
          <Icon glyph={capabilities[value].icon} size={16} />
          {t(capabilities[value].label)}
        </li>
      ))}
    </ul>
  );
}

/** The pills, the check's sentence and the two status actions: Check, and Make default on a pickable environment. */
function EnvironmentStatus({ item }: { item: EnvironmentItem }) {
  const t = useT();
  const state = checkState(item);
  const [label, tone] = STATES[state];
  return (
    <div className="stack route-name">
      <div className="environment-pills">
        {item.is_default && (
          <span className="status-pill type-label" data-tone="ok">
            {t("environments.default")}
          </span>
        )}
        {!item.enabled && (
          <span className="status-pill type-label" data-tone="muted">
            {t("environments.off")}
          </span>
        )}
        <span className="status-pill type-label" data-tone={tone}>
          {t(label)}
        </span>
      </div>
      {/* A scan limit with its bound says it more precisely below; otherwise the reason's own sentence. */}
      {state === "refused" && item.check.reason === "rootfs_missing" && (
        <EnvironmentRootfsMissing item={item} />
      )}
      {state === "refused" &&
        item.check.reason !== "rootfs_missing" &&
        !(item.check.reason === "image_scan_limit" && hasScanLimit(item)) && (
          <p className="type-small">{t(reasonKey(item.check.reason))}</p>
        )}
      {state !== "checking" && state !== "unchecked" && (
        <SecretFiles item={item} />
      )}
      {state === "checking" && (
        <p className="type-small muted">{t("environments.checkingLine")}</p>
      )}
      {state === "unchecked" && (
        <p className="type-small muted">{t("environments.uncheckedLine")}</p>
      )}
      {item.enabled && !offered(item) && (
        <p className="type-small muted">{t("environments.notPickable")}</p>
      )}
    </div>
  );
}

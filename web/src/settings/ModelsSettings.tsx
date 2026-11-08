"use client";

import { useQuery, useQueryClient } from "@tanstack/react-query";
import Link from "next/link";
import { useRouter } from "next/navigation";
import { useEffect, useState } from "react";

import { newActionKey } from "../api/action-key";
import { ApiError } from "../api/error";
import { useT } from "../i18n/I18nProvider";
import type { MessageKey } from "../i18n/translate";
import type { User } from "../protocol/generated/api";
import { Button } from "../ui/Button";
import { CellNote } from "../ui/CellNote";
import { ConfirmDialog } from "../ui/ConfirmDialog";
import { Skeleton } from "../ui/Skeleton";
import { TableScroll } from "../ui/TableScroll";

import type { RouteItem, RoutesApi } from "./admin/routes";
import { LoadFailure } from "./LoadFailure";
import { routeLabel } from "./route-label";
import { localRoute } from "./route-local";
import { RouteCheck } from "./RouteCheck";
import type { Preset } from "./RouteForm";
import {
  ROUTE_CHECK_NEXT_KEY,
  ROUTE_NOTICE_KEY,
  ROUTE_OPEN_KEY,
  ROUTES_KEY,
  type RouteOpening,
} from "./RoutePage";
import { RowMenu } from "./RowMenu";
import { keyName } from "./secret-copy";

const KEY = ROUTES_KEY;
const LIST = "/settings/models";
/** The open dialog: check or delete one route. Add and Edit open the route's own page (WEB-U-115). */
type Open = { remove: RouteItem } | { check: RouteItem } | null;
const PROTOCOLS: Readonly<Record<RouteItem["protocol"], MessageKey>> = {
  openai_chat: "models.protocolChat",
  openai_responses: "models.protocolResponses",
  anthropic_messages: "models.protocolMessages",
};
/** A route probe's result as a badge; a route never checked shows none. */
const PROBES: Readonly<
  Record<RouteItem["probe"]["state"], [MessageKey, string] | null>
> = {
  verified: ["models.probeVerified", "ok"],
  failed: ["models.probeFailed", "danger"],
  stale: ["models.probeStale", "warn"],
  never: null,
};
const SOURCES: Readonly<Record<string, MessageKey>> = {
  current: "models.fromSeed",
  removed: "models.seedRemoved",
  none: "models.addedHere",
};

/** Model routes for administrators (FR-4, FR-22, ADR-0028): list, add, edit, delete and export. */
export function ModelsSettings({ me, api }: { me: User; api: RoutesApi }) {
  const t = useT();
  if (me.role !== "admin")
    return <p className="muted">{t("models.adminsOnly")}</p>;
  return <Models api={api} />;
}

function Models({ api }: { api: RoutesApi }) {
  const t = useT();
  const cache = useQueryClient();
  const list = useQuery({
    queryKey: KEY,
    queryFn: ({ signal }) => api.routes(signal),
    retry: false,
  });
  const router = useRouter();
  // What a route's page just did, said here once it sent the administrator back.
  const [notice, setNotice] = useState<string | null>(
    () => cache.getQueryData<string>(ROUTE_NOTICE_KEY) ?? null,
  );
  // A route just added, which the notice offers to check.
  const [checkNext, setCheckNext] = useState<string | null>(
    () => cache.getQueryData<string>(ROUTE_CHECK_NEXT_KEY) ?? null,
  );
  useEffect(() => {
    cache.removeQueries({ queryKey: ROUTE_NOTICE_KEY, exact: true });
    cache.removeQueries({ queryKey: ROUTE_CHECK_NEXT_KEY, exact: true });
  }, [cache]);
  const [dialog, setDialog] = useState<Open>(null);
  /** Opens a route's page, asked to open ready for something (Paste key, a check's suggestion). */
  const openPage = (opening: RouteOpening) => {
    cache.setQueryData(ROUTE_OPEN_KEY, opening);
    router.push(`${LIST}/${encodeURIComponent(opening.name)}`);
  };
  const open = (next: Open) => {
    setNotice(null);
    setCheckNext(null);
    setDialog(next);
  };
  if (list.isPending) return <Skeleton label={t("models.loading")} />;
  if (list.isError)
    return (
      <LoadFailure
        message={t("models.failed")}
        onRetry={() => void list.refetch()}
      />
    );
  const items = list.data.items;
  const added = checkNext
    ? items.find((item) => item.name === checkNext)
    : undefined;
  return (
    <div className="stack">
      <p className="type-small muted">{t("models.intro")}</p>
      <div className="settings-controls">
        <Link href={`${LIST}/new`} className="button" data-variant="primary">
          {t("models.add")}
        </Link>
        <a
          className="button"
          data-variant="secondary"
          href={api.exportUrl()}
          download
          aria-describedby="models-export-hint"
        >
          {t("models.export")}
        </a>
      </div>
      {/* What the download is for, so it isn't taken for a step (the Models page review, P3). */}
      <p id="models-export-hint" className="type-small muted">
        {t("models.exportHint")}
      </p>
      {notice && (
        <div className="stack">
          <p className="type-small" role="status">
            {notice}
          </p>
          {added && (
            <div>
              <Button
                variant="secondary"
                onClick={() => open({ check: added })}
              >
                {t("models.checkNow", { name: routeLabel(added) })}
              </Button>
            </div>
          )}
        </div>
      )}
      <RouteTable
        items={items}
        onPaste={(item) => openPage({ name: item.name, paste: true })}
        onCheck={(item) => open({ check: item })}
        onDelete={(item) => open({ remove: item })}
      />
      <RouteDialogs
        api={api}
        dialog={dialog}
        onClose={() => setDialog(null)}
        onSuggest={(item, preset) => openPage({ name: item.name, preset })}
        onChecked={() => void cache.invalidateQueries({ queryKey: KEY })}
        onChanged={(copy) => {
          setDialog(null);
          setNotice(copy);
          void cache.invalidateQueries({ queryKey: KEY });
        }}
      />
    </div>
  );
}

/** The open add, edit or delete dialog; each reports its outcome as the notice to show. */
function RouteDialogs({
  api,
  dialog,
  onClose,
  onSuggest,
  onChecked,
  onChanged,
}: {
  api: RoutesApi;
  dialog: Open;
  onClose: () => void;
  onSuggest: (item: RouteItem, preset: Preset) => void;
  /** A route's check finished: its listed state changed. */
  onChecked: () => void;
  onChanged: (copy: string) => void;
}) {
  const t = useT();
  return (
    <>
      {dialog && "check" in dialog && (
        <RouteCheck
          name={dialog.check.name}
          label={routeLabel(dialog.check)}
          local={localRoute(dialog.check)}
          maxOutput={dialog.check.max_output}
          contextWindow={dialog.check.context_window}
          address={dialog.check.base_url}
          protocol={dialog.check.protocol}
          api={api}
          onClose={onClose}
          onChecked={onChecked}
          onSuggest={(preset) => onSuggest(dialog.check, preset)}
        />
      )}
      {dialog && "remove" in dialog && (
        <DeleteRoute
          item={dialog.remove}
          api={api}
          onClose={onClose}
          onDeleted={() =>
            onChanged(t("models.deleted", { name: routeLabel(dialog.remove) }))
          }
        />
      )}
    </>
  );
}
/** A route as the page names it, to the eye and to a screen reader alike: its display name, or its name (§10). */
export { routeLabel } from "./route-label";

type RowActions = {
  /** Opens the route's form with Paste key ready: its pasted key was scrubbed at upgrade (key_ref_invalid). */
  onPaste: (item: RouteItem) => void;
  onCheck: (item: RouteItem) => void;
  onDelete: (item: RouteItem) => void;
};

function RouteTable({
  items,
  ...actions
}: { items: RouteItem[] } & RowActions) {
  const t = useT();
  if (items.length === 0) return <p className="muted">{t("models.empty")}</p>;
  // On a page-made installation every row would say "Added here" and "Chat Completions", so Source shows once any route
  // comes from the configuration file (the list is the only place that says so, "No longer in the configuration file"
  // included), and the protocol once the routes differ in it; the route's form still names its protocol (WEB-U-301).
  const shown: Shown = {
    source: items.some((item) => item.seed),
    protocol: new Set(items.map((item) => item.protocol)).size > 1,
  };
  return (
    <TableScroll label={t("models.list")}>
      <table className="data-table models-table" aria-label={t("models.list")}>
        <thead className="type-label">
          <tr>
            <th scope="col">{t("models.route")}</th>
            <th scope="col">{t("models.model")}</th>
            <th scope="col">{t("models.key")}</th>
            <th scope="col">
              <span className="sr-only">{t("common.actions")}</span>
            </th>
          </tr>
        </thead>
        <tbody>
          {items.map((item) => (
            <RouteRow
              key={item.name}
              item={item}
              shown={shown}
              // The routes that fall back to it, by what the page calls them.
              fallbackOf={item.fallback_of.map((name) => {
                const other = items.find((route) => route.name === name);
                return other ? routeLabel(other) : name;
              })}
              {...actions}
            />
          ))}
        </tbody>
      </table>
    </TableScroll>
  );
}

/** The list's columns and lines shown only when the routes differ in them. */
type Shown = { source: boolean; protocol: boolean };

function RouteRow({
  item,
  shown,
  fallbackOf,
  onCheck,
  onDelete,
  onPaste,
}: {
  item: RouteItem;
  shown: Shown;
  fallbackOf: readonly string[];
} & RowActions) {
  const t = useT();
  const blocked = item.fallback_of.length > 0;
  return (
    <tr>
      <td>
        {/* The display name only (UI-GUIDELINES §10); the route's own name stands in when there is none. */}
        <div className="stack route-name">
          <div className="data-name" title={routeLabel(item)}>
            <span>{routeLabel(item)}</span>
          </div>
          {/* One muted line (17.3): what the route is for, then where it came from, as Connectors and Environments
              say theirs; the source only where some route comes from the configuration file (WEB-U-301). */}
          {(item.description || shown.source) && (
            <CellNote>
              {[
                item.description,
                shown.source
                  ? t(SOURCES[item.seed ?? "none"] ?? "models.addedHere")
                  : null,
              ]
                .filter(Boolean)
                .join(" · ")}
            </CellNote>
          )}
        </div>
      </td>
      <td>
        <div className="stack route-name">
          <div className="data-name" title={item.model}>
            <span>{item.model}</span>
            <ProbeBadge probe={item.probe} />
          </div>
          {shown.protocol && <CellNote>{t(PROTOCOLS[item.protocol])}</CellNote>}
        </div>
      </td>
      <td className="secret-changed">
        {item.auth.kind === "none" ? (
          <span className="muted">{t("models.noKey")}</span>
        ) : item.key_ref_invalid ? (
          <span className="status-pill type-label" data-tone="warn">
            {t("models.keyNeeded")}
          </span>
        ) : item.key_missing ? (
          <span className="stack route-name">
            <span className="status-pill type-label" data-tone="warn">
              {t("models.keyMissing")}
            </span>
            <CellNote href="/settings/keys/shared">
              {t("models.addKey")}
            </CellNote>
          </span>
        ) : (
          // The key's name, as Keys › Shared shows it: one line, never broken inside it; a long one shortens, and the full
          // reference is on hover (F8; WEB-U-181: `secrets/` is the configuration's notation, not part of the name).
          <code
            className="type-code secret-ref-line"
            title={item.key_ref ?? undefined}
          >
            {item.key_ref ? keyName(item.key_ref) : null}
          </code>
        )}
      </td>
      <td className="data-cell-action">
        <Link
          href={`${LIST}/${encodeURIComponent(item.name)}`}
          className="button"
          data-variant="ghost"
          aria-label={t("models.editName", { name: routeLabel(item) })}
        >
          {t("models.edit")}
        </Link>
        <RowMenu
          label={t("models.more", { name: routeLabel(item) })}
          actions={[
            {
              id: "check",
              label: t("models.check"),
              name: t("models.checkName", { name: routeLabel(item) }),
              onSelect: () => onCheck(item),
            },
            ...(item.key_ref_invalid
              ? [
                  {
                    id: "paste",
                    label: t("models.pasteKey"),
                    name: t("models.pasteKeyFor", { name: routeLabel(item) }),
                    onSelect: () => onPaste(item),
                  },
                ]
              : []),
            {
              id: "delete",
              label: t("models.delete"),
              name: t("models.deleteName", { name: routeLabel(item) }),
              danger: true,
              unavailable: blocked
                ? t("models.deleteBlocked", {
                    names: fallbackOf.join(", "),
                  })
                : undefined,
              onSelect: () => onDelete(item),
            },
          ]}
        />
      </td>
    </tr>
  );
}

function DeleteRoute({
  item,
  api,
  onDeleted,
  onClose,
}: {
  item: RouteItem;
  api: RoutesApi;
  onDeleted: () => void;
  onClose: () => void;
}) {
  const t = useT();
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<Error | null>(null);
  async function remove() {
    setPending(true);
    try {
      await api.deleteRoute(item.name, item.version, newActionKey());
      onDeleted();
    } catch (failure) {
      setError(failure instanceof Error ? failure : new Error("delete failed"));
    } finally {
      setPending(false);
    }
  }
  const copy = (refusal: Error): MessageKey =>
    refusal instanceof ApiError && refusal.code === "route_is_fallback"
      ? "models.isFallback"
      : refusal instanceof ApiError && refusal.code === "conflict"
        ? "models.conflictDelete"
        : "models.saveFailed";
  return (
    <ConfirmDialog
      title={t("models.deleteTitle", { name: routeLabel(item) })}
      confirm={t("models.deleteConfirm")}
      pending={pending}
      error={error}
      errorCopy={(refusal) => t(copy(refusal))}
      onConfirm={() => void remove()}
      onClose={onClose}
    >
      <p>{t("models.deleteBody")}</p>
      {/* A route from the configuration file stays deleted (admin guide, Files and the settings pages). */}
      {item.seed === "current" && <p>{t("models.deleteSeeded")}</p>}
    </ConfirmDialog>
  );
}

function ProbeBadge({ probe }: { probe: RouteItem["probe"] }) {
  const t = useT();
  const badge = PROBES[probe.state];
  if (!badge) return null;
  return (
    <span className="status-pill type-label" data-tone={badge[1]}>
      {t(badge[0])}
    </span>
  );
}

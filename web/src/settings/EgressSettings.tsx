"use client";

import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Trash2 } from "lucide-react";
import { usePathname, useRouter, useSearchParams } from "next/navigation";
import { useEffect, useState } from "react";

import { newActionKey } from "../api/action-key";
import { useT } from "../i18n/I18nProvider";
import type { MessageKey } from "../i18n/translate";
import type { User } from "../protocol/generated/api";
import { ActionMenu } from "../ui/ActionMenu";
import { Button } from "../ui/Button";
import { CellNote } from "../ui/CellNote";
import { ConfirmDialog } from "../ui/ConfirmDialog";
import { Skeleton } from "../ui/Skeleton";
import { TableScroll } from "../ui/TableScroll";

import type { EgressApi, EgressRule } from "./admin/egress";
import { ruleFailure } from "./egress-copy";
import { EgressPresets, PRESET_COPY, PRESETS_KEY } from "./EgressPresets";
import { canonicalIpv6 } from "./host-keys/address";
import { LoadFailure } from "./LoadFailure";
import { RuleForm } from "./RuleForm";

const KEY = ["admin-egress"];
const INTERNAL_STATES: Readonly<
  Record<NonNullable<EgressRule["internal_state"]>, MessageKey>
> = {
  internal_never_missing: "egress.internalNeverMissing",
  engine_unknown: "egress.engineUnknown",
};
const SOURCES: Readonly<Record<string, MessageKey>> = {
  current: "egress.fromSeed",
  removed: "egress.seedRemoved",
  none: "egress.addedHere",
};
/** The open dialog: add (edit: null, maybe with a host to start from), edit or delete one rule. */
type Open =
  | { edit: EgressRule | null; start?: { host: string; ports: string } }
  | { remove: EgressRule }
  | null;

/** A host as a key's address names it: lower case, labels of letters, digits and hyphens. */
const HOST =
  /^(?=.{1,253}$)[a-z0-9](?:[a-z0-9-]*[a-z0-9])?(?:\.[a-z0-9](?:[a-z0-9-]*[a-z0-9])?)*$/;

/**
 * The host (and port) this page was opened for, from a key's "Allow {host}" link (WEB-U-150), read from the router's
 * address: on a client navigation window.location is still the page before (Next updates the history after the render).
 * Null when there is none or it isn't a host or an address.
 */
function readAsked(
  params: URLSearchParams,
): { host: string; port: number } | null {
  if (!params.has("host")) return null;
  const host = params.get("host")?.trim().toLowerCase() ?? "";
  const portText = params.get("port");
  const port = portText === null ? 443 : Number(portText);
  // A name, or an exact IPv6 address in the one spelling a key's link writes (ADR-0037 Amendment 2).
  const known = HOST.test(host) || canonicalIpv6(host) === host;
  if (!known || !Number.isInteger(port) || port < 1 || port > 65535)
    return null;
  return { host, port };
}

/** The egress allow-list for administrators (ADR-0028, decision B): an empty list lets sandboxes reach nothing. */
export function EgressSettings({ me, api }: { me: User; api: EgressApi }) {
  const t = useT();
  if (me.role !== "admin")
    return <p className="muted">{t("egress.adminsOnly")}</p>;
  return <Egress api={api} />;
}

function Egress({ api }: { api: EgressApi }) {
  const t = useT();
  const cache = useQueryClient();
  const list = useQuery({
    queryKey: KEY,
    queryFn: ({ signal }) => api.rules(signal),
    retry: false,
  });
  const [notice, setNotice] = useState<string | null>(null);
  const [dialog, setDialog] = useState<Open>(null);
  const open = (next: Open) => {
    setNotice(null);
    setAskDone(true);
    setDialog(next);
  };
  // Opened for a host (a key's "Allow {host}"): its own rule to edit, or Add a site with it filled in, once the list
  // is here and never while egress is turned off; until the person closes it or opens something else.
  const params = useSearchParams();
  const router = useRouter();
  const pathname = usePathname();
  // Read once: dropping the parameters from the address below must not close what they opened.
  const [asked] = useState(() => readAsked(params));
  const [askDone, setAskDone] = useState(false);
  // Off the address once read, so a reload or Back doesn't ask again.
  const carries = params.has("host") || params.has("port");
  useEffect(() => {
    if (carries) router.replace(pathname);
  }, [carries, router, pathname]);
  const loaded = list.data;
  const existing = asked
    ? loaded?.items.find((item) => item.host === asked.host)
    : undefined;
  // A rule that already takes the port needs nothing; one without it opens with the port added, a change to save.
  const covered = existing?.ports.includes(asked?.port ?? 0) ?? false;
  const askedDialog: Open =
    asked && loaded && !loaded.disabled && !askDone && !covered
      ? existing
        ? {
            edit: existing,
            start: {
              host: existing.host,
              ports: [...existing.ports, asked.port]
                .sort((a, b) => a - b)
                .join(", "),
            },
          }
        : { edit: null, start: { host: asked.host, ports: String(asked.port) } }
      : null;
  const shown = dialog ?? askedDialog;
  if (list.isPending) return <Skeleton label={t("egress.loading")} />;
  if (list.isError)
    return (
      <LoadFailure
        message={t("egress.failed")}
        onRetry={() => void list.refetch()}
      />
    );
  const { items, disabled, limit } = list.data;
  const changed = (copy: string) => {
    setDialog(null);
    setAskDone(true);
    setNotice(copy);
    void cache.invalidateQueries({ queryKey: KEY });
  };
  return (
    <div className="stack">
      <p className="type-small muted">{t("egress.intro")}</p>
      {/* Any other site is added from the Custom card, the one way to add (WEB-U-118). */}
      {disabled && (
        <p className="type-small" role="note">
          {t("egress.disabled")}
        </p>
      )}
      {notice && (
        <p className="type-small" role="status">
          {notice}
        </p>
      )}
      {!disabled && (
        <EgressPresets
          api={api}
          onCustom={() => open({ edit: null })}
          onChanged={(copy) => {
            setNotice(copy);
            void cache.invalidateQueries({ queryKey: KEY });
            void cache.invalidateQueries({ queryKey: PRESETS_KEY });
          }}
        />
      )}
      <RuleTable
        items={items}
        editable={!disabled}
        onEdit={(item) => open({ edit: item })}
        onDelete={(item) => open({ remove: item })}
      />
      <RuleDialogs
        dialog={shown}
        api={api}
        limit={limit}
        onClose={() => {
          setAskDone(true);
          setDialog(null);
        }}
        onStale={async (host) =>
          (await list.refetch()).data?.items.find((item) => item.host === host)
        }
        onChanged={changed}
      />
    </div>
  );
}

/** The open add, edit or delete dialog; each reports its outcome as the notice to show. */
function RuleDialogs({
  dialog,
  api,
  limit,
  onClose,
  onStale,
  onChanged,
}: {
  dialog: Open;
  api: EgressApi;
  limit: number;
  onClose: () => void;
  onStale: (host: string) => Promise<EgressRule | undefined>;
  onChanged: (copy: string) => void;
}) {
  const t = useT();
  if (!dialog) return null;
  return "edit" in dialog ? (
    <RuleForm
      item={dialog.edit}
      start={dialog.start}
      api={api}
      limit={limit}
      onClose={onClose}
      onStale={onStale}
      onSaved={(item) => onChanged(t("egress.saved", { host: item.host }))}
    />
  ) : (
    <DeleteRule
      item={dialog.remove}
      api={api}
      limit={limit}
      onClose={onClose}
      onDeleted={() =>
        onChanged(t("egress.deleted", { host: dialog.remove.host }))
      }
    />
  );
}

function RuleTable({
  items,
  editable,
  onEdit,
  onDelete,
}: {
  items: EgressRule[];
  editable: boolean;
  onEdit: (item: EgressRule) => void;
  onDelete: (item: EgressRule) => void;
}) {
  const t = useT();
  // The rules under a heading of their own, below the ready-made lists (the 13f screens, #10, WEB-U-162). Not a
  // landmark: the table's scroll region already is one, by the same name.
  return (
    <div className="stack">
      <h2 className="type-heading">{t("egress.list")}</h2>
      {items.length === 0 ? (
        <p className="muted">{t("egress.empty")}</p>
      ) : (
        <RuleRows
          items={items}
          editable={editable}
          onEdit={onEdit}
          onDelete={onDelete}
        />
      )}
    </div>
  );
}

function RuleRows({
  items,
  editable,
  onEdit,
  onDelete,
}: {
  items: EgressRule[];
  editable: boolean;
  onEdit: (item: EgressRule) => void;
  onDelete: (item: EgressRule) => void;
}) {
  const t = useT();
  return (
    <TableScroll label={t("egress.list")}>
      <table className="data-table" aria-label={t("egress.list")}>
        <thead className="type-label">
          <tr>
            <th scope="col">{t("egress.host")}</th>
            <th scope="col">{t("egress.ports")}</th>
            <th scope="col">{t("egress.source")}</th>
            <th scope="col">
              <span className="sr-only">{t("common.actions")}</span>
            </th>
          </tr>
        </thead>
        <tbody>
          {items.map((item) => (
            <tr key={item.host}>
              <td>
                <span className="stack route-name">
                  {/* An internal server's rule (WEB-U-188) says so beside its host, and why the gateway can't use it
                      yet in the row's one muted line (17.3). */}
                  <span className="cell-pills">
                    <code className="type-code">{item.host}</code>{" "}
                    {item.internal && (
                      <span className="status-pill type-label">
                        {t("egress.internal")}
                      </span>
                    )}
                  </span>
                  {item.internal_state && (
                    <CellNote>
                      {t(INTERNAL_STATES[item.internal_state])}
                    </CellNote>
                  )}
                </span>
              </td>
              <td>{item.ports.join(", ")}</td>
              <td className="muted">
                {/* A rule a ready-made list added names the list (WEB-U-118). */}
                {item.preset
                  ? t(PRESET_COPY[item.preset].name)
                  : t(SOURCES[item.seed ?? "none"] ?? "egress.addedHere")}
              </td>
              <td className="data-cell-action">
                {editable && (
                  <>
                    <Button
                      variant="ghost"
                      aria-label={t("egress.editName", { host: item.host })}
                      onClick={() => onEdit(item)}
                    >
                      {t("egress.edit")}
                    </Button>
                    {/* Delete under the row's ⋯, as a danger item, as in every settings table (WEB-U-162). */}
                    <ActionMenu
                      label={t("egress.moreName", { host: item.host })}
                      actions={[
                        {
                          id: "delete",
                          label: t("egress.deleteMenu"),
                          name: t("egress.deleteName", { host: item.host }),
                          icon: Trash2,
                          danger: true,
                          onSelect: () => onDelete(item),
                        },
                      ]}
                    />
                  </>
                )}
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </TableScroll>
  );
}

/** Asks once, naming the host, then deletes the rule with the version the page read. */
function DeleteRule({
  item,
  api,
  limit,
  onDeleted,
  onClose,
}: {
  item: EgressRule;
  api: EgressApi;
  limit: number;
  onDeleted: () => void;
  onClose: () => void;
}) {
  const t = useT();
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<Error | null>(null);
  async function remove() {
    setPending(true);
    setError(null);
    try {
      await api.deleteRule(item.host, item.version, newActionKey());
      onDeleted();
    } catch (failure) {
      setError(failure instanceof Error ? failure : new Error("delete failed"));
    } finally {
      setPending(false);
    }
  }
  return (
    <ConfirmDialog
      title={t("egress.deleteTitle", { host: item.host })}
      confirm={t("egress.deleteConfirm")}
      pending={pending}
      error={error}
      errorCopy={(refusal) => {
        const copy = ruleFailure(refusal);
        return copy ? t(copy, { limit }) : null;
      }}
      onConfirm={() => void remove()}
      onClose={onClose}
    >
      <p>{t("egress.deleteBody")}</p>
    </ConfirmDialog>
  );
}

"use client";

import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";

import { newActionKey } from "../api/action-key";
import { useT } from "../i18n/I18nProvider";
import type { User } from "../protocol/generated/api";
import { Button } from "../ui/Button";
import { CellNote } from "../ui/CellNote";
import { ConfirmDialog } from "../ui/ConfirmDialog";
import { Skeleton } from "../ui/Skeleton";
import { TableScroll } from "../ui/TableScroll";

import type { Connector, ConnectorsApi } from "./admin/connectors";
import {
  connectorFailure,
  hostOf,
  SOURCES,
  toolsSummary,
} from "./connector-copy";
import { ConnectorForm } from "./ConnectorForm";
import { LoadFailure } from "./LoadFailure";
import { RowMenu } from "./RowMenu";

const KEY = ["admin-connectors"];
/** The open dialog: add (edit: null), edit or delete one connector. */
type Open = { edit: Connector | null } | { remove: Connector } | null;

/** The connector registry for administrators (ADR-0031): remote tool servers and how the gateway reaches them. */
export function ConnectorsSettings({
  me,
  api,
}: {
  me: User;
  api: ConnectorsApi;
}) {
  const t = useT();
  if (me.role !== "admin")
    return <p className="muted">{t("connectors.adminsOnly")}</p>;
  return <Connectors api={api} />;
}

function Connectors({ api }: { api: ConnectorsApi }) {
  const t = useT();
  const cache = useQueryClient();
  const list = useQuery({
    queryKey: KEY,
    queryFn: ({ signal }) => api.connectors(signal),
    retry: false,
  });
  const [notice, setNotice] = useState<string | null>(null);
  const [dialog, setDialog] = useState<Open>(null);
  const open = (next: Open) => {
    setNotice(null);
    setDialog(next);
  };
  if (list.isPending) return <Skeleton label={t("connectors.loading")} />;
  if (list.isError)
    return (
      <LoadFailure
        message={t("connectors.failed")}
        onRetry={() => void list.refetch()}
      />
    );
  const changed = (copy: string) => {
    setDialog(null);
    setNotice(copy);
    void cache.invalidateQueries({ queryKey: KEY });
  };
  return (
    <div className="stack">
      <p className="type-small muted">{t("connectors.intro")}</p>
      <div className="settings-controls">
        <Button onClick={() => open({ edit: null })}>
          {t("connectors.add")}
        </Button>
      </div>
      {notice && (
        <p className="type-small" role="status">
          {notice}
        </p>
      )}
      <ConnectorTable
        items={list.data.items}
        onEdit={(item) => open({ edit: item })}
        onDelete={(item) => open({ remove: item })}
      />
      {dialog && "edit" in dialog && (
        <ConnectorForm
          item={dialog.edit}
          api={api}
          onClose={() => setDialog(null)}
          onStale={async (name) =>
            (await list.refetch()).data?.items.find(
              (item) => item.name === name,
            )
          }
          onSaved={(item) =>
            changed(t("connectors.saved", { name: label(item) }))
          }
        />
      )}
      {dialog && "remove" in dialog && (
        <DeleteConnector
          item={dialog.remove}
          api={api}
          onClose={() => setDialog(null)}
          onDeleted={() =>
            changed(t("connectors.deleted", { name: label(dialog.remove) }))
          }
        />
      )}
    </div>
  );
}

/** A connector as the page names it: its display name, or its name when it has none. */
const label = (item: Connector) => item.display_name || item.name;

function ConnectorTable({
  items,
  onEdit,
  onDelete,
}: {
  items: Connector[];
  onEdit: (item: Connector) => void;
  onDelete: (item: Connector) => void;
}) {
  const t = useT();
  if (items.length === 0)
    return <p className="muted">{t("connectors.empty")}</p>;
  return (
    <TableScroll label={t("connectors.list")}>
      <table className="data-table" aria-label={t("connectors.list")}>
        <thead className="type-label">
          <tr>
            <th scope="col">{t("connectors.connector")}</th>
            <th scope="col">{t("connectors.tools")}</th>
            <th scope="col">{t("connectors.status")}</th>
            <th scope="col">
              <span className="sr-only">{t("common.actions")}</span>
            </th>
          </tr>
        </thead>
        <tbody>
          {items.map((item) => (
            <ConnectorRow
              key={item.name}
              item={item}
              onEdit={onEdit}
              onDelete={onDelete}
            />
          ))}
        </tbody>
      </table>
    </TableScroll>
  );
}

function ConnectorRow({
  item,
  onEdit,
  onDelete,
}: {
  item: Connector;
  onEdit: (item: Connector) => void;
  onDelete: (item: Connector) => void;
}) {
  const t = useT();
  const [tools, values] = toolsSummary(item.tools);
  const source = t(SOURCES[item.seed ?? "none"] ?? "connectors.addedHere");
  const name = label(item);
  return (
    <tr>
      <td>
        <div className="stack route-name">
          {/* The display name only (UI-GUIDELINES §10); the internal name stands in when there is none. */}
          <span className="cell-whole">{name}</span>
          <CellNote>{`${hostOf(item.url)} · ${source}`}</CellNote>
        </div>
      </td>
      <td>{t(tools, values)}</td>
      <td>
        <div className="stack route-name">
          <span className="cell-pills">
            {!item.enabled && (
              <span className="status-pill type-label" data-tone="muted">
                {t("connectors.off")}
              </span>
            )}
            {/* A setting, not a check: muted (17.3). */}
            {item.enabled && item.enabled_by_default && (
              <span className="status-pill type-label" data-tone="muted">
                {t("connectors.inNewTasks")}
              </span>
            )}
            {item.key_missing && (
              <span className="status-pill type-label" data-tone="warn">
                {t("connectors.keyMissing")}
              </span>
            )}
          </span>
          {item.key_missing && (
            <CellNote href="/settings/keys/shared">
              {t("connectors.addKey")}
            </CellNote>
          )}
        </div>
      </td>
      {/* One actions column (UI-GUIDELINES §7, as on Models and Keys): Edit, and Delete in the row's menu. */}
      <td className="data-cell-action">
        <Button
          variant="ghost"
          aria-label={t("connectors.editName", { name })}
          onClick={() => onEdit(item)}
        >
          {t("connectors.edit")}
        </Button>
        <RowMenu
          label={t("connectors.more", { name })}
          actions={[
            {
              id: "delete",
              label: t("connectors.delete"),
              name: t("connectors.deleteName", { name }),
              danger: true,
              onSelect: () => onDelete(item),
            },
          ]}
        />
      </td>
    </tr>
  );
}

/** Asks once, naming the connector, then deletes it with the version the page read. */
function DeleteConnector({
  item,
  api,
  onDeleted,
  onClose,
}: {
  item: Connector;
  api: ConnectorsApi;
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
      await api.deleteConnector(item.name, item.version, newActionKey());
      onDeleted();
    } catch (failure) {
      setError(failure instanceof Error ? failure : new Error("delete failed"));
    } finally {
      setPending(false);
    }
  }
  return (
    <ConfirmDialog
      title={t("connectors.deleteTitle", { name: label(item) })}
      confirm={t("connectors.deleteConfirm")}
      pending={pending}
      error={error}
      errorCopy={(refusal) => {
        const copy = connectorFailure(refusal);
        return copy === "connectors.conflict"
          ? t("connectors.conflictDelete")
          : copy
            ? t(copy)
            : null;
      }}
      onConfirm={() => void remove()}
      onClose={onClose}
    >
      <p>{t("connectors.deleteBody")}</p>
    </ConfirmDialog>
  );
}

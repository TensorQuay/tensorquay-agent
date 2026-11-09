"use client";

import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Power, Trash2 } from "lucide-react";
import { useState } from "react";

import { newActionKey } from "../api/action-key";
import { ApiError } from "../api/error";
import { useT } from "../i18n/I18nProvider";
import { formatCompact } from "../i18n/message";
import type { Translate } from "../i18n/translate";
import type { User } from "../protocol/generated/api";
import { ActionMenu } from "../ui/ActionMenu";
import { Button } from "../ui/Button";
import { CellNote } from "../ui/CellNote";
import { ConfirmDialog } from "../ui/ConfirmDialog";
import { Skeleton } from "../ui/Skeleton";
import { TableScroll } from "../ui/TableScroll";

import type { Engine, EnginesApi } from "./admin/decision-engines";
import {
  ENGINE_SOURCES,
  EXPOSURE_SHORT,
  engineFailure,
  engineHost,
  engineLabel,
  fieldsOf,
} from "./engine-copy";
import { EngineForm } from "./EngineForm";
import { LoadFailure } from "./LoadFailure";

const KEY = ["admin-decision-engines"];
/** The open dialog: add (edit: null), edit, turn off or delete one engine. */
type Open =
  { edit: Engine | null } | { remove: Engine } | { turnOff: Engine } | null;

/**
 * Decision engines for administrators (ADR-0013 point 6): services of the installation's own that tasks whose profile
 * names one consult at each step. The gateway holds their keys; a task never sees one.
 */
export function DecisionEnginesSettings({
  me,
  api,
}: {
  me: User;
  api: EnginesApi;
}) {
  const t = useT();
  if (me.role !== "admin")
    return <p className="muted">{t("engines.adminsOnly")}</p>;
  return <Engines api={api} />;
}

function Engines({ api }: { api: EnginesApi }) {
  const t = useT();
  const cache = useQueryClient();
  const list = useQuery({
    queryKey: KEY,
    queryFn: ({ signal }) => api.engines(signal),
    retry: false,
  });
  const [notice, setNotice] = useState<string | null>(null);
  const [dialog, setDialog] = useState<Open>(null);
  const open = (next: Open) => {
    setNotice(null);
    setDialog(next);
  };
  if (list.isPending) return <Skeleton label={t("engines.loading")} />;
  if (list.isError)
    return (
      <LoadFailure
        message={t("engines.failed")}
        onRetry={() => void list.refetch()}
      />
    );
  const changed = (copy: string) => {
    setDialog(null);
    setNotice(copy);
    void cache.invalidateQueries({ queryKey: KEY });
  };
  /** Turning an engine back on refuses nothing, so it needs no confirmation. */
  async function turnOn(item: Engine) {
    setNotice(null);
    try {
      await api.putEngine(
        item.name,
        { ...fieldsOf(item), enabled: true },
        item.version,
        newActionKey(),
      );
      changed(t("engines.turnedOn", { name: engineLabel(item) }));
    } catch (failure) {
      setNotice(t(engineFailure(failure) ?? "engines.saveFailed"));
    }
  }
  return (
    <div className="stack">
      <p className="type-small muted">{t("engines.intro")}</p>
      <div className="settings-controls">
        <Button onClick={() => open({ edit: null })}>{t("engines.add")}</Button>
      </div>
      {notice && (
        <p className="type-small" role="status">
          {notice}
        </p>
      )}
      <EngineTable
        items={list.data.items}
        onEdit={(item) => open({ edit: item })}
        onDelete={(item) => open({ remove: item })}
        onTurnOff={(item) => open({ turnOff: item })}
        onTurnOn={(item) => void turnOn(item)}
      />
      {dialog && "edit" in dialog && (
        <EngineForm
          item={dialog.edit}
          api={api}
          onClose={() => setDialog(null)}
          onStale={async (name) =>
            (await list.refetch()).data?.items.find(
              (item) => item.name === name,
            )
          }
          onSaved={(item) =>
            changed(t("engines.saved", { name: engineLabel(item) }))
          }
        />
      )}
      {dialog && "remove" in dialog && (
        <DeleteEngine
          item={dialog.remove}
          api={api}
          onClose={() => setDialog(null)}
          onDeleted={() =>
            changed(t("engines.deleted", { name: engineLabel(dialog.remove) }))
          }
        />
      )}
      {dialog && "turnOff" in dialog && (
        <TurnOffEngine
          item={dialog.turnOff}
          api={api}
          onClose={() => setDialog(null)}
          onDone={() =>
            changed(
              t("engines.turnedOff", { name: engineLabel(dialog.turnOff) }),
            )
          }
        />
      )}
    </div>
  );
}

type RowActions = {
  onEdit: (item: Engine) => void;
  onDelete: (item: Engine) => void;
  onTurnOff: (item: Engine) => void;
  onTurnOn: (item: Engine) => void;
};

function EngineTable({ items, ...actions }: { items: Engine[] } & RowActions) {
  const t = useT();
  if (items.length === 0) return <p className="muted">{t("engines.empty")}</p>;
  return (
    <TableScroll label={t("engines.list")}>
      <table
        className="data-table engines-table"
        aria-label={t("engines.list")}
      >
        <thead className="type-label">
          <tr>
            <th scope="col">{t("engines.engine")}</th>
            <th scope="col">{t("engines.seesColumn")}</th>
            <th scope="col">{t("engines.status")}</th>
            <th scope="col">
              <span className="sr-only">{t("common.actions")}</span>
            </th>
          </tr>
        </thead>
        <tbody>
          {items.map((item) => (
            <EngineRow key={item.name} item={item} {...actions} />
          ))}
        </tbody>
      </table>
    </TableScroll>
  );
}

/**
 * A budget's use over its current count, a sentence per limit; none without a budget. Token figures are short
 * (WEB-U-461), the use rounded down so it never reads as the limit before it is reached; `exact` gives every figure
 * whole, for the row's tooltip (UI-GUIDELINES §10).
 */
function budgetWords(item: Engine, t: Translate, exact = false): string[] {
  const { budget, budget_used: used } = item;
  if (!budget || !used) return [];
  const period = budget.period ?? "total";
  const date = item.budget_since
    ? new Intl.DateTimeFormat(t.locale, { dateStyle: "medium" }).format(
        new Date(item.budget_since),
      )
    : "";
  const lines: string[] = [];
  if (budget.calls !== undefined)
    lines.push(
      t("engines.budgetUseCalls", {
        used: used.calls,
        limit: budget.calls,
        period,
        date,
      }),
    );
  if (budget.tokens !== undefined)
    lines.push(
      t("engines.budgetUseTokens", {
        used: exact
          ? used.tokens
          : formatCompact(used.tokens, t.locale, "down"),
        limit: exact ? budget.tokens : formatCompact(budget.tokens, t.locale),
        period,
        date,
      }),
    );
  return lines;
}

function EngineRow({
  item,
  onEdit,
  ...actions
}: { item: Engine } & RowActions) {
  const t = useT();
  const label = engineLabel(item);
  const source = item.seed ? ENGINE_SOURCES[item.seed] : undefined;
  const host = engineHost(item.url);
  const budget = budgetWords(item, t).join(" · ");
  // The line is short; its tooltip gives the figures exactly (WEB-U-461).
  const exact = budgetWords(item, t, true).join(" · ");
  return (
    <tr>
      <td>
        <div className="stack route-name">
          <span>{label}</span>
          <CellNote>
            {[host, source ? t(source) : null].filter(Boolean).join(" · ")}
          </CellNote>
        </div>
      </td>
      <td>{t(EXPOSURE_SHORT[item.exposure ?? "none"])}</td>
      <td>
        <div className="stack route-name">
          <span className="cell-pills">
            <span
              className="status-pill type-label"
              // On or off is a setting, said by its word; green is only for a check that passed (17.3).
              data-tone="muted"
            >
              {t(item.enabled ? "engines.on" : "engines.off")}
            </span>
            {item.key_missing && (
              <span className="status-pill type-label" data-tone="warn">
                {t("engines.keyMissing")}
              </span>
            )}
          </span>
          {/* One line under them (17.3): where to add a missing key, then the budget's use, whole in its tooltip (no page
              says the use, so the row is its home). */}
          {item.key_missing ? (
            <CellNote
              href="/settings/keys/shared"
              after={budget || null}
              title={[t("engines.addKey"), exact].filter(Boolean).join(" · ")}
            >
              {t("engines.addKey")}
            </CellNote>
          ) : (
            budget && <CellNote title={exact}>{budget}</CellNote>
          )}
        </div>
      </td>
      <td className="data-cell-action">
        <Button
          variant="ghost"
          aria-label={t("engines.editName", { name: label })}
          onClick={() => onEdit(item)}
        >
          {t("engines.edit")}
        </Button>
        <EngineMenu item={item} {...actions} />
      </td>
    </tr>
  );
}

/** The row's "…" menu (UI-GUIDELINES §0.4): Turn off or Turn on, and Delete. */
function EngineMenu({
  item,
  onDelete,
  onTurnOff,
  onTurnOn,
}: { item: Engine } & Omit<RowActions, "onEdit">) {
  const t = useT();
  return (
    <ActionMenu
      label={t("engines.moreActions", { name: engineLabel(item) })}
      actions={[
        {
          id: "power",
          label: t(item.enabled ? "engines.turnOff" : "engines.turnOn"),
          icon: Power,
          onSelect: () => (item.enabled ? onTurnOff(item) : onTurnOn(item)),
        },
        {
          id: "delete",
          label: t("engines.delete"),
          icon: Trash2,
          danger: true,
          onSelect: () => onDelete(item),
        },
      ]}
    />
  );
}

/** Says what turning an engine off does to the tasks that use it, then turns it off with the version read. */
function TurnOffEngine({
  item,
  api,
  onDone,
  onClose,
}: {
  item: Engine;
  api: EnginesApi;
  onDone: () => void;
  onClose: () => void;
}) {
  const t = useT();
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<Error | null>(null);
  async function turnOff() {
    setPending(true);
    setError(null);
    try {
      await api.putEngine(
        item.name,
        { ...fieldsOf(item), enabled: false },
        item.version,
        newActionKey(),
      );
      onDone();
    } catch (failure) {
      setError(
        failure instanceof Error ? failure : new Error("turn off failed"),
      );
    } finally {
      setPending(false);
    }
  }
  return (
    <ConfirmDialog
      title={t("engines.turnOffTitle", { name: engineLabel(item) })}
      confirm={t("engines.turnOff")}
      pending={pending}
      error={error}
      errorCopy={(refusal) => {
        const copy = engineFailure(refusal);
        return copy === "engines.conflict"
          ? t("engines.conflictDelete")
          : copy
            ? t(copy)
            : null;
      }}
      onConfirm={() => void turnOff()}
      onClose={onClose}
    >
      <p>{t("engines.turnOffBody")}</p>
    </ConfirmDialog>
  );
}

/**
 * Asks once, naming the engine, then deletes it with the version the page read. An engine that profiles or open tasks
 * use is refused; the dialog says who, and that turning it off is the way.
 */
function DeleteEngine({
  item,
  api,
  onDeleted,
  onClose,
}: {
  item: Engine;
  api: EnginesApi;
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
      await api.deleteEngine(item.name, item.version, newActionKey());
      onDeleted();
    } catch (failure) {
      setError(failure instanceof Error ? failure : new Error("delete failed"));
    } finally {
      setPending(false);
    }
  }
  return (
    <ConfirmDialog
      title={t("engines.deleteTitle", { name: engineLabel(item) })}
      confirm={t("engines.deleteConfirm")}
      pending={pending}
      error={error}
      errorCopy={(refusal) => {
        if (refusal instanceof ApiError && refusal.inUse) {
          const { profiles = [], personal_profiles = 0 } = refusal.inUse;
          return t("engines.inUseDetail", {
            tasks: refusal.inUse.conversations,
            profiles: profiles.length + personal_profiles,
          });
        }
        const copy = engineFailure(refusal);
        return copy === "engines.conflict"
          ? t("engines.conflictDelete")
          : copy
            ? t(copy)
            : null;
      }}
      onConfirm={() => void remove()}
      onClose={onClose}
    >
      <p>{t("engines.deleteBody")}</p>
    </ConfirmDialog>
  );
}

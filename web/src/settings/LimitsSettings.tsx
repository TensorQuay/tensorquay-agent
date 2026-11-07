"use client";

import { useQuery, useQueryClient } from "@tanstack/react-query";
import Link from "next/link";
import { type ReactNode, useState } from "react";

import { newActionKey } from "../api/action-key";
import { useT } from "../i18n/I18nProvider";
import type { EnvironmentSize, User } from "../protocol/generated/api";
import { Button } from "../ui/Button";
import { ConfirmDialog } from "../ui/ConfirmDialog";
import { Skeleton } from "../ui/Skeleton";
import { TableScroll } from "../ui/TableScroll";

import type { Limit, LimitsApi } from "./admin/limits";
import {
  HelperSettingEdit,
  HelperSettingRows,
  type Setting,
  TEAM_SETTINGS_KEY,
  useHelperSettings,
} from "./HelperSettings";
import {
  APPLIES,
  display,
  isDesktopLimit,
  isHelperLimit,
  MIB,
  NAMES,
  refusal,
} from "./limit-copy";
import { LimitForm } from "./LimitForm";
import { LoadFailure } from "./LoadFailure";

const KEY = ["admin-limits"];
/** Where the link goes in a sentence, whatever the language's word order. */
const LINK_MARK = "\u0000link\u0000";
/** The open dialog: edit or reset one limit. */
type Open = { edit: Limit } | { reset: Limit } | null;

/** Admission and sandbox limits for administrators (slice 5, ADR-0028): changes apply to what starts next. */
export function LimitsSettings({ me, api }: { me: User; api: LimitsApi }) {
  const t = useT();
  if (me.role !== "admin")
    return <p className="muted">{t("limits.adminsOnly")}</p>;
  return <Limits api={api} />;
}

function Limits({ api }: { api: LimitsApi }) {
  const t = useT();
  const cache = useQueryClient();
  const list = useQuery({
    queryKey: KEY,
    queryFn: ({ signal }) => api.limits(signal),
    retry: false,
  });
  const [notice, setNotice] = useState<string | null>(null);
  const [dialog, setDialog] = useState<Open>(null);
  if (list.isPending) return <Skeleton label={t("limits.loading")} />;
  if (list.isError)
    return (
      <LoadFailure
        message={t("limits.failed")}
        onRetry={() => void list.refetch()}
      />
    );
  const changed = (copy: string) => {
    setDialog(null);
    setNotice(copy);
    void cache.invalidateQueries({ queryKey: KEY });
  };
  const open = (next: Open) => {
    setNotice(null);
    setDialog(next);
  };
  const general = list.data.items.filter(
    (item) => !isHelperLimit(item.key) && !isDesktopLimit(item.key),
  );
  const desktop = list.data.items.filter((item) => isDesktopLimit(item.key));
  const helpers = list.data.items.filter((item) => isHelperLimit(item.key));
  return (
    <div className="stack">
      <p className="type-small muted">{t("limits.intro")}</p>
      {notice && (
        <p className="type-small" role="status">
          {notice}
        </p>
      )}
      <LimitTable
        label={t("limits.list")}
        items={general}
        onEdit={(item) => open({ edit: item })}
      />
      <Largest ceiling={list.data.ceiling} />
      {desktop.length > 0 && (
        <section className="stack" aria-labelledby="limits-desktop">
          <h2 id="limits-desktop" className="type-label muted">
            {t("limits.desktopSection")}
          </h2>
          <p className="type-small muted">{t("limits.desktopIntro")}</p>
          <LimitTable
            label={t("limits.desktopList")}
            items={desktop}
            onEdit={(item) => open({ edit: item })}
          />
        </section>
      )}
      {helpers.length > 0 && (
        <Helpers
          api={api}
          items={helpers}
          all={list.data.items}
          onEdit={(item) => open({ edit: item })}
          onSaved={(copy) => {
            setNotice(copy);
            void cache.invalidateQueries({ queryKey: TEAM_SETTINGS_KEY });
          }}
          onOpen={() => setNotice(null)}
        />
      )}
      {dialog && "edit" in dialog && (
        <LimitForm
          item={dialog.edit}
          api={api}
          onClose={() => setDialog(null)}
          onReset={(item) => open({ reset: item })}
          onReload={async () =>
            (await list.refetch()).data?.items.find(
              (item) => item.key === dialog.edit.key,
            )
          }
          onSaved={(item) =>
            changed(
              t("limits.saved", {
                name: t(NAMES[item.key]),
                value: display(t, item.unit, item.value),
              }),
            )
          }
        />
      )}
      {dialog && "reset" in dialog && (
        <ResetLimit
          item={dialog.reset}
          api={api}
          onClose={() => setDialog(null)}
          onReset={(item) =>
            changed(
              t("limits.resetDone", {
                name: t(NAMES[item.key]),
                value: display(t, item.unit, item.value),
              }),
            )
          }
        />
      )}
    </div>
  );
}

function LimitTable({
  label,
  items,
  onEdit,
  first,
  last,
  note,
}: {
  label: string;
  items: Limit[];
  onEdit: (item: Limit) => void;
  /** A read-only row before the limits. */
  first?: ReactNode;
  /** Rows after the limits. */
  last?: ReactNode;
  /** A sentence under one limit's value, when the page has one. */
  note?: (item: Limit) => string | null;
}) {
  const t = useT();
  return (
    <TableScroll label={label}>
      <table className="data-table limits-table" aria-label={label}>
        <thead className="type-label">
          <tr>
            <th scope="col">{t("limits.name")}</th>
            {/* On a phone the limit's name says what the value is: no "Value" above it (WEB-U-361). */}
            <th scope="col" data-card-label="none">
              {t("limits.value")}
            </th>
            <th scope="col">
              <span className="sr-only">{t("common.actions")}</span>
            </th>
          </tr>
        </thead>
        <tbody>
          {first}
          {items.map((item) => (
            <LimitRow
              key={item.key}
              item={item}
              onEdit={onEdit}
              note={note?.(item) ?? null}
            />
          ))}
          {last}
        </tbody>
      </table>
    </TableScroll>
  );
}

function LimitRow({
  item,
  onEdit,
  note,
}: {
  item: Limit;
  onEdit: (item: Limit) => void;
  note: string | null;
}) {
  const t = useT();
  const name = t(NAMES[item.key]);
  // A value set here is the only one to name its source (its reset is in the edit dialog); the rest come from the file.
  const setHere = item.last_set_by === "admin";
  const applies = APPLIES[item.key];
  return (
    <tr>
      <td>{name}</td>
      <td>
        <div className="stack route-name">
          <span>{display(t, item.unit, item.value)}</span>
          {setHere && (
            <span className="type-small muted">{t("limits.setHere")}</span>
          )}
          {applies && (
            <span className="type-small muted">{t(applies.short)}</span>
          )}
          <LimitNotes item={item} />
          {note && <span className="type-small limit-note">{note}</span>}
        </div>
      </td>
      <td className="data-cell-action">
        <Button
          variant="ghost"
          aria-label={t("limits.editName", { name })}
          onClick={() => onEdit(item)}
        >
          {t("limits.edit")}
        </Button>
      </td>
    </tr>
  );
}

/**
 * What differs from the configuration file, in words. At load the gateway moves a stored value the file's bounds no
 * longer allow to the nearest bound and keeps the old one (`clamped_from`), so the row shows the bound and the note what
 * it was; or the file set a value below what can be set here.
 */
function LimitNotes({ item }: { item: Limit }) {
  const t = useT();
  const shown = (value: number) => display(t, item.unit, value);
  const clamped = item.clamped_from;
  return (
    <>
      {clamped !== null && (clamped > item.max || clamped < item.min) && (
        <span className="type-small limit-note">
          {clamped > item.max
            ? t("limits.clampedDown", {
                was: shown(clamped),
                max: shown(item.max),
              })
            : t("limits.clampedUp", {
                was: shown(clamped),
                min: shown(item.min),
              })}
        </span>
      )}
      {item.below_floor && item.value < item.min && (
        <span className="type-small limit-note">
          {t("limits.belowFloorWords", {
            value: shown(item.value),
            floor: shown(item.min),
          })}
        </span>
      )}
    </>
  );
}

/**
 * The helper limits (ADR-0034 S6), listed only while agent teams are on: the switch read-only (gateway.toml, with a
 * restart), then the limits. A lead task and its helpers must fit, so the helpers a lead task can have now are one
 * fewer than the slots or the per-user cap when either is the smaller; the gateway applies the smaller number, and the
 * page says so beside the value.
 */
function Helpers({
  api,
  items,
  all,
  onEdit,
  onSaved,
  onOpen,
}: {
  api: LimitsApi;
  items: Limit[];
  all: Limit[];
  onEdit: (item: Limit) => void;
  onSaved: (copy: string) => void;
  onOpen: () => void;
}) {
  const t = useT();
  const read = useHelperSettings(api);
  const [editing, setEditing] = useState<Setting | null>(null);
  const room = (["admission.slots", "admission.per_user"] as const)
    .map((key) => all.find((item) => item.key === key))
    .filter((item): item is Limit => item !== undefined)
    .sort((a, b) => a.value - b.value)[0];
  // A done helper can be given more work, so it holds its place until stopped (ADR-0034 member states).
  const note = (item: Limit) =>
    item.key === "agent_teams.max_members"
      ? [
          room && room.value - 1 < item.value
            ? t("limits.capLowered", {
                value: display(t, "helpers", Math.max(room.value - 1, 0)),
                name: t(NAMES[room.key]),
                limit: display(t, room.unit, room.value),
              })
            : null,
          t("limits.doneCounts"),
        ]
          .filter(Boolean)
          .join(" ")
      : null;
  return (
    <section className="stack" aria-labelledby="limits-helpers">
      <h2 id="limits-helpers" className="type-label muted">
        {t("limits.helpersSection")}
      </h2>
      <p className="type-small muted">{t("limits.helpersIntro")}</p>
      <LimitTable
        label={t("limits.helperList")}
        items={items}
        onEdit={onEdit}
        note={note}
        first={
          <tr>
            <td>{t("limits.teamsEnabled")}</td>
            <td>
              <div className="stack route-name">
                <span>{t("limits.on")}</span>
                <span className="type-small muted">
                  {t("limits.switchFromFile")}
                </span>
              </div>
            </td>
            <td />
          </tr>
        }
        last={
          <HelperSettingRows
            read={read}
            onEdit={(setting) => {
              onOpen();
              setEditing(setting);
            }}
          />
        }
      />
      {editing && (
        <HelperSettingEdit
          setting={editing}
          read={read}
          api={api}
          onClose={() => setEditing(null)}
          onSaved={(copy) => {
            setEditing(null);
            onSaved(copy);
          }}
        />
      )}
    </section>
  );
}

/** Asks once, naming the limit and the default it restores, then resets with the version read. */
function ResetLimit({
  item,
  api,
  onReset,
  onClose,
}: {
  item: Limit;
  api: LimitsApi;
  onReset: (item: Limit) => void;
  onClose: () => void;
}) {
  const t = useT();
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<Error | null>(null);
  async function reset() {
    setPending(true);
    setError(null);
    try {
      onReset(await api.resetLimit(item.key, item.version, newActionKey()));
    } catch (failure) {
      setError(failure instanceof Error ? failure : new Error("reset failed"));
    } finally {
      setPending(false);
    }
  }
  return (
    <ConfirmDialog
      title={t("limits.resetTitle", {
        name: t(NAMES[item.key]),
        value: display(t, item.unit, item.default),
      })}
      confirm={t("limits.resetConfirm")}
      pending={pending}
      error={error}
      errorCopy={(refused) => t(refusal(refused, item.key))}
      onConfirm={() => void reset()}
      onClose={onClose}
    >
      <p>{t("limits.resetBody")}</p>
    </ConfirmDialog>
  );
}

/**
 * The configuration file's ceiling for every environment's size (the owner's direction: sizes belong to each
 * environment), read-only here; the sizes themselves are set on Environments.
 */
function Largest({ ceiling }: { ceiling: EnvironmentSize }) {
  const t = useT();
  const [before, after] = t("limits.ceilingSource", { link: LINK_MARK }).split(
    LINK_MARK,
  );
  return (
    <section className="stack" aria-label={t("limits.largest")}>
      <h2 className="type-label muted">{t("limits.largest")}</h2>
      <p className="type-small">
        {t("limits.ceiling", {
          cpus: display(t, "millicores", ceiling.cpus * 1000),
          memory: display(t, "bytes", ceiling.memory_mib * MIB),
          pids: display(t, "processes", ceiling.pids),
          disk: display(t, "bytes", ceiling.workspace_mib * MIB),
        })}{" "}
        {before}
        <Link href="/settings/environments">{t("settings.environments")}</Link>
        {after}
      </p>
    </section>
  );
}

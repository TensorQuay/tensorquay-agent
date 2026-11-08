"use client";

import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Plus } from "lucide-react";
import { useState, type FormEvent } from "react";

import { newActionKey } from "../api/action-key";
import { useT } from "../i18n/I18nProvider";
import type { MessageKey } from "../i18n/translate";
import type { User } from "../protocol/generated/api";
import { Button } from "../ui/Button";
import { CellNote } from "../ui/CellNote";
import { ConfirmDialog } from "../ui/ConfirmDialog";
import { Dialog } from "../ui/Dialog";
import { FormRefusal } from "../ui/FormRefusal";
import { Icon } from "../ui/Icon";
import { Input } from "../ui/Input";
import { Skeleton } from "../ui/Skeleton";
import { TableScroll } from "../ui/TableScroll";

import type { AdminApi, SecretItem, SecretList } from "./admin/contract";
import { DateLabel } from "./DateLabel";
import { LoadFailure } from "./LoadFailure";
import { ReplaceSecret } from "./ReplaceSecret";
import { RowMenu } from "./RowMenu";
import {
  fieldProblems,
  keyName,
  reasonCopy,
  secretProblems,
  writeFailure,
} from "./secret-copy";
import {
  byKindAndName,
  ConsumerChoices,
  id,
  SecretBindings,
} from "./SecretBindings";
import { SecretReads } from "./SecretReads";
import { SecretReminder } from "./SecretReminder";

const SOURCES: Readonly<Record<string, MessageKey>> = {
  current: "secrets.fromSeed",
  removed: "secrets.seedRemoved",
  none: "secrets.addedHere",
};

/** Shared secrets for administrators (FR-22, FR-28): names only; values are write-only. */
export function SecretsSettings({ me, api }: { me: User; api: AdminApi }) {
  const t = useT();
  if (me.role !== "admin")
    return <p className="muted">{t("secrets.adminsOnly")}</p>;
  return <Secrets api={api} />;
}

/** The shared secret list, its refresh after a stale write, and its invalidation after a change. */
function useSecretList(api: AdminApi) {
  const cache = useQueryClient();
  const list = useQuery({
    queryKey: ["admin-secrets"],
    queryFn: ({ signal }) => api.secrets(signal),
    retry: false,
  });
  const refresh = async () => (await list.refetch()).data;
  const changed = () =>
    void cache.invalidateQueries({ queryKey: ["admin-secrets"] });
  return { list, refresh, changed };
}

function Secrets({ api }: { api: AdminApi }) {
  const t = useT();
  const { list, refresh, changed } = useSecretList(api);
  const [notice, setNotice] = useState<string | null>(null);
  const [replacing, setReplacing] = useState<SecretItem | null>(null);
  const [deleting, setDeleting] = useState<SecretItem | null>(null);
  const [reminding, setReminding] = useState<SecretItem | null>(null);
  const [reading, setReading] = useState<SecretItem | null>(null);
  const [binding, setBinding] = useState<SecretItem | null>(null);
  const [adding, setAdding] = useState(false);
  const people = useQuery({
    queryKey: ["admin-people"],
    queryFn: ({ signal }) => api.people(signal),
    enabled: reading !== null,
    retry: false,
  });
  if (list.isPending) return <Skeleton label={t("secrets.loading")} />;
  if (list.isError)
    return (
      <LoadFailure
        message={t("secrets.failed")}
        onRetry={() => void list.refetch()}
      />
    );
  const { items, limit } = list.data;
  return (
    <div className="stack">
      {/* One main action, as on Mine (K2): the add form is a dialog, not a form under the table. */}
      <div className="settings-bar">
        <p className="type-small muted">{t("secrets.intro")}</p>
        <Button
          variant="primary"
          onClick={() => {
            setNotice(null);
            setAdding(true);
          }}
        >
          <Icon glyph={Plus} />
          {t("secrets.addTitle")}
        </Button>
      </div>
      {notice && (
        <p className="type-small" role="status">
          {notice}
        </p>
      )}
      <SecretTable
        items={items}
        onReplace={(item) => {
          setNotice(null);
          setReplacing(item);
        }}
        onDelete={(item) => {
          setNotice(null);
          setDeleting(item);
        }}
        onRemind={(item) => {
          setNotice(null);
          setReminding(item);
        }}
        onReads={(item) => setReading(item)}
        onBindings={(item) => {
          setNotice(null);
          setBinding(item);
        }}
      />
      {binding && (
        <SecretBindings
          item={binding}
          api={api}
          onClose={() => setBinding(null)}
          onStale={async () =>
            (await refresh())?.items.find((item) => item.ref === binding.ref)
          }
          onDone={() => {
            setNotice(
              t("secrets.bindingsSaved", { name: keyName(binding.ref) }),
            );
            setBinding(null);
            changed();
          }}
        />
      )}
      {reading && (
        <SecretReads
          title={t("reads.titleSecret", { name: keyName(reading.ref) })}
          load={(before, signal) =>
            api.secretReads(keyName(reading.ref), before, signal)
          }
          // Until the people load, a read's person shows as unknown rather than holding the list back.
          emails={people.data ?? {}}
          onClose={() => setReading(null)}
        />
      )}
      {adding && (
        <AddSecret
          api={api}
          limit={limit}
          onClose={() => setAdding(false)}
          onAdded={(item, bindingsSaved) => {
            setNotice(
              t(bindingsSaved ? "secrets.added" : "secrets.addedNoBindings", {
                name: keyName(item.ref),
              }),
            );
            setAdding(false);
            changed();
          }}
        />
      )}
      {replacing && (
        <ReplaceSecret
          item={replacing}
          api={api}
          limit={limit}
          onClose={() => setReplacing(null)}
          onStale={async () =>
            (await refresh())?.items.find((item) => item.ref === replacing.ref)
          }
          onDone={() => {
            setNotice(t("secrets.updated", { name: keyName(replacing.ref) }));
            setReplacing(null);
            changed();
          }}
        />
      )}
      {reminding && (
        <SecretReminder
          item={reminding}
          api={api}
          onClose={() => setReminding(null)}
          onStale={async () =>
            (await refresh())?.items.find((item) => item.ref === reminding.ref)
          }
          onDone={() => {
            setNotice(
              t("secrets.reminderSaved", { name: keyName(reminding.ref) }),
            );
            setReminding(null);
            changed();
          }}
        />
      )}
      {deleting && (
        <DeleteSecret
          item={deleting}
          api={api}
          limit={limit}
          onClose={() => setDeleting(null)}
          onStale={refresh}
          onDeleted={() => {
            setNotice(t("secrets.deleted", { name: keyName(deleting.ref) }));
            setDeleting(null);
            changed();
          }}
        />
      )}
    </div>
  );
}

/**
 * The models that send a key: up to two by name; more as a count, with the list on hover and for screen readers, so the
 * row stays one line.
 */
function UsedBy({ routes }: { routes: readonly string[] }) {
  const t = useT();
  if (routes.length === 0)
    return <span className="muted">{t("secrets.notUsed")}</span>;
  const list = routes.join(", ");
  if (routes.length <= 2) return <span>{list}</span>;
  return (
    <span>
      <span className="nowrap" title={list} aria-hidden="true">
        {t("secrets.modelCount", { count: routes.length })}
      </span>
      <span className="sr-only">{list}</span>
    </span>
  );
}

function SecretTable({
  items,
  onReplace,
  onDelete,
  onRemind,
  onReads,
  onBindings,
}: {
  items: SecretList["items"];
  onReplace: (item: SecretItem) => void;
  onDelete: (item: SecretItem) => void;
  onRemind: (item: SecretItem) => void;
  onReads: (item: SecretItem) => void;
  onBindings: (item: SecretItem) => void;
}) {
  const t = useT();
  if (items.length === 0) return <p className="muted">{t("secrets.empty")}</p>;
  // Where a key came from, for a key from the configuration file: a note under its name, no column. A key added here, the
  // usual, says nothing; a file key always says so, since this list is the only place that does.
  return (
    <TableScroll label={t("secrets.list")}>
      <table className="data-table" aria-label={t("secrets.list")}>
        <thead className="type-label">
          <tr>
            <th scope="col">{t("secrets.name")}</th>
            <th scope="col">{t("secrets.usedBy")}</th>
            <th scope="col">{t("secrets.lastChanged")}</th>
            <th scope="col">
              <span className="sr-only">{t("common.actions")}</span>
            </th>
          </tr>
        </thead>
        <tbody>
          {items.map((item) => {
            const used = item.routes.length > 0;
            const dueId = `due-${item.ref}`;
            const name = keyName(item.ref);
            // When the value last changed: a same-value write or a reminder change does not count.
            const changedAt = item.rotated_at ?? item.updated_at;
            const days = item.rotate_after_days ?? null;
            return (
              <tr key={item.ref}>
                <td className="secret-changed">
                  {/* Its value line: the name (a long one truncates, whole on hover and in the row's name, §7) and a due
                      reminder's pill beside it; then one muted line, where it came from (17.3). */}
                  <span className="stack route-name">
                    <span className="cell-pills">
                      <span className="secret-ref-line" title={item.ref}>
                        {name}
                      </span>{" "}
                      {item.rotation_due && (
                        <span
                          className="status-pill type-label"
                          data-tone="warn"
                          aria-describedby={dueId}
                        >
                          {t("secrets.rotationDue")}
                        </span>
                      )}
                    </span>
                    {item.seed && (
                      <CellNote>
                        {t(SOURCES[item.seed] ?? "secrets.addedHere")}
                      </CellNote>
                    )}
                  </span>
                  {item.rotation_due && (
                    <span id={dueId} className="sr-only">
                      {t("secrets.rotationDueWhy", {
                        date: new Intl.DateTimeFormat(t.locale, {
                          dateStyle: "long",
                        }).format(new Date(changedAt)),
                        days: days ?? 0,
                      })}
                    </span>
                  )}
                </td>
                <td className="secret-changed">
                  {/* Who may use it is in the row's menu, Who may use: not repeated here. */}
                  <UsedBy routes={item.routes} />
                </td>
                <td className="secret-changed">
                  <span className="stack route-name">
                    <span className="nowrap">
                      <DateLabel value={changedAt} />
                    </span>
                    <CellNote>
                      {days === null
                        ? t("secrets.noReminder")
                        : t("secrets.reminderEvery", { days })}
                    </CellNote>
                  </span>
                </td>
                {/* One actions column (UI-GUIDELINES §7, as on Models): Replace, and the rest in the row's menu. */}
                <td className="data-cell-action">
                  <Button
                    variant="ghost"
                    aria-label={t("secrets.replaceName", { name })}
                    onClick={() => onReplace(item)}
                  >
                    {t("secrets.replace")}
                  </Button>
                  <RowMenu
                    label={t("secrets.more", { name })}
                    actions={[
                      {
                        id: "reads",
                        label: t("reads.show"),
                        name: t("reads.showSecret", { name }),
                        onSelect: () => onReads(item),
                      },
                      {
                        id: "bindings",
                        label: t("secrets.bindings"),
                        name: t("secrets.bindingsName", { name }),
                        onSelect: () => onBindings(item),
                      },
                      {
                        id: "reminder",
                        label: t("secrets.reminder"),
                        name: t("secrets.reminderName", { name }),
                        onSelect: () => onRemind(item),
                      },
                      {
                        id: "delete",
                        label: t("secrets.delete"),
                        name: t("secrets.deleteName", { name }),
                        danger: true,
                        unavailable: used
                          ? t("secrets.deleteBlocked")
                          : undefined,
                        onSelect: () => onDelete(item),
                      },
                    ]}
                  />
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </TableScroll>
  );
}

/** Adds a shared key in a dialog (K2), checked before sending; the value is sent once and never kept. */
function AddSecret({
  api,
  limit,
  onAdded,
  onClose,
}: {
  api: AdminApi;
  limit: number;
  /** The key as added, and whether who may use it was saved too (false only when that second step failed). */
  onAdded: (item: SecretItem, bindingsSaved: boolean) => void;
  onClose: () => void;
}) {
  const t = useT();
  const choices = useQuery({
    queryKey: ["admin-secret-consumer-choices"],
    queryFn: ({ signal }) => api.consumerChoices(signal),
    retry: false,
  });
  const [chosen, setChosen] = useState<ReadonlySet<string>>(new Set());
  const toggle = (key: string) =>
    setChosen((current) => {
      const next = new Set(current);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });
  const [pending, setPending] = useState(false);
  const [problems, setProblems] = useState<Record<string, string>>({});
  const [failure, setFailure] = useState<string | null>(null);
  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const form = event.currentTarget;
    const name = form.elements.namedItem("name") as HTMLInputElement;
    const value = form.elements.namedItem("value") as HTMLInputElement;
    const write = { name: name.value, value: value.value };
    const named = (list: readonly { field: string; reason: string }[]) =>
      Object.fromEntries(
        list.map((problem) => [problem.field, t(reasonCopy(problem.reason))]),
      );
    const local = secretProblems(write, true);
    setFailure(null);
    setProblems(named(local));
    if (local.length) return;
    setPending(true);
    try {
      const item = await api.putSecret(write, newActionKey());
      // Who may use it, in the same step (F8): with any that already name it, which the gateway allowed on its own.
      let saved = true;
      if (chosen.size > 0) {
        const consumers = [
          ...(item.consumers ?? []),
          ...(choices.data ?? []).filter((choice) => chosen.has(id(choice))),
        ]
          .filter(
            (consumer, index, all) =>
              all.findIndex((other) => id(other) === id(consumer)) === index,
          )
          .sort(byKindAndName);
        try {
          await api.setConsumers(keyName(item.ref), consumers, item.version);
        } catch {
          saved = false;
        }
      }
      form.reset();
      onAdded(item, saved);
    } catch (error) {
      setProblems(named(fieldProblems(error)));
      const copy = writeFailure(error);
      setFailure(copy ? t(copy, { limit }) : null);
    } finally {
      setPending(false);
    }
  }
  return (
    <Dialog
      open
      title={t("secrets.addTitle")}
      onClose={() => {
        if (!pending) onClose();
      }}
    >
      <form className="stack" onSubmit={(event) => void submit(event)}>
        <Input
          label={t("secrets.name")}
          name="name"
          autoComplete="off"
          spellCheck={false}
          aria-describedby="secret-name-hint"
          {...(problems.name ? { error: problems.name } : {})}
        />
        <p id="secret-name-hint" className="type-small muted">
          {t("secrets.nameHint")}
        </p>
        <Input
          label={t("secrets.value")}
          name="value"
          type="password"
          autoComplete="off"
          spellCheck={false}
          {...(problems.value ? { error: problems.value } : {})}
        />
        {choices.data && choices.data.length > 0 && (
          <>
            <ConsumerChoices
              legend={t("secrets.useItFor")}
              choices={choices.data}
              chosen={chosen}
              onToggle={toggle}
            />
            <p className="type-small muted">{t("secrets.alreadyNamed")}</p>
          </>
        )}
        {failure && <p className="field-error">{failure}</p>}
        <FormRefusal />
        <div className="dialog-actions">
          <Button
            variant="secondary"
            type="button"
            disabled={pending}
            onClick={onClose}
          >
            {t("common.cancel")}
          </Button>
          <Button type="submit" loading={pending}>
            {t("secrets.add")}
          </Button>
        </div>
      </form>
    </Dialog>
  );
}

/** Asks once, naming the secret, then deletes it with the version the page read. */
function DeleteSecret({
  item,
  api,
  limit,
  onDeleted,
  onStale,
  onClose,
}: {
  item: SecretItem;
  api: AdminApi;
  limit: number;
  onDeleted: () => void;
  onStale: () => Promise<unknown>;
  onClose: () => void;
}) {
  const t = useT();
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<Error | null>(null);
  async function remove() {
    setPending(true);
    setError(null);
    try {
      await api.deleteSecret(keyName(item.ref), item.version, newActionKey());
      onDeleted();
    } catch (failure) {
      setError(failure instanceof Error ? failure : new Error("delete failed"));
      const copy = writeFailure(failure);
      if (copy === "secrets.conflict" || copy === "secrets.gone")
        await onStale();
    } finally {
      setPending(false);
    }
  }
  return (
    <ConfirmDialog
      title={t("secrets.deleteTitle", { name: keyName(item.ref) })}
      confirm={t("secrets.deleteConfirm")}
      pending={pending}
      error={error}
      errorCopy={(refusal) => {
        const copy = writeFailure(refusal);
        return copy ? t(copy, { limit }) : null;
      }}
      onConfirm={() => void remove()}
      onClose={onClose}
    >
      <p>{t("secrets.deleteBody")}</p>
    </ConfirmDialog>
  );
}

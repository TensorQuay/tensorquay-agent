"use client";

import { useQuery, useQueryClient } from "@tanstack/react-query";
import { KeyRound, Plus, Trash2 } from "lucide-react";
import Link from "next/link";
import { useEffect, useId, useRef, useState } from "react";

import { ApiError } from "../../api/error";
import { useT } from "../../i18n/I18nProvider";
import { ActionMenu } from "../../ui/ActionMenu";
import { Button } from "../../ui/Button";
import { ConfirmDialog } from "../../ui/ConfirmDialog";
import { EmptyState } from "../../ui/EmptyState";
import { Icon } from "../../ui/Icon";
import { Popover } from "../../ui/Popover";
import { Skeleton } from "../../ui/Skeleton";
import { TableScroll } from "../../ui/TableScroll";
import { LoadFailure } from "../LoadFailure";

import { formatAddress } from "./address";
import type {
  AdminHostKeysApi,
  HostKey,
  HostKeysApi,
  PersonalKeyRow,
} from "./contract";
import { HOST_KEYS_NOTICE, KEYS_PAGE } from "./HostKeyList";
import { HostKeyPage, type ListPage } from "./HostKeyPage";
import { lastUseTime, lastUseWords } from "./last-use";

export const SHARED_PAGE = `${KEYS_PAGE}/shared`;
/** Every shared key and members' own keys, as an administrator lists them. */
export const ADMIN_HOST_KEYS = ["admin-host-keys"];
/** The shared keys alone, for a shared key's page. */
const SHARED_ONLY = ["admin-host-keys-shared"];
const SHARED_LIST: ListPage = {
  href: SHARED_PAGE,
  label: "hostKeys.sharedKeys",
  queryKey: SHARED_ONLY,
};
const PEOPLE = ["admin-host-keys-people"];

/**
 * The administrators' API as a key's page reads one: its list is the shared keys (members' own keys can't be edited),
 * no shared names besides them, and adding or changing goes to the shared-key routes, with `users`.
 */
export function sharedKeyStore(api: AdminHostKeysApi): HostKeysApi {
  return {
    keys: async (signal) => ({ items: (await api.keys(signal)).items }),
    shared: () => Promise.resolve({ items: [] }),
    addKey: (key) => api.addShared(key),
    updateKey: (id, key) => api.updateShared(id, key),
    deleteKey: (id, version) => api.deleteKey(id, version),
    reads: (id, before, signal) => api.reads(id, before, signal),
  };
}

/** A shared key's page, or Add a shared key (ADR-0037 §1, §9): a key's page with Who may use. */
export function SharedKeyPage({
  api,
  id,
  onDone,
  now,
  timeZone,
}: {
  api: AdminHostKeysApi;
  id?: string;
  onDone?: (notice: string) => void;
  now?: () => Date;
  timeZone?: string;
}) {
  const t = useT();
  const people = useQuery({
    queryKey: PEOPLE,
    queryFn: ({ signal }) => api.people(signal),
    retry: false,
  });
  if (people.isPending) return <Skeleton label={t("hostKeys.loading")} />;
  if (people.isError)
    return (
      <LoadFailure
        message={t("hostKeys.peopleFailed")}
        onRetry={() => void people.refetch()}
      />
    );
  return (
    <HostKeyPage
      api={sharedKeyStore(api)}
      list={SHARED_LIST}
      people={people.data}
      {...(id === undefined ? {} : { id })}
      {...(onDone ? { onDone } : {})}
      {...(now ? { now } : {})}
      {...(timeZone ? { timeZone } : {})}
    />
  );
}

/**
 * "Everyone", or "2 people" that opens the list, by pointer, keyboard or touch alike.
 */
function WhoMayUseCell({ item }: { item: HostKey }) {
  const t = useT();
  const anchor = useRef<HTMLButtonElement>(null);
  const [open, setOpen] = useState(false);
  if (!item.users || item.users === "all") return <>{t("hostKeys.everyone")}</>;
  return (
    <>
      <Button
        ref={anchor}
        variant="ghost"
        aria-expanded={open}
        onClick={() => setOpen(!open)}
      >
        {t("hostKeys.peopleCount", { count: item.users.length })}
      </Button>
      {open && (
        <Popover
          anchor={anchor}
          label={t("hostKeys.whoFor", { name: item.name })}
          placement="below-start"
          onClose={() => setOpen(false)}
        >
          <ul className="stack type-small">
            {item.users.map((user) => (
              <li key={user.id}>{user.email || t("usage.removedAccount")}</li>
            ))}
          </ul>
        </Popover>
      )}
    </>
  );
}

/**
 * Keys › Shared for administrators: the shared keys, with who may use each and its last use (by whom on hover), and Add
 * a shared key; then members' own keys, which an administrator may only delete. A gateway without host keys answers
 * 404, and the tab leaves them out.
 */
export function AdminSharedKeys({
  api,
  now = () => new Date(),
  timeZone,
}: {
  api: AdminHostKeysApi;
  now?: () => Date;
  timeZone?: string;
}) {
  const t = useT();
  const cache = useQueryClient();
  const heading = useId();
  const [notice, setNotice] = useState<string | null>(
    () => cache.getQueryData<string>(HOST_KEYS_NOTICE) ?? null,
  );
  useEffect(() => {
    cache.removeQueries({ queryKey: HOST_KEYS_NOTICE, exact: true });
  }, [cache]);
  // By id, so a refetch after a conflict gives the dialog the key as it is now.
  const [deleting, setDeleting] = useState<string | null>(null);
  const keys = useQuery({
    queryKey: ADMIN_HOST_KEYS,
    queryFn: ({ signal }) => api.keys(signal),
    retry: false,
  });
  if (keys.error instanceof ApiError && keys.error.status === 404) return null;
  const deletingItem = keys.data?.personal.find((item) => item.id === deleting);
  const add = (
    <Link
      href={`${SHARED_PAGE}/new`}
      className="button"
      data-variant="secondary"
    >
      <span className="button-label">
        <Icon glyph={Plus} />
        {t("hostKeys.addShared")}
      </span>
    </Link>
  );
  let body;
  if (keys.isPending) body = <Skeleton label={t("hostKeys.loading")} />;
  else if (keys.isError)
    body = (
      <LoadFailure
        message={t("hostKeys.sharedFailed")}
        onRetry={() => void keys.refetch()}
      />
    );
  else {
    const { items: shared, personal } = keys.data;
    const at = now();
    body = (
      <>
        <div>{add}</div>
        {shared.length === 0 ? (
          <EmptyState
            icon={KeyRound}
            message={t("hostKeys.sharedEmpty")}
            action={null}
          />
        ) : (
          <TableScroll label={t("hostKeys.sharedList")}>
            <table className="data-table" aria-label={t("hostKeys.sharedList")}>
              <thead className="type-label">
                <tr>
                  <th scope="col">{t("hostKeys.name")}</th>
                  <th scope="col">{t("hostKeys.address")}</th>
                  <th scope="col">{t("hostKeys.who")}</th>
                  <th scope="col">{t("hostKeys.lastUsed")}</th>
                </tr>
              </thead>
              <tbody>
                {shared.map((item) => {
                  const use = lastUseWords(item.last_use, t, at, timeZone);
                  return (
                    <tr key={item.id}>
                      <td>
                        <Link href={`${SHARED_PAGE}/${item.id}`}>
                          <code className="type-code">{item.name}</code>
                        </Link>
                      </td>
                      <td>
                        <span className="cell-cut" title={formatAddress(item)}>
                          {formatAddress(item)}
                        </span>
                      </td>
                      <td>
                        <WhoMayUseCell item={item} />
                      </td>
                      <td>
                        <span
                          className="cell-cut"
                          title={[use.text, use.title]
                            .filter(Boolean)
                            .join("\n")}
                        >
                          {use.text}
                        </span>
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </TableScroll>
        )}
        {personal.length > 0 && (
          <TableScroll label={t("hostKeys.personalList")}>
            <table
              className="data-table"
              aria-label={t("hostKeys.personalList")}
            >
              <thead className="type-label">
                <tr>
                  <th scope="col">{t("hostKeys.owner")}</th>
                  <th scope="col">{t("hostKeys.address")}</th>
                  <th scope="col">{t("hostKeys.lastUsed")}</th>
                  <th scope="col">
                    <span className="sr-only">{t("hostKeys.actions")}</span>
                  </th>
                </tr>
              </thead>
              <tbody>
                {personal.map((item) => {
                  // Its time alone: the member's task, and how the try went, are theirs (least exposure).
                  const used = lastUseTime(item.last_use, t, at, timeZone);
                  return (
                    <tr key={item.id}>
                      <td>{item.owner.email}</td>
                      <td>
                        <span className="cell-cut" title={formatAddress(item)}>
                          {formatAddress(item)}
                        </span>
                      </td>
                      <td>{used}</td>
                      <td>
                        <ActionMenu
                          label={t("hostKeys.morePersonal", {
                            person: item.owner.email,
                            address: formatAddress(item),
                          })}
                          actions={[
                            {
                              id: "delete",
                              label: t("hostKeys.delete"),
                              icon: Trash2,
                              danger: true,
                              onSelect: () => {
                                setNotice(null);
                                setDeleting(item.id);
                              },
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
        )}
      </>
    );
  }
  return (
    <section className="stack" aria-labelledby={heading}>
      <h2 id={heading} className="type-body-strong">
        {t("hostKeys.sharedTitle")}
      </h2>
      {notice && (
        <p className="type-small" role="status">
          {notice}
        </p>
      )}
      {body}
      {deletingItem && (
        <DeletePersonal
          item={deletingItem}
          api={api}
          onChanged={() => void keys.refetch()}
          onClose={() => setDeleting(null)}
          onDeleted={(words) => {
            setDeleting(null);
            setNotice(words);
            void keys.refetch();
          }}
        />
      )}
    </section>
  );
}

/** Deleting a member's own key: asks, naming whose it is and that only they can add it again. */
function DeletePersonal({
  item,
  api,
  onChanged,
  onClose,
  onDeleted,
}: {
  item: PersonalKeyRow;
  api: AdminHostKeysApi;
  /** The key changed meanwhile: the list is read again, so deleting again sends its new version. */
  onChanged: () => void;
  onClose: () => void;
  onDeleted: (notice: string) => void;
}) {
  const t = useT();
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<Error | null>(null);
  // Never the member's own name for the key: its owner and web address say which it is (least exposure).
  const words = { person: item.owner.email, address: formatAddress(item) };
  async function remove() {
    setPending(true);
    setError(null);
    try {
      await api.deleteKey(item.id, item.version);
      onDeleted(t("hostKeys.deletedPersonal", words));
    } catch (failure) {
      if (failure instanceof ApiError && failure.code === "conflict")
        onChanged();
      setError(failure instanceof Error ? failure : new Error(String(failure)));
    } finally {
      setPending(false);
    }
  }
  return (
    <ConfirmDialog
      title={t("hostKeys.deletePersonalTitle", words)}
      confirm={t("hostKeys.deleteConfirm")}
      pending={pending}
      error={error}
      errorCopy={(failure) =>
        t(
          failure instanceof ApiError && failure.code === "conflict"
            ? "hostKeys.deleteConflict"
            : "hostKeys.deleteFailed",
        )
      }
      onConfirm={() => void remove()}
      onClose={onClose}
    >
      <p>{t("hostKeys.deletePersonalBody", words)}</p>
    </ConfirmDialog>
  );
}

/**
 * Keys › Shared for members: the shared keys they may use, read-only, with their own last use only; no actions and no
 * one else's names.
 */
export function MemberSharedKeys({
  api,
  now = () => new Date(),
  timeZone,
}: {
  api: HostKeysApi;
  now?: () => Date;
  timeZone?: string;
}) {
  const t = useT();
  const heading = useId();
  const keys = useQuery({
    queryKey: ["host-keys-shared"],
    queryFn: ({ signal }) => api.shared(signal),
    retry: false,
  });
  if (keys.error instanceof ApiError && keys.error.status === 404) return null;
  let body;
  if (keys.isPending) body = <Skeleton label={t("hostKeys.loading")} />;
  else if (keys.isError)
    body = (
      <LoadFailure
        message={t("hostKeys.sharedFailed")}
        onRetry={() => void keys.refetch()}
      />
    );
  else if (keys.data.items.length === 0)
    body = <p className="muted">{t("hostKeys.memberSharedEmpty")}</p>;
  else {
    const at = now();
    body = (
      <TableScroll label={t("hostKeys.memberSharedList")}>
        <table
          className="data-table"
          aria-label={t("hostKeys.memberSharedList")}
        >
          <thead className="type-label">
            <tr>
              <th scope="col">{t("hostKeys.name")}</th>
              <th scope="col">{t("hostKeys.address")}</th>
              <th scope="col">{t("hostKeys.lastUsed")}</th>
            </tr>
          </thead>
          <tbody>
            {keys.data.items.map((item) => {
              const use = lastUseWords(item.last_use, t, at, timeZone);
              return (
                <tr key={item.id}>
                  <td>
                    <code className="type-code">{item.name}</code>
                  </td>
                  <td>
                    <span className="cell-cut" title={formatAddress(item)}>
                      {formatAddress(item)}
                    </span>
                  </td>
                  <td>
                    <span
                      className="cell-cut"
                      title={[use.text, use.title].filter(Boolean).join("\n")}
                    >
                      {use.text}
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
  return (
    <section className="stack" aria-labelledby={heading}>
      <h2 id={heading} className="type-body-strong">
        {t("hostKeys.sharedTitle")}
      </h2>
      {body}
    </section>
  );
}

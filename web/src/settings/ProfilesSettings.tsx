"use client";

import { useQueryClient } from "@tanstack/react-query";
import Link from "next/link";
import { useRouter } from "next/navigation";
import { useEffect, useState } from "react";

import { newActionKey } from "../api/action-key";
import { useT } from "../i18n/I18nProvider";
import type { User } from "../protocol/generated/api";
import { CellNote } from "../ui/CellNote";
import { ConfirmDialog } from "../ui/ConfirmDialog";
import { Skeleton } from "../ui/Skeleton";
import { TableScroll } from "../ui/TableScroll";

import { LoadFailure } from "./LoadFailure";
import { profileFailure, UNUSABLE } from "./profile-copy";
import {
  PROFILE_NOTICE_KEY,
  PROFILES_KEY,
  useProfiles,
  type ChoicesApi,
} from "./profile-queries";
import type { Profile, ProfilesApi } from "./profiles";
import { RowMenu } from "./RowMenu";

export type { ChoicesApi } from "./profile-queries";

const LIST = "/settings/agent-profiles";
const page = (id: string) => `${LIST}/${encodeURIComponent(id)}`;

/**
 * Agent profiles (ADR-0012): your own for everyone, the shared ones for all to use and administrators to write. Add,
 * Edit and Duplicate open the profile's own page (WEB-U-104).
 */
export function ProfilesSettings({
  me,
  api,
  choices,
}: {
  me: User;
  api: ProfilesApi;
  choices: ChoicesApi;
}) {
  const t = useT();
  const cache = useQueryClient();
  const { admin, list, options } = useProfiles(me, api, choices);
  // What a profile's page just did, said here once it sent the reader back.
  const [notice, setNotice] = useState<string | null>(
    () => cache.getQueryData<string>(PROFILE_NOTICE_KEY) ?? null,
  );
  useEffect(() => {
    cache.removeQueries({ queryKey: PROFILE_NOTICE_KEY, exact: true });
  }, [cache]);
  const [removing, setRemoving] = useState<Profile | null>(null);
  const deleted = (copy: string) => {
    setRemoving(null);
    setNotice(copy);
    void cache.invalidateQueries({ queryKey: PROFILES_KEY });
  };
  if (list.isPending || options.isPending)
    return <Skeleton label={t("profiles.loading")} />;
  if (list.isError || options.isError)
    return (
      <LoadFailure
        message={t("profiles.failed")}
        onRetry={() => void (list.isError ? list.refetch() : options.refetch())}
      />
    );
  const own = list.data.items.filter((item) => !item.shared);
  const shared = list.data.items.filter((item) => item.shared);
  const writable = (item: Profile) => !item.shared || admin;
  const none = own.length === 0 && shared.length === 0;
  const add = (
    <Link href={`${LIST}/new`} className="button" data-variant="primary">
      {t("profiles.add")}
    </Link>
  );
  const table = (items: Profile[], label: string, empty: string) => (
    <section className="stack" aria-label={label}>
      <h2 className="type-heading">{label}</h2>
      {items.length === 0 ? (
        <p className="muted">{empty}</p>
      ) : (
        <TableScroll label={label}>
          <table className="data-table profiles-table" aria-label={label}>
            <thead className="type-label">
              <tr>
                <th scope="col">{t("profiles.profile")}</th>
                <th scope="col">{t("profiles.status")}</th>
                <th scope="col">
                  <span className="sr-only">{t("common.actions")}</span>
                </th>
              </tr>
            </thead>
            <tbody>
              {items.map((item) => (
                <ProfileRow
                  key={item.id}
                  item={item}
                  writable={writable(item)}
                  onDelete={() => {
                    setNotice(null);
                    setRemoving(item);
                  }}
                />
              ))}
            </tbody>
          </table>
        </TableScroll>
      )}
    </section>
  );
  return (
    <div className="stack">
      <p className="type-small muted">{t("profiles.intro")}</p>
      {!none && <div className="settings-controls">{add}</div>}
      {notice && (
        <p className="type-small" role="status">
          {notice}
        </p>
      )}
      {/* Nothing on either list: one empty state with the way to add one, not two empty headings. */}
      {none ? (
        <section
          className="empty-state stack"
          aria-label={t("profiles.noneTitle")}
        >
          <p>{t("profiles.none")}</p>
          <div>{add}</div>
        </section>
      ) : (
        <>
          {table(own, t("profiles.yours"), t("profiles.yoursEmpty"))}
          {table(shared, t("profiles.shared"), t("profiles.sharedEmpty"))}
        </>
      )}
      {removing && (
        <DeleteProfile
          item={removing}
          api={api}
          onClose={() => setRemoving(null)}
          onDeleted={() =>
            deleted(t("profiles.deleted", { name: removing.display_name }))
          }
        />
      )}
    </div>
  );
}

function ProfileRow({
  item,
  writable,
  onDelete,
}: {
  item: Profile;
  writable: boolean;
  onDelete: () => void;
}) {
  const t = useT();
  const router = useRouter();
  return (
    <tr>
      <td>
        <div className="stack route-name">
          {/* The display name only: the internal name is not the reader's (as for connectors). */}
          <span>{item.display_name}</span>
          {item.description && <CellNote>{item.description}</CellNote>}
        </div>
      </td>
      <td>
        <div className="stack route-name">
          <span
            className="status-pill type-label"
            data-tone={item.usable ? "ok" : "danger"}
          >
            {t(item.usable ? "profiles.ready" : "profiles.unusable")}
          </span>
          {!item.usable && item.unusable_field && (
            <CellNote>{t(UNUSABLE[item.unusable_field])}</CellNote>
          )}
        </div>
      </td>
      {/* One actions column (UI-GUIDELINES §7, as on Models and Keys): Edit and a menu for a profile you can change;
          a shared one you can only read has its one action, Duplicate. */}
      <td className="data-cell-action">
        {writable ? (
          <>
            <Link
              href={page(item.id)}
              className="button"
              data-variant="ghost"
              aria-label={t("profiles.editName", { name: item.display_name })}
            >
              {t("profiles.edit")}
            </Link>
            <RowMenu
              label={t("profiles.more", { name: item.display_name })}
              actions={[
                ...(item.shared
                  ? [
                      {
                        id: "duplicate",
                        label: t("profiles.duplicate"),
                        name: t("profiles.duplicateName", {
                          name: item.display_name,
                        }),
                        onSelect: () =>
                          router.push(`${page(item.id)}/duplicate`),
                      },
                    ]
                  : []),
                {
                  id: "delete",
                  label: t("profiles.delete"),
                  name: t("profiles.deleteName", { name: item.display_name }),
                  danger: true,
                  onSelect: onDelete,
                },
              ]}
            />
          </>
        ) : (
          item.shared && (
            <Link
              href={`${page(item.id)}/duplicate`}
              className="button"
              data-variant="ghost"
              aria-label={t("profiles.duplicateName", {
                name: item.display_name,
              })}
            >
              {t("profiles.duplicate")}
            </Link>
          )
        )}
      </td>
    </tr>
  );
}

/** Deletes the version read; tasks made from the profile keep their copy (ADR-0012 §5). */
function DeleteProfile({
  item,
  api,
  onDeleted,
  onClose,
}: {
  item: Profile;
  api: ProfilesApi;
  onDeleted: () => void;
  onClose: () => void;
}) {
  const t = useT();
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<Error | null>(null);
  async function run() {
    setPending(true);
    setError(null);
    try {
      await api.deleteProfile(item.id, item.version, newActionKey());
      onDeleted();
    } catch (failure) {
      setError(failure instanceof Error ? failure : new Error("refused"));
    } finally {
      setPending(false);
    }
  }
  return (
    <ConfirmDialog
      title={t("profiles.deleteTitle", { name: item.display_name })}
      confirm={t("profiles.deleteConfirm")}
      pending={pending}
      error={error}
      errorCopy={(refusal) =>
        t(profileFailure(refusal) ?? "profiles.saveFailed")
      }
      onConfirm={() => void run()}
      onClose={onClose}
    >
      <p>{t("profiles.deleteBody")}</p>
    </ConfirmDialog>
  );
}

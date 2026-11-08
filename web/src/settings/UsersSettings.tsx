"use client";

import { useQuery, useQueryClient } from "@tanstack/react-query";
import { UserCheck, UserX } from "lucide-react";
import Link from "next/link";
import { useState, type FormEvent } from "react";

import { api } from "../api/client";
import { useRequest } from "../api/use-request";
import { useT } from "../i18n/I18nProvider";
import type {
  Account,
  CreatedInvitation,
  Invitation,
  User,
} from "../protocol/generated/api";
import { ActionMenu } from "../ui/ActionMenu";
import { Button } from "../ui/Button";
import { ConfirmDialog } from "../ui/ConfirmDialog";
import { ErrorMessage } from "../ui/ErrorMessage";
import { FormRefusal } from "../ui/FormRefusal";
import { Input } from "../ui/Input";
import { MenuButton } from "../ui/MenuButton";
import { Skeleton } from "../ui/Skeleton";
import { TableScroll } from "../ui/TableScroll";

import { accountError } from "./account-errors";
import { DateLabel } from "./DateLabel";
import { settingsApi } from "./requests";
import { SecretDialog } from "./SecretDialog";
import { SECTION_HEADING } from "./SettingsShell";
import { allTasks } from "./team-queries";

const text = (value: FormDataEntryValue | null) =>
  typeof value === "string" ? value : "";
const ROLES = ["member", "admin"] as const;
const INVITATION_TONES = {
  pending: "warn",
  // A plain state, not a check (17.3).
  accepted: "muted",
  expired: "muted",
  revoked: "muted",
} as const;

/**
 * An email that may break only after its @ or a dot, never inside a word (WEB-U-117, the first hour's F19): read whole,
 * on one line wherever the table has room.
 */
function breakable(email: string) {
  return email
    .split(/(?<=[@.])/)
    .flatMap((part, index) =>
      index === 0 ? [part] : [<wbr key={index} />, part],
    );
}

/** User administration (GW-I-046): invite by email, see invitations, change roles, disable or enable accounts. */
export function UsersSettings({
  me,
  clock = () => new Date(),
}: {
  me: User;
  clock?: () => Date;
}) {
  const t = useT();
  if (me.role !== "admin")
    return <p className="muted">{t("users.adminsOnly")}</p>;
  return <Administration me={me} clock={clock} />;
}

/** How long an expired invitation stays listed, so it can be sent again; accepted and revoked ones go at once. */
const EXPIRED_SHOWN_MS = 7 * 86_400_000;

function Administration({ me, clock }: { me: User; clock: () => Date }) {
  const t = useT();
  const cache = useQueryClient();
  const users = useQuery({
    queryKey: ["users"],
    queryFn: ({ signal }) => api.users(signal),
  });
  const invitations = useQuery({
    queryKey: ["invitations"],
    queryFn: ({ signal }) => settingsApi.invitations(signal),
  });
  const tasks = useQuery({
    queryKey: ["all-tasks"],
    queryFn: ({ signal }) => allTasks(signal),
  });
  const invite = useRequest();
  const change = useRequest();
  /** While a change is saved, the rows' Disable and Enable wait and say why (WEB-U-169). */
  const pendingReason = change.pending ? t("users.changePending") : undefined;
  const revoke = useRequest();
  const [created, setCreated] = useState<CreatedInvitation | null>(null);
  const [inviteRole, setInviteRole] = useState<Account["role"]>("member");
  // The email's problem in the page's words, not the browser's bubble (WEB-U-355).
  const [emailProblem, setEmailProblem] = useState<
    "users.emailRequired" | "users.emailInvalid" | null
  >(null);
  const [disabling, setDisabling] = useState<Account | null>(null);
  const [revoking, setRevoking] = useState<Invitation | null>(null);
  const describe = (error: Error | null) =>
    error && (accountError(error, t) ?? error);

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const form = event.currentTarget;
    const data = new FormData(form);
    const email = text(data.get("email")).trim();
    const role = data.get("role") === "admin" ? "admin" : "member";
    const problem = !email
      ? "users.emailRequired"
      : /^[^\s@]+@[^\s@]+$/.test(email)
        ? null
        : "users.emailInvalid";
    setEmailProblem(problem);
    if (problem) {
      form.querySelector<HTMLInputElement>('input[name="email"]')?.focus();
      return;
    }
    const result = await invite.run((signal) =>
      settingsApi.createInvitation({ email, role }, signal),
    );
    if (!result.ok) return;
    form.reset();
    setCreated(result.value);
    await cache.invalidateQueries({ queryKey: ["invitations"] });
  }
  async function update(
    account: Account,
    body: { role: Account["role"] } | { disabled: boolean },
  ) {
    const result = await change.run((signal) =>
      settingsApi.changeUser(account.id, body, signal),
    );
    await cache.invalidateQueries({ queryKey: ["users"] });
    return result.ok;
  }
  async function confirmRevoke(invitation: Invitation) {
    const result = await revoke.run((signal) =>
      settingsApi.revokeInvitation(invitation.id, signal),
    );
    await cache.invalidateQueries({ queryKey: ["invitations"] });
    if (result.ok) setRevoking(null);
  }
  const counts = new Map<string, number>();
  for (const task of tasks.data ?? [])
    counts.set(task.owner_id, (counts.get(task.owner_id) ?? 0) + 1);

  // Pending invitations, and expired ones for a week (adversarial review, Users): the rest is history.
  const listed = (invitations.data?.items ?? []).filter(
    (invitation) =>
      invitation.status === "pending" ||
      (invitation.status === "expired" &&
        clock().getTime() - new Date(invitation.expires_at).getTime() <
          EXPIRED_SHOWN_MS),
  );
  return (
    <div className="settings-section stack people-page">
      {/* Under the section's heading, People (17.3), its one line; the list is named by that heading. */}
      <p className="type-small muted">{t("users.intro")}</p>
      <ErrorMessage error={disabling ? null : describe(change.error)} />
      {users.isPending ? (
        <Skeleton label={t("skeleton.loading")} />
      ) : users.isError ? (
        <div className="stack">
          <p role="alert">{t("users.loadFailed")}</p>
          <Button
            variant="secondary"
            onClick={() => {
              void users.refetch();
            }}
          >
            {t("common.tryAgain")}
          </Button>
        </div>
      ) : (
        <>
          <TableScroll labelledBy={SECTION_HEADING}>
            <table className="data-table" aria-labelledby={SECTION_HEADING}>
              <thead className="type-label">
                <tr>
                  <th scope="col">{t("users.email")}</th>
                  <th scope="col">{t("users.role")}</th>
                  <th scope="col">{t("users.status")}</th>
                  <th scope="col">{t("users.joined")}</th>
                  {/* A count: on the right, in tabular figures (17.3, §7 Table). */}
                  <th scope="col" className="numeric">
                    {t("users.tasks")}
                  </th>
                  <th scope="col">
                    <span className="sr-only">{t("common.actions")}</span>
                  </th>
                </tr>
              </thead>
              <tbody>
                {users.data.items.map((account) => {
                  const own = account.id === me.id;
                  const count = counts.get(account.id) ?? 0;
                  return (
                    <tr key={account.id}>
                      <td>
                        <div
                          className="data-name data-name-wrap"
                          title={account.email}
                        >
                          <span>{breakable(account.email)}</span>
                          {own && (
                            <span className="status-pill type-label">
                              {t("users.you")}
                            </span>
                          )}
                        </div>
                      </td>
                      <td>
                        <MenuButton
                          label={t("users.roleFor", { email: account.email })}
                          options={ROLES.map((role) => ({
                            id: role,
                            label: t("users.roleName", { role }),
                          }))}
                          value={account.role}
                          disabled={change.pending}
                          // Your own role is another administrator's to change (WEB-U-117, the first hour's F19).
                          disabledReason={own ? t("users.ownRole") : undefined}
                          onChange={(role) =>
                            void update(account, {
                              role: role === "admin" ? "admin" : "member",
                            })
                          }
                        />
                      </td>
                      <td>
                        <span
                          className="status-pill type-label"
                          // Active or not is a state, said by its word (17.3).
                          data-tone="muted"
                        >
                          {t("users.accountStatus", {
                            disabled: account.disabled ? "yes" : "no",
                          })}
                        </span>
                      </td>
                      <td>
                        <DateLabel value={account.created_at} />
                      </td>
                      <td className="numeric">
                        {tasks.data && (
                          <Link href={`/settings/people/${account.id}`}>
                            {t("users.taskCount", { count })}
                          </Link>
                        )}
                      </td>
                      <td className="data-cell-action">
                        {/* Disable is a danger item under the row's ⋯ (WEB-U-169); your own row has nothing to do. */}
                        {!own && (
                          <ActionMenu
                            label={t("users.moreName", {
                              email: account.email,
                            })}
                            actions={[
                              account.disabled
                                ? {
                                    id: "enable",
                                    label: t("users.enable"),
                                    name: t("users.enableName", {
                                      email: account.email,
                                    }),
                                    icon: UserCheck,
                                    unavailable: pendingReason,
                                    onSelect: () =>
                                      void update(account, { disabled: false }),
                                  }
                                : {
                                    id: "disable",
                                    label: t("users.disableMenu"),
                                    name: t("users.disableName", {
                                      email: account.email,
                                    }),
                                    icon: UserX,
                                    unavailable: pendingReason,
                                    danger: true,
                                    onSelect: () => setDisabling(account),
                                  },
                            ]}
                          />
                        )}
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </TableScroll>
          {users.data.items.length === 1 && (
            <p className="muted">{t("users.onlyYou")}</p>
          )}
        </>
      )}

      {listed.length > 0 && (
        <>
          <h2 className="type-heading" id="invitation-list">
            {t("users.invitations")}
          </h2>
          <TableScroll labelledBy="invitation-list">
            <table className="data-table" aria-labelledby="invitation-list">
              <thead className="type-label">
                <tr>
                  <th scope="col">{t("users.email")}</th>
                  <th scope="col">{t("users.role")}</th>
                  <th scope="col">{t("users.status")}</th>
                  <th scope="col">{t("tokens.expires")}</th>
                  <th scope="col">
                    <span className="sr-only">{t("common.actions")}</span>
                  </th>
                </tr>
              </thead>
              <tbody>
                {listed.map((invitation) => (
                  <tr key={invitation.id}>
                    <td>
                      <div
                        className="data-name data-name-wrap"
                        title={invitation.email}
                      >
                        <span>{breakable(invitation.email)}</span>
                      </div>
                    </td>
                    <td>{t("users.roleName", { role: invitation.role })}</td>
                    <td>
                      <span
                        className="status-pill type-label"
                        data-tone={INVITATION_TONES[invitation.status]}
                      >
                        {t("users.invitationStatus", {
                          status: invitation.status,
                        })}
                      </span>
                    </td>
                    <td>
                      <DateLabel value={invitation.expires_at} />
                    </td>
                    <td className="data-cell-action">
                      {/* Revoke is a danger item under the row's ⋯, as in every settings table (WEB-U-191). */}
                      {invitation.status === "pending" && (
                        <ActionMenu
                          label={t("users.invitationMore", {
                            email: invitation.email,
                          })}
                          actions={[
                            {
                              id: "revoke",
                              label: t("users.revokeInvitationMenu"),
                              name: t("users.revokeInvitation", {
                                email: invitation.email,
                              }),
                              icon: UserX,
                              danger: true,
                              onSelect: () => setRevoking(invitation),
                            },
                          ]}
                        />
                      )}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </TableScroll>
        </>
      )}

      {/* Invite someone after the people and the pending invitations (the 13f screens, #7, WEB-U-156). */}
      <h2 className="type-heading" id="invite-heading">
        {t("users.inviteTitle")}
      </h2>
      <form
        className="settings-form"
        aria-labelledby="invite-heading"
        noValidate
        onSubmit={(event) => {
          void submit(event);
        }}
      >
        <Input
          label={t("users.email")}
          name="email"
          type="email"
          required
          maxLength={254}
          autoComplete="off"
          onChange={() => setEmailProblem(null)}
          {...(emailProblem ? { error: t(emailProblem) } : {})}
        />
        {/* The standard picker (WEB-U-117); the form reads its choice from the hidden field. */}
        <div className="field stack">
          <span className="type-label muted" aria-hidden="true">
            {t("users.role")}
          </span>
          <MenuButton
            label={t("users.role")}
            options={ROLES.map((role) => ({
              id: role,
              label: t("users.roleName", { role }),
            }))}
            value={inviteRole}
            onChange={(role) =>
              setInviteRole(role === "admin" ? "admin" : "member")
            }
          />
          <input type="hidden" name="role" value={inviteRole} />
        </div>
        <FormRefusal />
        <Button type="submit" variant="primary" loading={invite.pending}>
          {t("users.invite")}
        </Button>
      </form>
      <ErrorMessage error={describe(invite.error)} />

      {created && (
        <SecretDialog
          title={t("users.createdTitle")}
          label={t("users.link")}
          value={`${window.location.origin}/join/${encodeURIComponent(created.token)}`}
          onClose={() => setCreated(null)}
        >
          {t("users.createdHelp", {
            email: created.email,
            date: new Intl.DateTimeFormat(t.locale, {
              dateStyle: "long",
            }).format(new Date(created.expires_at)),
          })}
        </SecretDialog>
      )}
      {disabling && (
        <ConfirmDialog
          title={t("users.disableTitle", { email: disabling.email })}
          confirm={t("users.disableConfirm")}
          pending={change.pending}
          error={change.error}
          errorCopy={(error) => accountError(error, t)}
          onConfirm={() => {
            void update(disabling, { disabled: true }).then((ok) => {
              if (ok) setDisabling(null);
            });
          }}
          onClose={() => setDisabling(null)}
        >
          {t("users.disableBody")}
        </ConfirmDialog>
      )}
      {revoking && (
        <ConfirmDialog
          title={t("users.revokeInvitationTitle")}
          confirm={t("users.revokeInvitationConfirm")}
          pending={revoke.pending}
          error={revoke.error}
          errorCopy={(error) => accountError(error, t)}
          onConfirm={() => {
            void confirmRevoke(revoking);
          }}
          onClose={() => setRevoking(null)}
        >
          {t("users.revokeInvitationBody", { email: revoking.email })}
        </ConfirmDialog>
      )}
    </div>
  );
}

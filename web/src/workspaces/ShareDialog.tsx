"use client";

import "./catalogue";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useState, type FormEvent } from "react";

import { api, ApiError } from "../api/client";
import {
  shareWorkspace,
  shareWorkspaceByEmail,
  unshareWorkspace,
  workspaceShares,
} from "../api/shares";
import { useRequest } from "../api/use-request";
import { useT } from "../i18n/I18nProvider";
import type { Translate } from "../i18n/translate";
import type { Workspace, WorkspaceShare } from "../protocol/generated/api";
import { Button } from "../ui/Button";
import { Dialog } from "../ui/Dialog";
import { ErrorMessage } from "../ui/ErrorMessage";
import { Input } from "../ui/Input";
import { MenuButton } from "../ui/MenuButton";
import { Skeleton } from "../ui/Skeleton";

import { workspaceName } from "./workspace-state";

type Access = WorkspaceShare["access"];

function accessChoices(t: Translate) {
  return [
    {
      id: "write",
      label: t("workspaces.canEdit"),
      detail: (
        <span className="type-small muted">
          {t("workspaces.canEditDetail")}
        </span>
      ),
    },
    {
      id: "read",
      label: t("workspaces.canView"),
      detail: (
        <span className="type-small muted">
          {t("workspaces.canViewDetail")}
        </span>
      ),
    },
  ] as const;
}

/** Share failures the owner can act on, in their words; anything else keeps the generic error copy. */
function shareError(error: Error, t: Translate): string | null {
  if (!(error instanceof ApiError)) return null;
  if (error.code === "user_not_found") return t("workspaces.userNotFound");
  if (error.status === 400) return t("workspaces.emailInvalid");
  return null;
}

/**
 * The owner's share dialog (mockup change 4): an exact email and an access level, never a
 * directory of accounts; the members with the app's picker for their access and a named Remove.
 */
export function ShareDialog({
  workspace,
  onClose,
}: {
  workspace: Workspace;
  onClose: () => void;
}) {
  const t = useT();
  const cache = useQueryClient();
  const key = ["workspace-shares", workspace.id];
  const shares = useQuery({
    queryKey: key,
    queryFn: ({ signal }) => workspaceShares(api, workspace.id, signal),
  });
  const add = useRequest();
  const change = useRequest();
  const [email, setEmail] = useState("");
  // The least access unless the owner chooses more (WEB-U-173).
  const [access, setAccess] = useState<Access>("read");
  // Editing the email dismisses its error until the next attempt.
  const [dismissed, setDismissed] = useState<Error | null>(null);
  const choices = accessChoices(t);
  async function refresh() {
    await Promise.all([
      cache.invalidateQueries({ queryKey: key }),
      cache.invalidateQueries({ queryKey: ["workspaces"] }),
    ]);
  }
  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const address = email.trim();
    if (!address) return;
    const result = await add.run((signal) =>
      shareWorkspaceByEmail(
        api,
        workspace.id,
        { email: address, access },
        signal,
      ),
    );
    if (!result.ok) return;
    setEmail("");
    // The next person starts at the least access again (WEB-U-173).
    setAccess("read");
    await refresh();
  }
  async function setShare(share: WorkspaceShare, next: Access) {
    const result = await change.run((signal) =>
      shareWorkspace(
        api,
        workspace.id,
        share.user_id,
        { access: next },
        signal,
      ),
    );
    if (result.ok) await refresh();
  }
  async function remove(share: WorkspaceShare) {
    const result = await change.run((signal) =>
      unshareWorkspace(api, workspace.id, share.user_id, signal),
    );
    if (result.ok) await refresh();
  }
  const failure = add.error === dismissed ? null : add.error;
  const fieldError = failure
    ? (shareError(failure, t) ?? undefined)
    : undefined;
  return (
    <Dialog
      open
      title={t("workspaces.shareTitle", { name: workspaceName(workspace, t) })}
      onClose={onClose}
      className="share-dialog"
    >
      <p className="muted">{t("workspaces.shareIntro")}</p>
      <form
        className="share-form"
        noValidate
        onSubmit={(event) => {
          void submit(event);
        }}
      >
        <Input
          label={t("workspaces.email")}
          labelHidden
          type="email"
          name="email"
          autoComplete="off"
          placeholder={t("workspaces.email")}
          spellCheck={false}
          value={email}
          {...(fieldError ? { error: fieldError } : {})}
          onChange={(event) => {
            setEmail(event.target.value);
            setDismissed(add.error);
          }}
        />
        <MenuButton
          label={t("workspaces.access")}
          options={choices}
          value={access}
          onChange={(next) => setAccess(next === "read" ? "read" : "write")}
        />
        <Button type="submit" variant="primary" loading={add.pending}>
          {t("workspaces.share")}
        </Button>
      </form>
      {failure && !fieldError && <ErrorMessage error={failure} />}
      <h3 className="type-label muted">{t("workspaces.members")}</h3>
      {shares.isPending ? (
        <Skeleton label={t("workspaces.membersLoading")} lines={2} />
      ) : shares.isError ? (
        <div className="stack">
          <ErrorMessage error={shares.error} />
          <Button
            variant="secondary"
            onClick={() => {
              void shares.refetch();
            }}
          >
            {t("common.tryAgain")}
          </Button>
        </div>
      ) : shares.data.items.length === 0 ? (
        <p className="type-small muted">{t("workspaces.noMembers")}</p>
      ) : (
        <ul className="share-list">
          {shares.data.items.map((share) => (
            <li key={share.user_id} className="share-row">
              <span className="share-email" title={share.email}>
                {share.email}
              </span>
              <MenuButton
                label={t("workspaces.accessFor", { email: share.email })}
                options={choices}
                value={share.access}
                disabled={change.pending}
                onChange={(next) => {
                  void setShare(share, next === "read" ? "read" : "write");
                }}
              />
              <Button
                variant="ghost"
                className="danger-ghost"
                aria-label={t("workspaces.removeName", { email: share.email })}
                disabled={change.pending}
                onClick={() => {
                  void remove(share);
                }}
              >
                {t("workspaces.remove")}
              </Button>
            </li>
          ))}
        </ul>
      )}
      <ErrorMessage error={change.error} />
    </Dialog>
  );
}

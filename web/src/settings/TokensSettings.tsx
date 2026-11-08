"use client";

import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useState, type FormEvent } from "react";

import { useRequest } from "../api/use-request";
import { RelativeTime } from "../conversations/RelativeTime";
import { useT } from "../i18n/I18nProvider";
import { sentenceGap } from "../i18n/sentences";
import type { ApiToken, CreatedToken } from "../protocol/generated/api";
import { Button } from "../ui/Button";
import { ConfirmDialog } from "../ui/ConfirmDialog";
import { ErrorMessage } from "../ui/ErrorMessage";
import { FormRefusal } from "../ui/FormRefusal";
import { Input } from "../ui/Input";
import { Select } from "../ui/Select";
import { Skeleton } from "../ui/Skeleton";
import { TableScroll } from "../ui/TableScroll";

import { accountError } from "./account-errors";
import { DateLabel } from "./DateLabel";
import { settingsApi } from "./requests";
import { SecretDialog } from "./SecretDialog";

const text = (value: FormDataEntryValue | null) =>
  typeof value === "string" ? value : "";
const EXPIRY_DAYS = [7, 30, 90, 180, 365] as const;
const DEFAULT_DAYS = 90;
// The header is inserted as code at this marker; translations never contain it.
const HEADER_MARK = "\u0000";
const BEARER_HEADER = "Authorization: Bearer <token>"; // copy-lint: not user copy

/** Personal API tokens (FR-26): created with a name and expiry, shown once, listed by hint, revoked with a confirm. */
export function TokensSettings({
  clock = () => new Date(),
}: {
  clock?: () => Date;
} = {}) {
  const t = useT();
  const cache = useQueryClient();
  const tokens = useQuery({
    queryKey: ["tokens"],
    queryFn: ({ signal }) => settingsApi.tokens(signal),
  });
  const create = useRequest();
  const revoke = useRequest();
  const [created, setCreated] = useState<CreatedToken | null>(null);
  const [revoking, setRevoking] = useState<ApiToken | null>(null);
  // An empty name is said under the field in the page's words, not the browser's bubble (WEB-U-355).
  const [nameProblem, setNameProblem] = useState(false);
  const [intro, outro] = t("tokens.intro", { header: HEADER_MARK }).split(
    HEADER_MARK,
  );

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const form = event.currentTarget;
    const data = new FormData(form);
    const name = text(data.get("name")).trim();
    const days = Number(data.get("days"));
    if (!name) {
      setNameProblem(true);
      form.querySelector<HTMLInputElement>('input[name="name"]')?.focus();
      return;
    }
    const result = await create.run((signal) =>
      settingsApi.createToken({ name, expires_in_days: days }, signal),
    );
    if (!result.ok) return;
    form.reset();
    setCreated(result.value);
    await cache.invalidateQueries({ queryKey: ["tokens"] });
  }

  async function confirmRevoke(token: ApiToken) {
    const result = await revoke.run((signal) =>
      settingsApi.revokeToken(token.id, signal),
    );
    if (!result.ok) return;
    setRevoking(null);
    await cache.invalidateQueries({ queryKey: ["tokens"] });
  }

  return (
    <div className="settings-section stack">
      <p className="muted">
        {intro}
        <code>{BEARER_HEADER}</code>
        {outro}
      </p>
      <form
        className="settings-form"
        noValidate
        onSubmit={(event) => {
          void submit(event);
        }}
      >
        <Input
          label={t("tokens.name")}
          name="name"
          required
          maxLength={100}
          autoComplete="off"
          onChange={() => setNameProblem(false)}
          {...(nameProblem ? { error: t("tokens.nameRequired") } : {})}
        />
        <Select
          label={t("tokens.expiresIn")}
          name="days"
          defaultValue={String(DEFAULT_DAYS)}
        >
          {EXPIRY_DAYS.map((days) => (
            <option key={days} value={days}>
              {t("tokens.days", { count: days })}
            </option>
          ))}
        </Select>
        <FormRefusal />
        <Button type="submit" variant="primary" loading={create.pending}>
          {t("tokens.create")}
        </Button>
      </form>
      <ErrorMessage
        error={create.error && (accountError(create.error, t) ?? create.error)}
      />
      <h2 className="type-heading" id="token-list">
        {t("tokens.list")}
      </h2>
      {tokens.isPending ? (
        <Skeleton label={t("skeleton.loading")} />
      ) : tokens.isError ? (
        <div className="stack">
          <p role="alert">{t("tokens.loadFailed")}</p>
          <Button
            variant="secondary"
            onClick={() => {
              void tokens.refetch();
            }}
          >
            {t("common.tryAgain")}
          </Button>
        </div>
      ) : tokens.data.items.length === 0 ? (
        <p className="muted">{t("tokens.empty")}</p>
      ) : (
        <TableScroll labelledBy="token-list">
          <table className="data-table" aria-labelledby="token-list">
            <thead className="type-label">
              <tr>
                <th scope="col">{t("tokens.name")}</th>
                <th scope="col">{t("tokens.token")}</th>
                <th scope="col">{t("tokens.created")}</th>
                <th scope="col">{t("tokens.expires")}</th>
                <th scope="col">{t("tokens.lastUsed")}</th>
                <th scope="col">
                  <span className="sr-only">{t("common.actions")}</span>
                </th>
              </tr>
            </thead>
            <tbody>
              {tokens.data.items.map((token) => {
                // An expired token is listed until revoked, greyed and marked.
                const expired =
                  new Date(token.expires_at).getTime() <= clock().getTime();
                return (
                  <tr key={token.id} data-expired={expired || undefined}>
                    <td>
                      <div className="data-name" title={token.name}>
                        <span>{token.name}</span>
                        {expired && (
                          <span
                            className="status-pill type-label"
                            data-tone="muted"
                          >
                            {t("tokens.expired")}
                          </span>
                        )}
                      </div>
                    </td>
                    <td>
                      <code>{`tqa_pat_…${token.hint}`}</code>
                    </td>
                    <td>
                      <RelativeTime value={token.created_at} />
                    </td>
                    <td>
                      <DateLabel value={token.expires_at} />
                    </td>
                    <td>
                      {token.last_used_at ? (
                        <RelativeTime value={token.last_used_at} />
                      ) : (
                        t("tokens.never")
                      )}
                    </td>
                    <td className="data-cell-action">
                      <Button
                        variant="ghost"
                        aria-label={t("tokens.revokeName", {
                          name: token.name,
                        })}
                        onClick={() => setRevoking(token)}
                      >
                        {t("tokens.revokeConfirm")}
                      </Button>
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </TableScroll>
      )}
      {created && (
        <SecretDialog
          title={t("tokens.createdTitle")}
          label={t("tokens.token")}
          value={created.token}
          onClose={() => setCreated(null)}
        >
          {t("tokens.createdOnce")}
          {sentenceGap(t.locale)}
          {t("tokens.createdExpires", {
            date: new Intl.DateTimeFormat(t.locale, {
              dateStyle: "long",
            }).format(new Date(created.expires_at)),
          })}
        </SecretDialog>
      )}
      {revoking && (
        <ConfirmDialog
          title={t("tokens.revokeTitle")}
          confirm={t("tokens.revokeConfirm")}
          pending={revoke.pending}
          error={revoke.error}
          onConfirm={() => {
            void confirmRevoke(revoking);
          }}
          onClose={() => setRevoking(null)}
        >
          {t("tokens.revokeBody", { name: revoking.name })}
        </ConfirmDialog>
      )}
    </div>
  );
}

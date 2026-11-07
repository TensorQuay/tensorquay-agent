"use client";

import "../auth/catalogue";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import Link from "next/link";
import { useRouter } from "next/navigation";
import { useState, type FormEvent } from "react";

import { api, ApiError } from "../api/client";
import { useRequest } from "../api/use-request";
import { passwordProblem, type FieldProblem } from "../auth/field-problems";
import { PasswordField } from "../auth/PasswordField";
import { useT } from "../i18n/I18nProvider";
import { APP_NAME } from "../lib/app";
import { Button } from "../ui/Button";
import { ErrorMessage } from "../ui/ErrorMessage";
import { Input } from "../ui/Input";
import { Skeleton } from "../ui/Skeleton";

import { lookupInvitation } from "./lookup";

const unusable = (error: Error | null) =>
  error instanceof ApiError && error.code === "invitation_invalid";

/**
 * Joins by an email-bound invitation link (GW-I-002). The gateway answers every unknown, used, expired or revoked link
 * the same way, so the page has one plain state for all of them.
 */
export function JoinForm({ token }: { token: string }) {
  const t = useT();
  const router = useRouter();
  const cache = useQueryClient();
  const request = useRequest();
  const [password, setPassword] = useState("");
  // A short or empty password in the page's words, before anything is sent (WEB-U-355).
  const [problem, setProblem] = useState<FieldProblem | null>(null);
  const invitee = useQuery({
    queryKey: ["invitation", token],
    queryFn: ({ signal }) => lookupInvitation(token, signal),
    retry: false,
    staleTime: Infinity,
  });
  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const found = passwordProblem(password, true);
    setProblem(found);
    if (found) {
      event.currentTarget
        .querySelector<HTMLInputElement>('input[name="password"]')
        ?.focus();
      return;
    }
    const result = await request.run((signal) =>
      api.acceptInvitation({ token, password }, signal),
    );
    if (!result.ok) return;
    setPassword("");
    cache.clear();
    cache.setQueryData(["session"], result.value);
    router.replace("/");
  }
  const invalid = unusable(invitee.error) || unusable(request.error);
  return (
    <section className="auth-column">
      <title>{t("join.tab", { product: APP_NAME })}</title>
      <span
        className="brand-wordmark"
        role="img"
        aria-label={t("auth.wordmark")}
      />
      <header className="stack auth-heading">
        <h1 className="type-display">
          {t("join.title", { product: APP_NAME })}
        </h1>
        {invitee.data && !invalid && (
          <p className="type-small muted">
            {t("join.intro", { role: invitee.data.role })}
          </p>
        )}
      </header>
      {invalid ? (
        <div className="stack">
          <p>{t("join.invalid")}</p>
          <Link href="/login">{t("join.signIn")}</Link>
        </div>
      ) : invitee.isError ? (
        <ErrorMessage error={invitee.error} />
      ) : !invitee.data ? (
        <Skeleton label={t("join.loading")} />
      ) : (
        <form
          noValidate
          onSubmit={(event) => {
            void submit(event);
          }}
        >
          <Input
            label={t("auth.email")}
            type="email"
            name="email"
            autoComplete="username"
            readOnly
            value={invitee.data.email}
          />
          <PasswordField
            value={password}
            onChange={(value) => {
              setPassword(value);
              setProblem(null);
            }}
            error={problem ? t(problem) : undefined}
            email={invitee.data.email}
            setup
            disabled={request.pending}
          />
          <ErrorMessage error={request.error} />
          <Button type="submit" variant="primary" loading={request.pending}>
            {t("join.submit")}
          </Button>
        </form>
      )}
    </section>
  );
}

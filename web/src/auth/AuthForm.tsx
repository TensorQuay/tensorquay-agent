"use client";

import "./catalogue";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useRouter } from "next/navigation";
import { useEffect, useRef, useState, type FormEvent } from "react";

import { api, ApiError } from "../api/client";
import { useRequest } from "../api/use-request";
import { useT } from "../i18n/I18nProvider";
import { APP_NAME } from "../lib/app";
import { Button } from "../ui/Button";
import { ErrorMessage } from "../ui/ErrorMessage";
import { Input } from "../ui/Input";

import {
  emailProblem,
  passwordProblem,
  type FieldProblem,
} from "./field-problems";
import { PasswordField } from "./PasswordField";
import { returnPath } from "./return-path";
import { setupState } from "./setup-state";

/** Keeps credentials in the form, then uses the session established by either successful auth action. */
/**
 * The server's refusals a person fixes in one field: a wrong setup token, or a wrong email or password (said on the
 * password). They are said under that field in web-owned copy, never the gateway's diagnostic (WEB-U-380).
 */
type Refusal = "auth.wrongSetupToken" | "auth.wrongCredentials";
function refusal(
  setup: boolean,
  error: unknown,
): { field: "token" | "password"; problem: Refusal } | null {
  if (!(error instanceof ApiError)) return null;
  if (setup && error.code === "unauthorized")
    return { field: "token", problem: "auth.wrongSetupToken" };
  if (!setup && error.status === 401)
    return { field: "password", problem: "auth.wrongCredentials" };
  return null;
}

// A marker the translated help text never contains; the variable name is inserted as code at its place.
const VARIABLE_MARK = "\u0000";
const BOOTSTRAP_VARIABLE = "TQA_BOOTSTRAP_TOKEN"; // copy-lint: not user copy

export function AuthForm({ mode }: { mode: "login" | "setup" }) {
  const router = useRouter();
  const query = useQueryClient();
  const request = useRequest();
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  // Each field's problem in the page's words, before anything is sent (WEB-U-355).
  const [problems, setProblems] = useState<{
    email?: FieldProblem | null;
    password?: FieldProblem | Refusal | null;
    token?: FieldProblem | Refusal | null;
  }>({});
  // The field a refusal is about; it gets focus once the form is enabled again (the request disables it, and the
  // browser then drops focus to the page: the 16f page pass, F-P01-1/F-P02-1).
  const refocus = useRef<"token" | "password" | null>(null);
  const formRef = useRef<HTMLFormElement>(null);
  const setup = mode === "setup";
  const state = useQuery({
    queryKey: ["setup-state"],
    queryFn: ({ signal }) => setupState(signal),
    enabled: !setup,
    retry: false,
    staleTime: 0,
  });
  useEffect(() => {
    if (!setup && state.data?.required) router.replace("/setup");
  }, [router, setup, state.data?.required]);
  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const form = event.currentTarget;
    // The token's field is read where it is: FormData is the request's own step, after these checks.
    const token = form.querySelector<HTMLInputElement>(
      'input[name="token"]',
    )?.value;
    const found = {
      email: emailProblem(email),
      password: passwordProblem(password, setup),
      token:
        setup && !token?.trim() ? ("auth.setupTokenRequired" as const) : null,
    };
    setProblems(found);
    const first = (["email", "password", "token"] as const).find(
      (field) => found[field],
    );
    if (first) {
      form.querySelector<HTMLInputElement>(`input[name="${first}"]`)?.focus();
      return;
    }
    // Set inside the request when the server refuses it in a way a field fixes.
    const outcome: { refused: ReturnType<typeof refusal> } = { refused: null };
    const result = await request.run(async (signal) => {
      const data = new FormData(form);
      const field = (name: string) => {
        const value = data.get(name);
        return typeof value === "string" ? value : "";
      };
      try {
        return await (setup
          ? api.bootstrap(
              {
                email: field("email"),
                password: field("password"),
                token: field("token"),
              },
              signal,
            )
          : api.login(
              { email: field("email"), password: field("password") },
              signal,
            ));
      } catch (cause) {
        outcome.refused = refusal(setup, cause);
        throw cause;
      }
    });
    if (!result.ok) {
      const found = outcome.refused;
      if (found) {
        setProblems((current) => ({
          ...current,
          [found.field]: found.problem,
        }));
        refocus.current = found.field;
      }
      return;
    }
    form.reset();
    setEmail("");
    setPassword("");
    query.clear();
    query.setQueryData(["session"], result.value);
    router.replace(
      setup
        ? "/"
        : returnPath(new URLSearchParams(window.location.search).get("next")),
    );
  }
  useEffect(() => {
    if (!refocus.current || request.pending) return;
    formRef.current
      ?.querySelector<HTMLInputElement>(`input[name="${refocus.current}"]`)
      ?.focus();
    refocus.current = null;
  });
  const t = useT();
  // A refusal is said under its field instead (above), so the form's own message is for everything else.
  const error = refusal(setup, request.error) ? null : request.error;
  // The variable name stays code; the sentence around it is translated.
  const [helpBefore, helpAfter] = t("auth.setupTokenHelp", {
    variable: VARIABLE_MARK,
  }).split(VARIABLE_MARK);
  return (
    <section className="auth-column">
      <title>
        {t(setup ? "auth.setupTab" : "auth.signInTab", { product: APP_NAME })}
      </title>
      <span
        className="brand-wordmark"
        role="img"
        aria-label={t("auth.wordmark")}
      />
      <header className="stack auth-heading">
        <h1 className="type-display">
          {t(setup ? "auth.setupTitle" : "auth.signInTitle")}
        </h1>
        <p className="type-small muted">
          {t(setup ? "auth.setupIntro" : "auth.signInIntro")}
        </p>
      </header>
      <form
        ref={formRef}
        noValidate
        onSubmit={(event) => {
          void submit(event);
        }}
      >
        <Input
          label={t("auth.email")}
          type="email"
          name="email"
          autoComplete="email"
          required
          maxLength={254}
          disabled={request.pending}
          value={email}
          onChange={(event) => {
            setEmail(event.target.value);
            // A wrong email or password is said on the password; a new email makes that stale too.
            setProblems((current) => ({
              ...current,
              email: null,
              password:
                current.password === "auth.wrongCredentials"
                  ? null
                  : (current.password ?? null),
            }));
          }}
          {...(problems.email ? { error: t(problems.email) } : {})}
        />
        <PasswordField
          value={password}
          onChange={(value) => {
            setPassword(value);
            setProblems((current) => ({ ...current, password: null }));
          }}
          error={problems.password ? t(problems.password) : undefined}
          email={email}
          setup={setup}
          disabled={request.pending}
        />
        {setup && (
          <div className="stack">
            <Input
              label={t("auth.setupToken")}
              type="password"
              name="token"
              autoComplete="off"
              required
              maxLength={1024}
              disabled={request.pending}
              aria-describedby="setup-help"
              onChange={() =>
                setProblems((current) => ({ ...current, token: null }))
              }
              {...(problems.token ? { error: t(problems.token) } : {})}
            />
            <p className="type-small muted" id="setup-help">
              {helpBefore}
              <code>{BOOTSTRAP_VARIABLE}</code>
              {helpAfter}
            </p>
          </div>
        )}
        <ErrorMessage error={error ?? state.error} />
        {state.isError && (
          <Button
            variant="secondary"
            onClick={() => {
              void state.refetch();
            }}
          >
            {t("auth.checkSetup")}
          </Button>
        )}
        <Button type="submit" loading={request.pending}>
          {t(setup ? "auth.createAdministrator" : "auth.signIn")}
        </Button>
      </form>
    </section>
  );
}

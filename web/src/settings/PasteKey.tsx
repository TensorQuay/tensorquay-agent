"use client";

import { useEffect, useId, useState, type KeyboardEvent } from "react";

import { newActionKey } from "../api/action-key";
import { ApiError } from "../api/error";
import { useT } from "../i18n/I18nProvider";
import type { MessageKey } from "../i18n/translate";
import { Button } from "../ui/Button";
import { Input } from "../ui/Input";

import type { RoutesApi } from "./admin/routes";
import { fieldProblems, reasonCopy, secretProblems } from "./secret-copy";

type Problem = { field: "name" | "value" | null; copy: MessageKey };

/** The secret name Paste key suggests for a route: its name with `-key`, in the characters a secret name allows. */
export function suggestedSecretName(route: string): string {
  const base = route.trim().replace(/[^A-Za-z0-9_-]/g, "-") || "route";
  return `${base}-key`;
}

/**
 * Stores a pasted key as a new shared secret and hands back its reference (WEB-U-056). The write is create-only: a name
 * already taken is a conflict and nothing is overwritten. The value leaves state as soon as it is sent.
 */
export function PasteKey({
  api,
  route,
  open: startOpen,
  onStored,
}: {
  api: RoutesApi;
  route: string;
  open: boolean;
  /** `allowed` is false when the secret was stored but could not yet be allowed for the route (vault slice 3). */
  onStored: (ref: string, allowed: boolean) => void;
}) {
  const t = useT();
  const [open, setOpen] = useState(startOpen);
  const [name, setName] = useState(() => suggestedSecretName(route));
  const [value, setValue] = useState("");
  const [problem, setProblem] = useState<Problem | null>(null);
  const [conflict, setConflict] = useState<string | null>(null);
  const [pending, setPending] = useState(false);
  const keyId = useId();
  // A frame later: opened with the route form (Key needed), the dialog's showModal would otherwise take focus.
  useEffect(() => {
    if (!open) return;
    const frame = requestAnimationFrame(() =>
      document.getElementById(keyId)?.focus(),
    );
    return () => cancelAnimationFrame(frame);
  }, [open, keyId]);

  async function store() {
    const write = { name: name.trim(), value };
    setProblem(null);
    setConflict(null);
    const local = secretProblems(write, true)[0];
    if (local) {
      setProblem({
        field: local.field === "value" ? "value" : "name",
        copy: reasonCopy(local.reason),
      });
      return;
    }
    setPending(true);
    try {
      const ref = await api.createSecret(
        write.name,
        write.value,
        newActionKey(),
      );
      setValue("");
      setOpen(false);
      // A new secret may be used only by what it is bound to: bind this route at once (ADR-0033).
      const allowed = await api.allowSecret(ref, route.trim()).then(
        () => true,
        () => false,
      );
      onStored(ref, allowed);
    } catch (error) {
      const detail = fieldProblems(error)[0];
      if (error instanceof ApiError && error.code === "conflict")
        setConflict(write.name);
      else if (detail)
        setProblem({
          field: detail.field === "value" ? "value" : "name",
          copy: reasonCopy(detail.reason),
        });
      else setProblem({ field: null, copy: "secrets.saveFailed" });
    } finally {
      setPending(false);
    }
  }
  // Enter stores the key here; it never submits the route form around it.
  const onKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.key !== "Enter") return;
    event.preventDefault();
    if (!pending) void store();
  };

  if (!open)
    return (
      // A secondary button at its own width, as every other in the form, never the field's whole width (WEB-E2E-099).
      <div className="panel-actions">
        <Button
          variant="secondary"
          type="button"
          onClick={() => {
            setName(suggestedSecretName(route));
            setOpen(true);
          }}
        >
          {t("models.pasteKey")}
        </Button>
      </div>
    );
  return (
    // An inset panel of its own, its buttons inside it: Save stays the page's one primary action.
    <fieldset className="paste-key-panel stack">
      <legend className="type-label">{t("models.pasteTitle")}</legend>
      <Input
        label={t("models.secretName")}
        value={name}
        onChange={(event) => setName(event.target.value)}
        onKeyDown={onKeyDown}
        autoComplete="off"
        spellCheck={false}
        {...(problem?.field === "name" ? { error: t(problem.copy) } : {})}
      />
      <Input
        id={keyId}
        label={t("models.keyValue")}
        type="password"
        value={value}
        onChange={(event) => setValue(event.target.value)}
        onKeyDown={onKeyDown}
        autoComplete="off"
        spellCheck={false}
        {...(problem?.field === "value" ? { error: t(problem.copy) } : {})}
      />
      {conflict && (
        <p className="field-error" role="alert">
          {t("models.secretExists", { name: conflict })}
        </p>
      )}
      {problem?.field === null && (
        <p className="field-error" role="alert">
          {t(problem.copy)}
        </p>
      )}
      <div className="panel-actions">
        <Button
          variant="secondary"
          type="button"
          loading={pending}
          onClick={() => void store()}
        >
          {t("models.storeKey")}
        </Button>
        <Button
          variant="ghost"
          type="button"
          disabled={pending}
          onClick={() => {
            setValue("");
            setOpen(false);
          }}
        >
          {t("common.cancel")}
        </Button>
      </div>
    </fieldset>
  );
}

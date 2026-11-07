"use client";

import { useState, type FormEvent } from "react";

import { newActionKey } from "../api/action-key";
import { ApiError } from "../api/error";
import { useT } from "../i18n/I18nProvider";
import type { MessageKey } from "../i18n/translate";
import { Button } from "../ui/Button";
import { Dialog } from "../ui/Dialog";
import { FormRefusal } from "../ui/FormRefusal";
import { Input } from "../ui/Input";

import type { EgressApi, EgressRule } from "./admin/egress";
import { parsePorts, ruleFailure, ruleProblems } from "./egress-copy";
import { isIpAddress } from "./host-keys/address";

/** Only a rule for one exact host name may be an internal server's: never a wildcard or an address (ADR-0035's
 * amendment), which the gateway refuses as `internal_name_only`. */
export const canBeInternal = (host: string) => {
  const name = host.trim().toLowerCase();
  return name !== "" && !name.includes("*") && !isIpAddress(name);
};

type FormProps = {
  item: EgressRule | null;
  api: EgressApi;
  limit: number;
  onSaved: (item: EgressRule) => void;
  onStale: (host: string) => Promise<EgressRule | undefined>;
  onClose: () => void;
  /** The host and ports to start from: a new rule's, or a rule's with a port added (Internet access opened for a host,
   * WEB-U-150). */
  start?: { host: string; ports: string } | undefined;
};

/** The typed host and ports and their save: the ports are checked first, then the gateway's problems; a conflict keeps the edits. */
function useRuleSave({ item, api, onSaved, onStale, start }: FormProps) {
  const [host, setHost] = useState(item?.host ?? start?.host ?? "");
  // A start's ports win over a rule's own: opened for a port the rule lacks, it is added (WEB-U-150). A new site starts
  // at 443, the port most sites need (WEB-U-421).
  const [ports, setPorts] = useState(
    start?.ports ?? item?.ports.join(", ") ?? "443",
  );
  const [internal, setInternal] = useState(item?.internal ?? false);
  const [version, setVersion] = useState(item?.version);
  const [problems, setProblems] = useState<Record<string, MessageKey>>({});
  const [failure, setFailure] = useState<MessageKey | null>(null);
  const [pending, setPending] = useState(false);
  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const parsed = parsePorts(ports);
    setFailure(null);
    setProblems(parsed ? {} : { ports: "egress.portsInvalid" });
    if (!parsed) return;
    setPending(true);
    try {
      onSaved(
        await api.putRule(
          host.trim(),
          // A choice made for a name stays off once the host can't take it.
          { ports: parsed, internal: internal && canBeInternal(host) },
          version,
          newActionKey(),
        ),
      );
    } catch (error) {
      setProblems(ruleProblems(error));
      setFailure(ruleFailure(error));
      if (error instanceof ApiError && error.code === "conflict")
        setVersion((await onStale(host.trim()))?.version);
    } finally {
      setPending(false);
    }
  }
  return {
    host,
    setHost,
    ports,
    setPorts,
    internal,
    setInternal,
    problems,
    failure,
    pending,
    submit,
  };
}

/** Adds a rule (host and ports) or edits one rule's ports; a replace sends the version read (ADR-0028). */
export function RuleForm(props: FormProps) {
  const { item, limit, onClose } = props;
  const t = useT();
  const {
    host,
    setHost,
    ports,
    setPorts,
    internal,
    setInternal,
    problems,
    failure,
    pending,
    submit,
  } = useRuleSave(props);
  const internalAllowed = canBeInternal(host);
  const problem = (field: string) =>
    problems[field] ? { error: t(problems[field]) } : {};
  return (
    <Dialog
      open
      title={
        item ? t("egress.editTitle", { host: item.host }) : t("egress.addTitle")
      }
      onClose={() => {
        if (!pending) onClose();
      }}
    >
      <form className="stack" onSubmit={(event) => void submit(event)}>
        {item === null && (
          <Input
            label={t("egress.host")}
            value={host}
            onChange={(event) => setHost(event.target.value)}
            autoComplete="off"
            spellCheck={false}
            {...problem("host")}
          />
        )}
        <Input
          label={t("egress.ports")}
          value={ports}
          onChange={(event) => setPorts(event.target.value)}
          inputMode="numeric"
          autoComplete="off"
          aria-describedby="egress-ports-hint"
          {...problem("ports")}
        />
        <p id="egress-ports-hint" className="type-small muted">
          {t("egress.portsHint")}
        </p>
        <label className="check-row">
          <input
            type="checkbox"
            checked={internal && internalAllowed}
            disabled={!internalAllowed}
            onChange={(event) => setInternal(event.target.checked)}
            aria-describedby="egress-internal-hint"
          />
          {t("egress.internal")}
        </label>
        <p
          id="egress-internal-hint"
          className={problems.internal ? "field-error" : "type-small muted"}
        >
          {t(
            problems.internal ??
              (internalAllowed
                ? "egress.internalHint"
                : "egress.internalNameOnly"),
          )}
        </p>
        {failure && <p className="field-error">{t(failure, { limit })}</p>}
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
            {t("egress.save")}
          </Button>
        </div>
      </form>
    </Dialog>
  );
}

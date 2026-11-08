"use client";

import { useQuery } from "@tanstack/react-query";
import { useState, type FormEvent } from "react";

import { ApiError } from "../api/error";
import { useT } from "../i18n/I18nProvider";
import type { MessageKey, Translate } from "../i18n/translate";
import type { SecretConsumer } from "../protocol/generated/api";
import { Button } from "../ui/Button";
import { CheckRow } from "../ui/CheckRow";
import { Dialog } from "../ui/Dialog";
import { FormRefusal } from "../ui/FormRefusal";
import { Skeleton } from "../ui/Skeleton";

import type { AdminApi, SecretItem } from "./admin/contract";
import { LoadFailure } from "./LoadFailure";
import { keyName } from "./secret-copy";

export const id = (consumer: SecretConsumer) =>
  `${consumer.kind}/${consumer.name}`;
export const byKindAndName = (a: SecretConsumer, b: SecretConsumer) =>
  id(a).localeCompare(id(b));

const CONSUMER_KINDS: Readonly<Record<SecretConsumer["kind"], MessageKey>> = {
  route: "secrets.consumerRoute",
  connector: "secrets.consumerConnector",
  engine: "secrets.consumerEngine",
};

/** A route, connector or engine as people read it: "Route local", "Connector tracker", "Decision engine router". */
export function consumerLabel(consumer: SecretConsumer, t: Translate): string {
  return t(CONSUMER_KINDS[consumer.kind], { name: consumer.name });
}

/**
 * Changes which routes and connectors may use one secret, with the version the page read (WEB-U-054).
 * A bound route or connector that no longer exists stays listed, ticked, until someone unticks it.
 */
export function SecretBindings({
  item,
  api,
  onDone,
  onStale,
  onClose,
}: {
  item: SecretItem;
  api: AdminApi;
  onDone: () => void;
  /** Refreshes the list and resolves to the current item, after another administrator changed it. */
  onStale: () => Promise<SecretItem | undefined>;
  onClose: () => void;
}) {
  const t = useT();
  const choices = useQuery({
    queryKey: ["admin-secret-consumer-choices"],
    queryFn: ({ signal }) => api.consumerChoices(signal),
    retry: false,
  });
  const [version, setVersion] = useState(item.version);
  const [chosen, setChosen] = useState(
    () => new Set((item.consumers ?? []).map(id)),
  );
  const [failure, setFailure] = useState<MessageKey | null>(null);
  const [pending, setPending] = useState(false);
  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const consumers = [...listed(), ...gone()]
      .filter((consumer) => chosen.has(id(consumer)))
      .sort(byKindAndName);
    setFailure(null);
    setPending(true);
    try {
      await api.setConsumers(
        item.ref.slice("secrets/".length),
        consumers,
        version,
      );
      onDone();
    } catch (error) {
      const refused = error instanceof ApiError ? error : null;
      if (refused?.code === "conflict") {
        setFailure("secrets.reminderConflict");
        const current = await onStale();
        if (current) setVersion(current.version);
      } else
        setFailure(
          refused?.status === 400
            ? "secrets.consumersInvalid"
            : refused?.code === "not_found"
              ? "secrets.gone"
              : "secrets.saveFailed",
        );
    } finally {
      setPending(false);
    }
  }
  const listed = () => choices.data ?? [];
  const gone = () =>
    (item.consumers ?? []).filter(
      (consumer) => !listed().some((choice) => id(choice) === id(consumer)),
    );
  const toggle = (key: string) =>
    setChosen((current) => {
      const next = new Set(current);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });
  let body;
  if (choices.isPending)
    body = <Skeleton label={t("secrets.choicesLoading")} />;
  else if (choices.isError)
    body = (
      <LoadFailure
        message={t("secrets.choicesFailed")}
        onRetry={() => void choices.refetch()}
      />
    );
  else
    body = (
      <ConsumerChoices
        legend={t("secrets.bindingsIntro")}
        choices={listed()}
        gone={gone()}
        chosen={chosen}
        onToggle={toggle}
      />
    );
  return (
    <Dialog
      open
      title={t("secrets.bindingsTitle", { name: keyName(item.ref) })}
      onClose={() => {
        if (!pending) onClose();
      }}
    >
      <form className="stack" onSubmit={(event) => void submit(event)}>
        {/* The one place the page states the reference: what configuration files name it by. */}
        <p className="type-small muted">
          {t("secrets.reference", { ref: item.ref })}
        </p>
        {body}
        {failure && <p className="field-error">{t(failure)}</p>}
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
          <Button type="submit" loading={pending} disabled={!choices.isSuccess}>
            {t("secrets.bindingsSave")}
          </Button>
        </div>
      </form>
    </Dialog>
  );
}

const GROUPS: readonly [SecretConsumer["kind"], MessageKey][] = [
  ["route", "secrets.groupRoutes"],
  ["connector", "secrets.groupConnectors"],
  ["engine", "secrets.groupEngines"],
];

/**
 * The routes, connectors and engines a key may be used for, as boxes grouped by kind: the group names the kind, so each
 * box reads the plain name. One no longer there stays listed, ticked, until unticked.
 */
export function ConsumerChoices({
  legend,
  choices,
  gone = [],
  chosen,
  onToggle,
}: {
  legend: string;
  choices: readonly SecretConsumer[];
  gone?: readonly SecretConsumer[];
  chosen: ReadonlySet<string>;
  onToggle: (key: string) => void;
}) {
  const t = useT();
  const all = [
    ...choices.map((consumer) => ({ consumer, gone: false })),
    ...gone.map((consumer) => ({ consumer, gone: true })),
  ].sort((a, b) => byKindAndName(a.consumer, b.consumer));
  return (
    <fieldset className="stack">
      <legend className="type-small muted">{legend}</legend>
      {GROUPS.map(([kind, label]) => {
        const items = all.filter((entry) => entry.consumer.kind === kind);
        if (items.length === 0) return null;
        return (
          <fieldset key={kind} className="stack consumer-group">
            <legend className="type-label">{t(label)}</legend>
            {items.map(({ consumer, gone: missing }) => (
              <Choice
                key={id(consumer)}
                label={
                  missing
                    ? t("secrets.consumerGone", { consumer: consumer.name })
                    : consumer.name
                }
                checked={chosen.has(id(consumer))}
                onChange={() => onToggle(id(consumer))}
              />
            ))}
          </fieldset>
        );
      })}
    </fieldset>
  );
}

function Choice({
  label,
  checked,
  onChange,
}: {
  label: string;
  checked: boolean;
  onChange: () => void;
}) {
  return (
    <CheckRow
      className="type-small"
      checked={checked}
      onChange={onChange}
      label={label}
    />
  );
}

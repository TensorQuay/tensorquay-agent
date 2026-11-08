"use client";

import { useQuery } from "@tanstack/react-query";
import { useState, type FormEvent, type ReactNode } from "react";

import { newActionKey } from "../api/action-key";
import { ApiError } from "../api/error";
import { useT } from "../i18n/I18nProvider";
import type { MessageKey } from "../i18n/translate";
import { Button } from "../ui/Button";
import { CheckRow } from "../ui/CheckRow";
import { Dialog } from "../ui/Dialog";
import { FormRefusal } from "../ui/FormRefusal";
import { Input } from "../ui/Input";
import { Select } from "../ui/Select";
import { Textarea } from "../ui/Textarea";

import type { Connector, ConnectorsApi } from "./admin/connectors";
import { AllowSecret } from "./AllowSecret";
import {
  connectorFailure,
  connectorProblems,
  fromDraft,
  SCHEMES,
  toDraft,
  type ConnectorDraft,
} from "./connector-copy";
import { keyName } from "./secret-copy";

type FormProps = {
  item: Connector | null;
  api: ConnectorsApi;
  onSaved: (item: Connector) => void;
  onStale: (name: string) => Promise<Connector | undefined>;
  onClose: () => void;
};
type Section = {
  draft: ConnectorDraft;
  set: (change: Partial<ConnectorDraft>) => void;
  problems: Record<string, MessageKey>;
};

/** The draft and its save: the page's checks first, then the gateway's; a conflict keeps the edits (ADR-0028). */
function useConnectorSave({ item, api, onSaved, onStale }: FormProps) {
  const [draft, setDraft] = useState(() => toDraft(item));
  const [version, setVersion] = useState(item?.version);
  const [problems, setProblems] = useState<Record<string, MessageKey>>({});
  const [failure, setFailure] = useState<MessageKey | null>(null);
  const [pending, setPending] = useState(false);
  /** The secret a save named but may not use yet (vault slice 3: 409 secret_not_allowed). */
  const [notAllowed, setNotAllowed] = useState<string | null>(null);
  const set = (change: Partial<ConnectorDraft>) =>
    setDraft((current) => ({ ...current, ...change }));
  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const { fields, problems: found } = fromDraft(draft, item === null);
    setFailure(null);
    setNotAllowed(null);
    setProblems(found);
    if (Object.keys(found).length > 0) return;
    const name = item?.name ?? draft.name.trim();
    setPending(true);
    try {
      onSaved(await api.putConnector(name, fields, version, newActionKey()));
    } catch (error) {
      // No details come with it: the secret is the one this form names.
      if (
        error instanceof ApiError &&
        error.code === "secret_not_allowed" &&
        fields.secret_ref
      ) {
        setNotAllowed(fields.secret_ref);
        return;
      }
      setProblems(connectorProblems(error));
      setFailure(connectorFailure(error));
      if (error instanceof ApiError && error.code === "conflict")
        setVersion((await onStale(name))?.version);
    } finally {
      setPending(false);
    }
  }
  return {
    section: { draft, set, problems },
    failure,
    pending,
    submit,
    notAllowed,
    name: item?.name ?? draft.name.trim(),
  };
}

/** Adds a connector or edits one; its credential is a secret named here, never a value (ADR-0031). */
export function ConnectorForm(props: FormProps) {
  const { item, api, onClose } = props;
  const t = useT();
  const { section, failure, pending, submit, notAllowed, name } =
    useConnectorSave(props);
  const secrets = useQuery({
    queryKey: ["admin-secret-refs"],
    queryFn: ({ signal }) => api.secretRefs(signal),
    retry: false,
  });
  return (
    <Dialog
      open
      title={
        item
          ? t("connectors.editTitle", { name: item.display_name || item.name })
          : t("connectors.addTitle")
      }
      onClose={() => {
        if (!pending) onClose();
      }}
    >
      <form
        className="stack"
        noValidate
        onSubmit={(event) => void submit(event)}
      >
        <Basics section={section} creating={item === null} />
        <Credential section={section} secrets={secrets.data ?? []} />
        <Tools section={section} />
        <Behaviour section={section} />
        {(item?.insecure_http || item?.ca_file) && (
          <p className="type-small muted" role="note">
            {t("connectors.fileSettings")}
          </p>
        )}
        {failure && <p className="field-error">{t(failure)}</p>}
        {notAllowed && (
          <AllowSecret
            key={notAllowed}
            secret={notAllowed}
            consumer={name}
            copy={{
              notAllowed: "connectors.notAllowed",
              allowFor: "connectors.allowFor",
              allowed: "connectors.allowed",
              failed: "connectors.allowFailed",
            }}
            allow={() => api.allowSecret(notAllowed, name)}
          />
        )}
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
            {t("connectors.save")}
          </Button>
        </div>
      </form>
    </Dialog>
  );
}

function Group({
  legend,
  children,
}: {
  legend: MessageKey;
  children: ReactNode;
}) {
  const t = useT();
  return (
    <fieldset className="route-group stack">
      <legend className="type-label">{t(legend)}</legend>
      {children}
    </fieldset>
  );
}

/** A labelled text field bound to one draft field, with its problem and an optional hint. */
function Text({
  section,
  name,
  labelKey,
  hintKey,
}: {
  section: Section;
  name: "name" | "displayName" | "url" | "header" | "timeout";
  labelKey: MessageKey;
  hintKey?: MessageKey;
}) {
  const t = useT();
  const problem = section.problems[name];
  return (
    <>
      <Input
        label={t(labelKey)}
        value={section.draft[name]}
        onChange={(event) => section.set({ [name]: event.target.value })}
        autoComplete="off"
        spellCheck={false}
        {...(problem ? { error: t(problem) } : {})}
      />
      {hintKey && <p className="type-small muted">{t(hintKey)}</p>}
    </>
  );
}

function Check({
  section,
  name,
  labelKey,
  hintKey,
  disabled = false,
}: {
  section: Section;
  name: "required" | "enabled" | "byDefault";
  labelKey: MessageKey;
  hintKey: MessageKey;
  disabled?: boolean;
}) {
  const t = useT();
  return (
    <div>
      <CheckRow
        className="type-small"
        checked={section.draft[name] && !disabled}
        disabled={disabled}
        onChange={(event) => section.set({ [name]: event.target.checked })}
        label={t(labelKey)}
      />
      <p className="type-small muted">{t(hintKey)}</p>
    </div>
  );
}

function Basics({
  section,
  creating,
}: {
  section: Section;
  creating: boolean;
}) {
  return (
    <Group legend="connectors.basics">
      {creating && (
        <Text
          section={section}
          name="name"
          labelKey="connectors.name"
          hintKey="connectors.nameHint"
        />
      )}
      <Text
        section={section}
        name="displayName"
        labelKey="connectors.displayName"
      />
      <Text
        section={section}
        name="url"
        labelKey="connectors.url"
        hintKey="connectors.urlHint"
      />
    </Group>
  );
}

function Credential({
  section,
  secrets,
}: {
  section: Section;
  secrets: string[];
}) {
  const t = useT();
  const { draft, set, problems } = section;
  const none = draft.scheme === "none";
  const problem = (field: string) =>
    problems[field] ? { error: t(problems[field]) } : {};
  return (
    <Group legend="connectors.credential">
      <Select
        label={t("connectors.sendAs")}
        value={draft.scheme}
        onChange={(choice) =>
          set({ scheme: choice as ConnectorDraft["scheme"] })
        }
        {...problem("scheme")}
      >
        {SCHEMES.map(([scheme, label]) => (
          <option key={scheme} value={scheme}>
            {t(label)}
          </option>
        ))}
      </Select>
      {draft.scheme === "header" && (
        <Text
          section={section}
          name="header"
          labelKey="connectors.header"
          hintKey="connectors.headerHint"
        />
      )}
      <Check
        section={section}
        name="required"
        labelKey="connectors.required"
        hintKey="connectors.requiredHint"
        disabled={none}
      />
      <Select
        label={t("connectors.shared")}
        value={none ? "" : draft.secretRef}
        disabled={none}
        onChange={(choice) => set({ secretRef: choice })}
        {...problem("secretRef")}
      >
        <option value="">{t("connectors.sharedNone")}</option>
        {[...new Set([...secrets, draft.secretRef].filter(Boolean))].map(
          // Each key by its name, as Keys › Shared and the route form show it (WEB-U-291, WEB-U-300); the value saved is
          // still its reference.
          (ref) => (
            <option key={ref} value={ref}>
              {keyName(ref)}
            </option>
          ),
        )}
      </Select>
      <p className="type-small muted">{t("connectors.sharedHint")}</p>
    </Group>
  );
}

function Tools({ section }: { section: Section }) {
  const t = useT();
  const { draft, set, problems } = section;
  return (
    <Group legend="connectors.toolsSection">
      <Select
        label={t("connectors.toolMode")}
        value={draft.toolMode}
        onChange={(choice) =>
          set({ toolMode: choice as ConnectorDraft["toolMode"] })
        }
      >
        <option value="all">{t("connectors.modeAll")}</option>
        <option value="allow">{t("connectors.modeAllow")}</option>
        <option value="deny">{t("connectors.modeDeny")}</option>
      </Select>
      {draft.toolMode !== "all" && (
        <>
          <Textarea
            label={t("connectors.toolNames")}
            value={draft.tools}
            rows={4}
            spellCheck={false}
            onChange={(event) => set({ tools: event.target.value })}
            {...(problems.tools ? { error: t(problems.tools) } : {})}
          />
          <p className="type-small muted">{t("connectors.toolsHint")}</p>
        </>
      )}
    </Group>
  );
}

function Behaviour({ section }: { section: Section }) {
  return (
    <>
      <Group legend="connectors.limits">
        <Text
          section={section}
          name="timeout"
          labelKey="connectors.timeout"
          hintKey="connectors.timeoutHint"
        />
      </Group>
      <Group legend="connectors.availability">
        <Check
          section={section}
          name="enabled"
          labelKey="connectors.enabled"
          hintKey="connectors.enabledHint"
        />
        <Check
          section={section}
          name="byDefault"
          labelKey="connectors.byDefault"
          hintKey="connectors.byDefaultHint"
        />
      </Group>
    </>
  );
}

"use client";

import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Plus } from "lucide-react";
import { useState } from "react";

import { useLanguage, useT } from "../i18n/I18nProvider";
import type { User } from "../protocol/generated/api";
import { Button } from "../ui/Button";
import { CellNote } from "../ui/CellNote";
import { Dialog } from "../ui/Dialog";
import { Icon } from "../ui/Icon";
import { Skeleton } from "../ui/Skeleton";
import { TableScroll } from "../ui/TableScroll";

import type { CredentialsApi, UserConnector } from "./connector-credentials";
import {
  CredentialState,
  RemoveCredential,
  SetCredential,
  shown,
} from "./ConnectorCredentials";
import type { HostKeysApi } from "./host-keys/contract";
import { HostKeyList } from "./host-keys/HostKeyList";
import { KeysViews } from "./KeysViews";
import { LoadFailure } from "./LoadFailure";
import { RowMenu } from "./RowMenu";
import { SecretReads } from "./SecretReads";

const CONNECTOR_KEYS = ["connector-credentials"];

/** One row: a connector that takes a person's key. */
type Row = { service: string; item: UserConnector };

type Open =
  | { ask: true }
  | { setConnector: UserConnector }
  | { removeConnector: UserConnector }
  | { readsConnector: UserConnector }
  | null;

/**
 * Your account › Keys (WEB-U-076, UI-GUIDELINES §7): keys for web services (ADR-0037), then the keys connectors take,
 * each by the service it is for (the owner's direction, c8: people never need to know git). A key for a Git host is a
 * key for a web service: since ADR-0037 §7 the gateway keeps no separate project keys (WEB-U-149). Everyone switches to
 * the shared keys from here.
 */
export function KeysSettings({
  me,
  connectors,
  hostKeys,
}: {
  me: User;
  connectors: CredentialsApi;
  /** Keys for web services (ADR-0037), listed above the rest when given; the gateway binding comes with its API. */
  hostKeys?: HostKeysApi | undefined;
}) {
  const t = useT();
  const { language } = useLanguage();
  const cache = useQueryClient();
  const people = useQuery({
    queryKey: CONNECTOR_KEYS,
    queryFn: ({ signal }) => connectors.connectors(signal),
    retry: false,
  });
  const [open, setOpen] = useState<Open>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const changed = (copy: string, key: readonly string[]) => {
    setOpen(null);
    setNotice(copy);
    void cache.invalidateQueries({ queryKey: key });
  };
  const failed = people.isError;
  const takesKey = (people.data?.items ?? []).filter(
    (item) => item.scheme !== "none",
  );
  const rows: Row[] = takesKey
    .map((item) => ({ service: shown(item), item }))
    .sort((a, b) =>
      a.service.localeCompare(b.service, language, { sensitivity: "base" }),
    );
  return (
    <div className="stack">
      <KeysViews current="mine" />
      {hostKeys && <HostKeyList api={hostKeys} admin={me.role === "admin"} />}
      {/* Its own heading and what it is for, so it never reads as part of the web services' list above (a first-hour
          check before 17.0; WEB-U-262). */}
      <h2 className="type-body-strong">{t("keys.title")}</h2>
      <div className="settings-bar">
        <p className="type-small muted">{t("keys.intro")}</p>
        {/* Nothing to add when no connector here takes a person's key. */}
        {(people.isPending || failed || takesKey.length > 0) && (
          <Button
            // Secondary beside the web services' Add above it, the view's one primary (17.3); the only Add on a page
            // without them.
            variant={hostKeys ? "secondary" : "primary"}
            onClick={() => {
              setNotice(null);
              setOpen({ ask: true });
            }}
          >
            <Icon glyph={Plus} />
            {t("keys.add")}
          </Button>
        )}
      </div>
      {notice && (
        <p className="type-small" role="status">
          {notice}
        </p>
      )}
      {failed ? (
        <LoadFailure
          message={t("keys.failed")}
          onRetry={() => void people.refetch()}
        />
      ) : people.isPending ? (
        <Skeleton label={t("keys.loading")} />
      ) : rows.length === 0 ? (
        <p className="muted">{t("keys.empty")}</p>
      ) : (
        <KeysTable rows={rows} onOpen={setOpen} />
      )}
      {open && "ask" in open && (
        <Dialog open title={t("keys.add")} onClose={() => setOpen(null)}>
          <div className="stack">
            <p>{t("keys.whichService")}</p>
            {/* The connectors that take a person's key, the needed first; then a project, by its link. */}
            {[...takesKey]
              .sort(
                (a, b) =>
                  Number(b.required && !b.has_credential) -
                  Number(a.required && !a.has_credential),
              )
              .map((item) => (
                <Button
                  key={item.name}
                  variant="secondary"
                  onClick={() => setOpen({ setConnector: item })}
                >
                  {shown(item)}
                </Button>
              ))}
          </div>
        </Dialog>
      )}
      {open && "setConnector" in open && (
        <SetCredential
          item={open.setConnector}
          api={connectors}
          onClose={() => setOpen(null)}
          onSaved={() =>
            changed(
              t("credentials.saved", { name: shown(open.setConnector) }),
              CONNECTOR_KEYS,
            )
          }
        />
      )}
      {open && "removeConnector" in open && (
        <RemoveCredential
          item={open.removeConnector}
          api={connectors}
          onClose={() => setOpen(null)}
          onRemoved={() =>
            changed(
              t("credentials.removed", { name: shown(open.removeConnector) }),
              CONNECTOR_KEYS,
            )
          }
        />
      )}
      {open && "readsConnector" in open && (
        <SecretReads
          title={t("reads.titleConnector", {
            name: shown(open.readsConnector),
          })}
          load={(before, signal) =>
            connectors.reads(open.readsConnector.name, before, signal)
          }
          missing="reads.noConnectorCredential"
          onClose={() => setOpen(null)}
        />
      )}
    </div>
  );
}

/** The keys by service: what each is for, whether it is added, and its actions (one actions column, §7). */
function KeysTable({
  rows,
  onOpen,
}: {
  rows: Row[];
  onOpen: (open: Open) => void;
}) {
  const t = useT();
  return (
    <TableScroll label={t("keys.list")}>
      <table className="data-table" aria-label={t("keys.list")}>
        <thead className="type-label">
          <tr>
            <th scope="col">{t("keys.service")}</th>
            <th scope="col">{t("keys.state")}</th>
            <th scope="col">
              <span className="sr-only">{t("common.actions")}</span>
            </th>
          </tr>
        </thead>
        <tbody>
          {rows.map((row) => (
            <ConnectorRow key={row.item.name} item={row.item} onOpen={onOpen} />
          ))}
        </tbody>
      </table>
    </TableScroll>
  );
}

function ConnectorRow({
  item,
  onOpen,
}: {
  item: UserConnector;
  onOpen: (open: Open) => void;
}) {
  const t = useT();
  const name = shown(item);
  return (
    <tr>
      <td>
        {/* The name people know; the internal id is for administrators (the Connectors page). */}
        <div className="stack route-name">
          <div className="data-name">
            <span>{name}</span>
          </div>
          <CellNote>{t("keys.forConnector")}</CellNote>
        </div>
      </td>
      <td>
        <CredentialState item={item} />
      </td>
      <td className="data-cell-action">
        <Button
          variant="ghost"
          aria-label={t(
            item.has_credential
              ? "credentials.replaceName"
              : "credentials.addName",
            { name },
          )}
          onClick={() => onOpen({ setConnector: item })}
        >
          {t(item.has_credential ? "keys.replace" : "credentials.add")}
        </Button>
        {item.has_credential && (
          <RowMenu
            label={t("credentials.more", { name })}
            actions={[
              {
                id: "reads",
                label: t("reads.show"),
                name: t("reads.showConnector", { name }),
                onSelect: () => onOpen({ readsConnector: item }),
              },
              {
                id: "remove",
                label: t("credentials.remove"),
                name: t("credentials.removeName", { name }),
                danger: true,
                onSelect: () => onOpen({ removeConnector: item }),
              },
            ]}
          />
        )}
      </td>
    </tr>
  );
}

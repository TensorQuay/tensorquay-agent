"use client";

import { useQuery } from "@tanstack/react-query";
import { useId, useState } from "react";

import { newActionKey } from "../api/action-key";
import { useT } from "../i18n/I18nProvider";
import type { MessageKey } from "../i18n/translate";
import { Button } from "../ui/Button";
import { ConfirmDialog } from "../ui/ConfirmDialog";

import {
  PRESET_ORDER,
  type EgressApi,
  type EgressPreset,
  type EgressPresetId,
} from "./admin/egress";

export const PRESETS_KEY = ["admin-egress-presets"];

/** Each preset's name and one plain line, keyed by the gateway's id (the web owns the words, F20). */
export const PRESET_COPY: Readonly<
  Record<EgressPresetId, { name: MessageKey; line: MessageKey }>
> = {
  package_registries: {
    name: "egress.presetPackages",
    line: "egress.presetPackagesLine",
  },
  code_hosting: { name: "egress.presetCode", line: "egress.presetCodeLine" },
  open_web: { name: "egress.presetWeb", line: "egress.presetWebLine" },
};

/**
 * Ready-made lists (WEB-U-118, a first-hour check): what tasks usually need, allowed in one step. Each says what it
 * allows and whether it is in the allow-list, and Custom opens Add rule for any other site; Open web asks first, and is never called read-only (the proxy can't
 * tell a read from a write inside TLS).
 */
export function EgressPresets({
  api,
  onChanged,
  onCustom,
}: {
  api: EgressApi;
  /** Custom: any other site, by the Add rule dialog. */
  onCustom: () => void;
  /** A list was added or removed: the words to say, and the allow-list to read again. */
  onChanged: (notice: string) => void;
}) {
  const t = useT();
  const heading = useId();
  const presets = useQuery({
    queryKey: PRESETS_KEY,
    queryFn: ({ signal }) => api.presets(signal),
    retry: false,
  });
  const [asking, setAsking] = useState(false);
  const [pending, setPending] = useState<EgressPresetId | null>(null);
  const name = (id: EgressPresetId) => t(PRESET_COPY[id].name);
  const hosts = (list: { host: string }[]) =>
    list.map((each) => each.host).join(", ");
  async function apply(id: EgressPresetId) {
    setPending(id);
    try {
      const { added, kept } = await api.applyPreset(id, newActionKey());
      onChanged(
        [
          id === "open_web"
            ? t("egress.presetAddedAll", { name: name(id) })
            : t("egress.presetAdded", { name: name(id), count: added.length }),
          ...(kept.length
            ? [t("egress.presetKept", { hosts: hosts(kept) })]
            : []),
        ].join(" "),
      );
    } catch {
      onChanged(t("egress.presetFailed"));
    } finally {
      setPending(null);
    }
  }
  async function remove(id: EgressPresetId) {
    setPending(id);
    try {
      const { kept } = await api.removePreset(id, newActionKey());
      onChanged(
        [
          t("egress.presetRemoved", { name: name(id) }),
          ...(kept.length
            ? [t("egress.presetYoursKept", { hosts: hosts(kept) })]
            : []),
        ].join(" "),
      );
    } catch {
      onChanged(t("egress.presetFailed"));
    } finally {
      setPending(null);
    }
  }
  if (!presets.data) return null;
  const byId = new Map(presets.data.items.map((item) => [item.id, item]));
  return (
    <section className="stack" aria-labelledby={heading}>
      <h2 id={heading} className="type-heading">
        {t("egress.presets")}
      </h2>
      <div className="preset-cards">
        {PRESET_ORDER.flatMap((id) => {
          const preset = byId.get(id);
          return preset
            ? [
                <Preset
                  key={id}
                  preset={preset}
                  pending={pending === id}
                  onAdd={() =>
                    id === "open_web" ? setAsking(true) : void apply(id)
                  }
                  onRemove={() => void remove(id)}
                />,
              ]
            : [];
        })}
        <div
          role="group"
          aria-label={t("egress.presetCustom")}
          className="preset-card stack"
        >
          <span className="type-body-strong">{t("egress.presetCustom")}</span>
          <p className="type-small muted">{t("egress.presetCustomLine")}</p>
          <div className="preset-actions">
            <Button variant="secondary" onClick={onCustom}>
              {t("egress.presetCustomAdd")}
            </Button>
          </div>
        </div>
      </div>
      {asking && (
        <ConfirmDialog
          title={t("egress.webTitle")}
          confirm={t("egress.webConfirm")}
          pending={pending === "open_web"}
          error={null}
          errorCopy={() => t("egress.presetFailed")}
          onConfirm={() => {
            setAsking(false);
            void apply("open_web");
          }}
          onClose={() => setAsking(false)}
        >
          <p>{t("egress.webBody")}</p>
        </ConfirmDialog>
      )}
    </section>
  );
}

function Preset({
  preset,
  pending,
  onAdd,
  onRemove,
}: {
  preset: EgressPreset;
  pending: boolean;
  onAdd: () => void;
  onRemove: () => void;
}) {
  const t = useT();
  const name = t(PRESET_COPY[preset.id].name);
  const sites =
    preset.id === "open_web"
      ? ""
      : ` ${t("egress.presetSites", { count: preset.rules.length })}`;
  return (
    <div role="group" aria-label={name} className="preset-card stack">
      <span className="type-body-strong">{name}</span>
      <p className="type-small muted">
        {`${t(PRESET_COPY[preset.id].line)}${sites}`}
      </p>
      {preset.applied === "partial" && (
        <p className="type-small">{t("egress.presetPartial")}</p>
      )}
      <div className="preset-actions">
        {/* Whether the list is on, said on every card, not only by its buttons (WEB-U-162). */}
        <span
          role="status"
          className="status-pill type-label"
          data-tone={
            // On is a setting (muted, 17.3); a list only partly on is a warning.
            preset.applied === "all"
              ? "muted"
              : preset.applied === "partial"
                ? "warn"
                : "muted"
          }
        >
          {t(
            preset.applied === "all"
              ? "egress.presetOn"
              : preset.applied === "partial"
                ? "egress.presetPartlyOn"
                : "egress.presetOff",
          )}
        </span>
        {preset.applied !== "all" && (
          <Button
            variant="secondary"
            loading={pending}
            aria-label={t("egress.presetAddName", { name })}
            onClick={onAdd}
          >
            {t(
              preset.applied === "partial"
                ? "egress.presetAddAgain"
                : "egress.presetAdd",
            )}
          </Button>
        )}
        {preset.applied !== "none" && (
          <Button
            variant="secondary"
            disabled={pending}
            aria-label={t("egress.presetRemoveName", { name })}
            onClick={onRemove}
          >
            {t("egress.presetRemove")}
          </Button>
        )}
      </div>
    </div>
  );
}

"use client";

import "./catalogue";
import { useState } from "react";

import { ApiError } from "../api/error";
import { useT } from "../i18n/I18nProvider";
import { Button } from "../ui/Button";
import { Dialog } from "../ui/Dialog";
import { Input } from "../ui/Input";

import { resetTime } from "./budget-time";
import type { Channel } from "./contract";

/**
 * A channel's wake budget (ADR-0014's amendment, A5; decided on 20 Nov 2026, C-1; WEB-U-529): what it is a day and what
 * woken runs spent today, and, while a window is open, when it renews in the viewer's time and who can raise it sooner
 * (C-14, WEB-U-533); its owner, a person who leads it, or an administrator (`may`) sets the number, and everyone else
 * reads who can.
 */
export function ChannelBudget({
  channel,
  may,
  onSave,
  onClose,
}: {
  channel: Channel;
  may: boolean;
  onSave: (budget: number) => Promise<void>;
  onClose: () => void;
}) {
  const t = useT();
  const budget = channel.wake_budget ?? 0;
  const spent = channel.spent_tokens ?? 0;
  const [value, setValue] = useState(String(budget));
  const [pending, setPending] = useState(false);
  const [failure, setFailure] = useState<
    "channels.budgetInvalid" | "channels.budgetFailed" | null
  >(null);
  const number = new Intl.NumberFormat(t.locale);
  async function save() {
    const wanted = Number(value);
    if (value.trim() === "" || !Number.isInteger(wanted) || wanted < 0) {
      setFailure("channels.budgetInvalid");
      return;
    }
    setPending(true);
    setFailure(null);
    try {
      await onSave(wanted);
      onClose();
    } catch (error) {
      setFailure(
        error instanceof ApiError && error.status === 400
          ? "channels.budgetInvalid"
          : "channels.budgetFailed",
      );
    } finally {
      setPending(false);
    }
  }
  return (
    <Dialog
      open
      title={t("channels.budgetTitle")}
      onClose={onClose}
      actions={
        may ? (
          <>
            <Button variant="ghost" onClick={onClose}>
              {t("common.cancel")}
            </Button>
            <Button loading={pending} onClick={() => void save()}>
              {t("page.save")}
            </Button>
          </>
        ) : (
          <Button variant="secondary" onClick={onClose}>
            {t("common.close")}
          </Button>
        )
      }
    >
      <div className="stack">
        <p>
          {t("channels.budgetSpentOf", {
            spent: number.format(spent),
            budget: number.format(budget),
            percent: number.format(
              budget > 0 ? Math.round((spent * 100) / budget) : 100,
            ),
          })}
        </p>
        {channel.budget_resets_at && (
          <p>
            {t("channels.budgetResets", {
              time: resetTime(channel.budget_resets_at, t.locale),
            })}
          </p>
        )}
        {may ? (
          <>
            <Input
              label={t("channels.budgetField")}
              type="number"
              min={0}
              step={1000}
              inputMode="numeric"
              value={value}
              aria-describedby="channel-budget-hint"
              onChange={(event) => setValue(event.target.value)}
            />
            <p id="channel-budget-hint" className="type-small muted">
              {t("channels.budgetHint")}
            </p>
            {failure && (
              <p className="field-error" role="alert">
                {t(failure)}
              </p>
            )}
          </>
        ) : (
          <p className="type-small muted">{t("channels.budgetWho")}</p>
        )}
      </div>
    </Dialog>
  );
}

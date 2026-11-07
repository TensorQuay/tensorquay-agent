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
import { Select } from "../ui/Select";

import type { Limit, LimitsApi } from "./admin/limits";
import {
  APPLIES,
  display,
  DURATIONS,
  durationUnit,
  type DurationUnit,
  hasCeiling,
  INPUT,
  NAMES,
  refusal,
  source,
} from "./limit-copy";

type FormProps = {
  item: Limit;
  api: LimitsApi;
  onSaved: (item: Limit) => void;
  onReload: () => Promise<Limit | undefined>;
  onClose: () => void;
  /** Asks to reset a value set here to the configuration file's; absent, no reset is offered. */
  onReset?: (item: Limit) => void;
};

/**
 * The typed value and its save: the page's range check first, then the gateway's own reason (limit_out_of_range may
 * name a floor the page did not know), so the two can never disagree silently. A conflict asks to reload (slice 5 §5).
 */
function useLimitSave({ item, api, onSaved, onReload }: FormProps) {
  const { step } = INPUT[item.unit];
  const duration = item.unit === "seconds";
  const [unit, setUnit] = useState(durationUnit(item.value));
  const scaleOf = (value: DurationUnit) =>
    duration ? DURATIONS[value].seconds : INPUT[item.unit].scale;
  const scale = scaleOf(unit);
  const decimals = scale > step;
  const [current, setCurrent] = useState(item);
  const [text, setText] = useState(String(item.value / scale));
  const [problem, setProblem] = useState<MessageKey | null>(null);
  const [stale, setStale] = useState(false);
  const [pending, setPending] = useState(false);
  const ceiling = hasCeiling(current.max);
  // The range in the unit edited, to two decimals at most and never beyond the gateway's bounds.
  const min = Math.ceil((current.min / scale) * 100 - 1e-6) / 100;
  const max = ceiling
    ? Math.floor((current.max / scale) * 100 + 1e-6) / 100
    : Infinity;
  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const pattern = decimals ? /^\d+(\.\d+)?$/ : /^\d+$/;
    const typed = pattern.test(text.trim()) ? Number(text.trim()) : NaN;
    // The gateway's unit: memory in whole MiB, CPU in whole millicores.
    const value = Math.round((typed * scale) / step) * step;
    if (
      !Number.isSafeInteger(value) ||
      value < current.min ||
      (ceiling && value > current.max)
    ) {
      const [bounded, open] = decimals
        ? (["limits.number", "limits.numberFrom"] as const)
        : (["limits.wholeNumber", "limits.wholeNumberFrom"] as const);
      // Desktop environments at once can go up to Tasks running at once (ADR-0039 §10), which is its max as the
      // gateway lists it: above it, the rule and its number say why, not only a range (the review of WEB-E2E-067).
      const overSlots =
        current.key === "desktop.max_sessions" &&
        ceiling &&
        Number.isSafeInteger(value) &&
        value > current.max;
      setProblem(
        overSlots ? "limits.orderDesktopNow" : ceiling ? bounded : open,
      );
      return;
    }
    setProblem(null);
    setPending(true);
    try {
      onSaved(
        await api.putLimit(current.key, value, current.version, newActionKey()),
      );
    } catch (error) {
      setProblem(refusal(error, current.key));
      setStale(error instanceof ApiError && error.code === "conflict");
    } finally {
      setPending(false);
    }
  }
  async function reload() {
    const fresh = await onReload();
    if (fresh) {
      const freshUnit = durationUnit(fresh.value);
      setCurrent(fresh);
      setUnit(freshUnit);
      setText(String(fresh.value / scaleOf(freshUnit)));
    }
    setProblem(null);
    setStale(false);
  }
  /**
   * Changes a duration's unit and converts the typed value through whole seconds (what is saved), so 90 minutes reads
   * 1.5 hours, never 90 hours. Four decimals keep every whole second exact; a blank or unreadable value stays as typed.
   */
  function changeUnit(next: DurationUnit) {
    const typed = text.trim() === "" ? NaN : Number(text.trim());
    if (Number.isFinite(typed)) {
      const seconds = Math.round(typed * scale);
      setText(String(Number((seconds / scaleOf(next)).toFixed(4))));
    }
    setUnit(next);
  }
  return {
    current,
    duration,
    unit,
    changeUnit,
    decimals,
    text,
    setText,
    problem,
    stale,
    pending,
    min,
    max,
    submit,
    reload,
  };
}

/**
 * Edits one limit in the unit the page shows it in (memory in GiB, CPU in CPUs, a duration in its unit), with the allowed range stated; a value
 * set here can be reset from here, so a table row holds only Edit (UI-GUIDELINES §7).
 */
export function LimitForm(props: FormProps) {
  const { item, onClose, onReset } = props;
  const t = useT();
  const form = useLimitSave(props);
  const ceiling = form.max !== Infinity;
  const range = { min: String(form.min), max: String(form.max) };
  const applies = APPLIES[item.key];
  return (
    <Dialog
      open
      title={t("limits.editTitle", { name: t(NAMES[item.key]) })}
      onClose={() => {
        if (!form.pending) onClose();
      }}
    >
      {/* The page's range check and the gateway's reason speak, not the browser's own tooltip. */}
      <form
        className="stack"
        noValidate
        onSubmit={(event) => void form.submit(event)}
      >
        <div className={form.duration ? "limit-value" : undefined}>
          <Input
            label={t(INPUT[item.unit].label)}
            type="number"
            inputMode={form.decimals ? "decimal" : "numeric"}
            min={form.min}
            {...(ceiling ? { max: form.max } : {})}
            step={form.decimals ? "any" : 1}
            value={form.text}
            onChange={(event) => form.setText(event.target.value)}
            aria-describedby="limit-range"
            {...(form.problem ? { error: t(form.problem, range) } : {})}
          />
          {/* A unit change converts the number, and the range below follows it (WEB-U-041). */}
          {form.duration && (
            <Select
              label={t("limits.unit")}
              value={form.unit}
              onChange={(choice) => form.changeUnit(choice as DurationUnit)}
            >
              {(Object.keys(DURATIONS) as DurationUnit[]).map((unit) => (
                <option key={unit} value={unit}>
                  {t(DURATIONS[unit].choice)}
                </option>
              ))}
            </Select>
          )}
        </div>
        <p id="limit-range" className="type-small muted">
          {t(ceiling ? "limits.rangeHint" : "limits.rangeHintFrom", range)}
        </p>
        {/* When the change takes effect, where it differs from the page's rule (the Desktop limits). */}
        {applies && <p className="type-small muted">{t(applies.whole)}</p>}
        {/* The default and where the value comes from, out of the table (WEB-U-041). */}
        <p className="type-small muted">
          {t("limits.defaultIs", {
            value: display(t, item.unit, item.default),
          })}
          {" · "}
          {t(source(item))}
        </p>
        {onReset && form.current.last_set_by === "admin" && (
          <div>
            <Button
              variant="secondary"
              type="button"
              disabled={form.pending}
              onClick={() => onReset(form.current)}
            >
              {t("limits.resetToFile")}
            </Button>
          </div>
        )}
        <FormRefusal />
        <div className="dialog-actions">
          {form.stale && (
            <Button
              variant="secondary"
              type="button"
              onClick={() => void form.reload()}
            >
              {t("limits.reload")}
            </Button>
          )}
          <Button
            variant="secondary"
            type="button"
            disabled={form.pending}
            onClick={onClose}
          >
            {t("common.cancel")}
          </Button>
          <Button type="submit" loading={form.pending}>
            {t("limits.save")}
          </Button>
        </div>
      </form>
    </Dialog>
  );
}

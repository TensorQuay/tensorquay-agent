import { english, type Translate } from "../i18n/translate";
import type { Model } from "../protocol/generated/api";

/** A route as the picker reads it: where it runs is the gateway's own answer (runs_on), never a host name. */
export type RoutedModel = Model;

/**
 * A model's muted second line in the picker (the UI standard: every row has one or none does): where it runs, then
 * what it can do, in plain words ("On this computer · Reads images", "DeepSeek · Thinks before answering"). Null when
 * the route says nothing of either; the administrator's description, when written, is used instead of this.
 */
export function modelLine(
  model: RoutedModel,
  t: Translate = english,
): string | null {
  const where =
    model.runs_on.where === "this_computer"
      ? t("picker.runsLocal")
      : model.runs_on.where === "network"
        ? t("picker.runsNetwork")
        : (model.runs_on.provider ?? t("picker.runsHosted"));
  const parts = [
    where,
    model.vision ? t("picker.readsImages") : null,
    model.reasoning ? t("picker.thinks") : null,
  ].filter((part): part is string => Boolean(part));
  return parts.length > 0 ? parts.join(" · ") : null;
}

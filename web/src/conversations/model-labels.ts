import { english, type Translate } from "../i18n/translate";

import type { RoutedModel } from "./model-line";
import { modelName } from "./model-name";

/** Where a route's model runs, in the picker's words (as modelLine's first part). */
function where(model: RoutedModel, t: Translate): string {
  return model.runs_on.where === "this_computer"
    ? t("picker.runsLocal")
    : model.runs_on.where === "network"
      ? t("picker.runsNetwork")
      : (model.runs_on.provider ?? t("picker.runsHosted"));
}

/**
 * Each route's label in the model picker, by route name (15c's known issue: two routes with one display name read the
 * same). A label no other route has is the route's name as people read it (modelName). Routes whose labels match,
 * ignoring case, each add what tells them apart, in people's words and never the route's id (UI-GUIDELINES §10): the
 * model's readable name when that differs within the group, else where it runs, else their place in the list.
 */
export function modelLabels(
  models: readonly RoutedModel[],
  t: Translate = english,
): Map<string, string> {
  const groups = new Map<string, RoutedModel[]>();
  for (const model of models) {
    const key = modelName(model).toLocaleLowerCase();
    groups.set(key, [...(groups.get(key) ?? []), model]);
  }
  const labels = new Map<string, string>();
  for (const group of groups.values()) {
    if (group.length === 1) {
      const [model] = group;
      if (model) labels.set(model.name, modelName(model));
      continue;
    }
    const readable = (model: RoutedModel) => modelName({ model: model.model });
    const apart = (part: (model: RoutedModel) => string) =>
      new Set(group.map((model) => part(model).toLocaleLowerCase())).size ===
      group.length;
    group.forEach((model, index) => {
      const name = modelName(model);
      const label = apart(readable)
        ? t("picker.modelApart", { name, apart: readable(model) })
        : apart((each) => where(each, t))
          ? t("picker.modelApart", { name, apart: where(model, t) })
          : t("picker.modelNumbered", { name, count: index + 1 });
      labels.set(model.name, label);
    });
  }
  return labels;
}

/**
 * The route New task starts with when the person hasn't picked one: the first whose current check didn't fail
 * (verified false), so an unchecked route still counts; the first route when every check failed, as before.
 */
export function defaultRoute(
  models: readonly RoutedModel[] | undefined,
): string {
  return (
    (models?.find((model) => model.verified !== false) ?? models?.[0])?.name ??
    ""
  );
}

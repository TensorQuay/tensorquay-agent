import { Sparkles } from "lucide-react";

import { useT } from "../i18n/I18nProvider";
import { MenuButton } from "../ui/MenuButton";

import { modelLabels } from "./model-labels";
import { modelLine, type RoutedModel } from "./model-line";

/**
 * Uses the display name or a readable model name (modelName), keeping route identity off the label; routes that would
 * read the same say what tells them apart (modelLabels). A route whose last check failed says so on its row only.
 */
export function ModelPicker({
  models,
  route,
  onChange,
  disabled = false,
  unavailable = false,
  disabledReason,
  placement,
}: {
  models: RoutedModel[] | undefined;
  route: string;
  onChange: (route: string) => void;
  disabled?: boolean;
  unavailable?: boolean;
  disabledReason?: string | undefined;
  /** Where its choices open (the task header's open below, aligned to their end). */
  placement?: "below-start" | "below-end" | "above-start" | undefined;
}) {
  const t = useT();
  const labels = modelLabels(models ?? [], t);
  return (
    <MenuButton
      label={t("picker.model")}
      value={route}
      onChange={onChange}
      disabled={disabled}
      disabledReason={disabledReason}
      {...(placement ? { placement } : {})}
      icon={Sparkles}
      options={
        models?.map((model) => {
          // What it is good for, in the administrator's words; else where it runs and what it can do (WEB-U-103).
          const line = model.description ?? modelLine(model, t);
          // A person picking a failing route by hand knows first (WEB-U-322); the closed button never says it.
          const failing = model.verified === false;
          return {
            id: model.name,
            label: labels.get(model.name) ?? model.name,
            ...(line || failing
              ? {
                  detail: (
                    <>
                      {line && <span className="type-small muted">{line}</span>}
                      {failing && (
                        <span className="type-small muted">
                          {t("picker.lastCheckFailed")}
                        </span>
                      )}
                    </>
                  ),
                }
              : {}),
          };
        }) ?? []
      }
      fallback={t(
        unavailable
          ? "picker.modelsUnavailable"
          : models
            ? "picker.modelUnavailable"
            : "picker.loadingModels",
      )}
    />
  );
}

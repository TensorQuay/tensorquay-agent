"use client";

import type { ReactNode } from "react";

import { areaLoader, useAreaCatalogue } from "../i18n/area-catalogue";
import { useT } from "../i18n/I18nProvider";
import { Skeleton } from "../ui/Skeleton";

import { LoadFailure } from "./LoadFailure";

const AREA = "settings";

/**
 * Loads the settings pages' English once (WEB-U-014): the conversation page never downloads it
 * (WEB-U-014). Other languages load their whole catalogue already, settings messages included.
 */
export const loadSettingsCatalogue = areaLoader(
  AREA,
  () => import("../i18n/generated/en.settings.json"),
);

/** Renders the settings pages once their copy is available; a failed download can be retried. */
export function SettingsCatalogue({
  children,
  load = loadSettingsCatalogue,
}: {
  children: ReactNode;
  load?: () => Promise<void>;
}) {
  const t = useT();
  const { state, retry } = useAreaCatalogue(
    AREA,
    load,
    load === loadSettingsCatalogue,
  );
  if (state === "ready") return children;
  if (state === "failed")
    return <LoadFailure message={t("panel.failed")} onRetry={retry} />;
  return <Skeleton label={t("skeleton.loading")} />;
}

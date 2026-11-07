"use client";

import type { ReactNode } from "react";

import { areaLoader, useAreaCatalogue } from "../i18n/area-catalogue";
import { useT } from "../i18n/I18nProvider";
import { LoadFailure } from "../settings/LoadFailure";
import { Skeleton } from "../ui/Skeleton";

const AREA = "context";

/** The Context tab's English, downloaded when the tab first opens (WEB-U-014 budget, design-context-panel). */
export const loadContextCatalogue = areaLoader(
  AREA,
  () => import("../i18n/generated/en.context.json"),
);

/** Renders the Context tab once its copy is available; a failed download can be retried. */
export function ContextCatalogue({
  children,
  load = loadContextCatalogue,
}: {
  children: ReactNode;
  load?: () => Promise<void>;
}) {
  const t = useT();
  const { state, retry } = useAreaCatalogue(
    AREA,
    load,
    load === loadContextCatalogue,
  );
  if (state === "ready") return children;
  if (state === "failed")
    return <LoadFailure message={t("panel.failed")} onRetry={retry} />;
  return <Skeleton label={t("context.loading")} />;
}

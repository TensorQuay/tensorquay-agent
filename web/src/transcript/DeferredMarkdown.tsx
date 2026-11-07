"use client";

import { lazy, Suspense } from "react";

import { useT } from "../i18n/I18nProvider";
import { Skeleton } from "../ui/Skeleton";

const Markdown = lazy(() =>
  import("./AssistantMarkdown").then(({ AssistantMarkdown }) => ({
    default: AssistantMarkdown,
  })),
);

/** Loads the parser only when recorded text needs it; pending text stays in the transcript store. */
export function DeferredMarkdown({ text }: { text: string }) {
  const t = useT();
  return (
    <Suspense
      fallback={<Skeleton label={t("markdown.formatting")} lines={1} />}
    >
      <Markdown text={text} />
    </Suspense>
  );
}

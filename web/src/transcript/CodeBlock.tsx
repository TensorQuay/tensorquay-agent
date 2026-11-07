"use client";

import { useState } from "react";

import { useRequest } from "../api/use-request";
import { useT } from "../i18n/I18nProvider";
import { Button } from "../ui/Button";
import { ErrorMessage } from "../ui/ErrorMessage";

import { copyText } from "./copy-text";
import "../i18n/deferred-catalogue";

/** Keeps code literal, labelled and horizontally scrollable with a visible copy result. */
export function CodeBlock({
  text,
  language,
}: {
  text: string;
  language: string;
}) {
  const request = useRequest();
  const t = useT();
  const [copied, setCopied] = useState<string | null>(null);
  async function copy() {
    const result = await request.run(() => copyText(text));
    if (result.ok) setCopied(text);
  }
  return (
    <div className="markdown-code">
      <div className="row code-heading">
        <span className="type-label muted">{language}</span>
        <Button
          variant="ghost"
          loading={request.pending}
          onClick={() => {
            void copy();
          }}
        >
          {t(copied === text ? "code.copied" : "code.copy")}
        </Button>
      </div>
      {/* A labelled group, not a landmark: an answer can hold several blocks of one language (axe landmark-unique). */}
      <pre
        className="code-block type-code"
        role="group"
        tabIndex={0}
        aria-label={t("code.region", { language })}
      >
        <code>{text}</code>
      </pre>
      <ErrorMessage error={request.error} />
    </div>
  );
}

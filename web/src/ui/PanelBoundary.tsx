"use client";

import { Component, type ReactNode } from "react";

import { useT } from "../i18n/I18nProvider";

import { Button } from "./Button";

/** The translated fallback a failed panel shows in place of its content. */
export function PanelFailure({ onRetry }: { onRetry: () => void }) {
  const t = useT();
  return (
    <div className="stack">
      <p role="alert">{t("panel.failed")}</p>
      <Button variant="secondary" onClick={onRetry}>
        {t("common.tryAgain")}
      </Button>
    </div>
  );
}

/**
 * Isolates a rendering failure to its panel without exposing untrusted error details. A panel with its own frame gives
 * `fallback`, so its header stays around the failure (WEB-U-253); it gets the retry to offer.
 */
export class PanelBoundary extends Component<
  { children: ReactNode; fallback?: (retry: () => void) => ReactNode },
  { failed: boolean }
> {
  override state = { failed: false };
  static getDerivedStateFromError() {
    return { failed: true };
  }
  override render() {
    const retry = () => this.setState({ failed: false });
    if (!this.state.failed) return this.props.children;
    return this.props.fallback ? (
      this.props.fallback(retry)
    ) : (
      <PanelFailure onRetry={retry} />
    );
  }
}

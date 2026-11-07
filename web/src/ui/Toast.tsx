"use client";

import { Check, Info, TriangleAlert, X } from "lucide-react";
import { useSyncExternalStore } from "react";

import { useT } from "../i18n/I18nProvider";

import { Button } from "./Button";
import { Icon } from "./Icon";
import { toastStore, type ToastStore } from "./toast-store";

/** Announces store messages with accessible dismissal and optional actions. */
export function Toast({
  store = toastStore,
  preview = false,
}: {
  store?: ToastStore;
  preview?: boolean;
}) {
  const t = useT();
  const entries = useSyncExternalStore(
    store.subscribe,
    store.snapshot,
    store.serverSnapshot,
  );
  return (
    <section
      aria-label={t("toast.region")}
      className={`toast-region ${preview ? "toast-preview" : ""}`}
    >
      {entries.map((entry) => (
        <div
          className="toast"
          role={entry.tone === "error" ? "alert" : "status"}
          key={entry.id}
        >
          <Icon
            glyph={
              entry.tone === "error"
                ? TriangleAlert
                : entry.tone === "success"
                  ? Check
                  : Info
            }
            size={16}
          />
          <span className="toast-message">{entry.message}</span>
          {entry.action && (
            <Button variant="ghost" onClick={entry.action.onClick}>
              {entry.action.label}
            </Button>
          )}
          <Button
            variant="ghost"
            className="icon-button"
            aria-label={t("toast.dismiss", { message: entry.message })}
            onClick={() => store.dismiss(entry.id)}
          >
            <Icon glyph={X} size={16} />
          </Button>
        </div>
      ))}
    </section>
  );
}

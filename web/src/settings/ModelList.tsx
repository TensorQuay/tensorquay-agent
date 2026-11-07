"use client";

import { useId, useRef, useState, type KeyboardEvent } from "react";

import { ApiError } from "../api/error";
import { useT } from "../i18n/I18nProvider";
import type { MessageValues } from "../i18n/message";
import type { MessageKey } from "../i18n/translate";
import { Button } from "../ui/Button";

import type { RouteModels, RoutesApi } from "./admin/routes";
import { failureKey } from "./probe-copy";

type Failure = NonNullable<RouteModels["failure"]>;
type State =
  | { kind: "closed" }
  | { kind: "loading" }
  | { kind: "listed"; listing: RouteModels; wait?: number }
  | { kind: "refused"; words: [MessageKey, MessageValues] };

/** The listing's own words where the check's would name the wrong thing; the check's otherwise. */
const failureWords = (failure: Failure): MessageKey =>
  failure.code === "not_found"
    ? "models.listNotFound"
    : failure.code === "models_unreadable"
      ? "models.listUnreadable"
      : failureKey(failure.code);

/**
 * Each route's last usable listing, for the page's life, per API client (so tests don't share it): while the gateway
 * asks to wait, it shows instead of an error (F5), even after the form was closed and opened again.
 */
const lastListings = new WeakMap<RoutesApi, Map<string, RouteModels>>();

/** Why a listing could not run, with the gateway's wait (Retry-After, always sent on its 429). */
function refusal(error: unknown): [MessageKey, MessageValues] {
  if (!(error instanceof ApiError)) return ["models.listFailed", {}];
  if (error.code === "key_missing") return ["models.checkKeyMissing", {}];
  if (error.code === "models_running") return ["models.listRunning", {}];
  if (error.status === 429 && error.retryAfter)
    return ["models.listTooSoon", { seconds: error.retryAfter }];
  if (error.status === 504) return ["models.listTimeout", {}];
  return ["models.listFailed", {}];
}

/**
 * List models, under the route form's Model field (WEB-U-067): what the saved route's server offers, listed with its
 * saved address and key, to pick Model from. Only for a saved route, and only while the address and key in the form
 * are the saved ones. Picking fills Model; nothing is saved until the form is.
 */
/** A filter leads the list from this many models; fewer are a few buttons, each reached with Tab. */
const FILTER_FROM = 8;

export function ModelList({
  api,
  route,
  stale,
  value,
  onPick,
}: {
  api: RoutesApi;
  /** The saved route's name; null for a new route, which has nothing stored to list with. */
  route: string | null;
  /** The form has unsaved changes to what the listing uses (address, protocol, key or headers). */
  stale: boolean;
  value: string;
  onPick: (id: string) => void;
}) {
  const t = useT();
  const base = useId();
  const [state, setState] = useState<State>({ kind: "closed" });
  const [filter, setFilter] = useState("");
  const [active, setActive] = useState(-1);
  const abort = useRef<AbortController | null>(null);
  if (route === null) return null;
  const name = route;
  async function list() {
    abort.current?.abort();
    const controller = new AbortController();
    abort.current = controller;
    setFilter("");
    setActive(-1);
    setState({ kind: "loading" });
    try {
      const listing = await api.models(name, controller.signal);
      if (!listing.failure) {
        const kept = lastListings.get(api) ?? new Map<string, RouteModels>();
        kept.set(name, listing);
        lastListings.set(api, kept);
      }
      if (!controller.signal.aborted) setState({ kind: "listed", listing });
    } catch (error) {
      if (controller.signal.aborted) return;
      const kept = lastListings.get(api)?.get(name);
      if (
        kept &&
        error instanceof ApiError &&
        error.status === 429 &&
        error.retryAfter
      )
        setState({ kind: "listed", listing: kept, wait: error.retryAfter });
      else setState({ kind: "refused", words: refusal(error) });
    }
  }
  const close = () => {
    abort.current?.abort();
    setState({ kind: "closed" });
  };
  const pick = (id: string) => {
    onPick(id);
    close();
  };
  const listing = state.kind === "listed" ? state.listing : null;
  const needle = filter.trim().toLowerCase();
  const shown = (listing?.models ?? []).filter(
    (model) =>
      !needle ||
      model.id.toLowerCase().includes(needle) ||
      (model.display_name ?? "").toLowerCase().includes(needle),
  );
  function keys(event: KeyboardEvent<HTMLInputElement>) {
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      const step = event.key === "ArrowDown" ? 1 : -1;
      setActive((current) =>
        shown.length === 0
          ? -1
          : Math.min(shown.length - 1, Math.max(0, current + step)),
      );
    } else if (event.key === "Enter") {
      event.preventDefault();
      const chosen = shown[active];
      if (chosen) pick(chosen.id);
    } else if (event.key === "Escape") {
      event.preventDefault();
      close();
    }
  }
  const optionId = (index: number) => `${base}-model-${index}`;
  const failure = listing?.failure ?? null;
  return (
    <div className="stack model-list">
      <div>
        <Button
          variant="secondary"
          type="button"
          disabled={stale || state.kind === "loading"}
          onClick={() => void list()}
        >
          {t("models.listModels")}
        </Button>
      </div>
      {stale && <p className="type-small muted">{t("models.listSaveFirst")}</p>}
      {state.kind === "loading" && (
        <p className="type-small muted" role="status">
          {t("models.listing")}
        </p>
      )}
      {state.kind === "refused" && (
        <p className="field-error" role="alert">
          {t(state.words[0], state.words[1])}
        </p>
      )}
      {listing && failure && (
        <div className="stack">
          <p className="field-error" role="alert">
            {t(failureWords(failure))}
          </p>
          {failure.detail && <p className="type-small">{failure.detail}</p>}
          {(failure.status !== null || failure.excerpt) && (
            <div className="stack">
              <p className="type-label muted">{t("models.serverAnswer")}</p>
              {failure.status !== null && (
                <p className="type-small">
                  {t("models.httpStatus", { status: failure.status })}
                </p>
              )}
              {failure.excerpt && (
                <pre className="type-code check-excerpt">{failure.excerpt}</pre>
              )}
            </div>
          )}
        </div>
      )}
      {listing && !failure && listing.models.length === 0 && (
        <p className="type-small">{t("models.modelsEmpty")}</p>
      )}
      {listing && !failure && listing.models.length > 0 && (
        <div className="stack">
          {state.kind === "listed" && state.wait !== undefined && (
            <p className="type-small muted" role="status">
              {t("models.listLast", { seconds: state.wait })}
            </p>
          )}
          <p className="type-small muted">
            {t("models.modelsCount", { count: listing.models.length })}
          </p>
          {listing.truncated && (
            <p className="type-small">{t("models.modelsTruncated")}</p>
          )}
          {listing.models.length < FILTER_FROM ? (
            <ul aria-label={t("models.modelsList")} className="model-options">
              {listing.models.map((model, index) => {
                const current =
                  model.id === value ||
                  (index === 0 &&
                    !listing.models.some((item) => item.id === value));
                return (
                  <li key={model.id}>
                    <button
                      type="button"
                      className="model-option"
                      aria-current={model.id === value || undefined}
                      // The current model, else the first, takes the focus as the list opens.
                      // eslint-disable-next-line jsx-a11y/no-autofocus
                      autoFocus={current}
                      onClick={() => pick(model.id)}
                      onKeyDown={(event) => {
                        if (event.key === "Escape") {
                          event.preventDefault();
                          close();
                        }
                      }}
                    >
                      <Option model={model} current={model.id === value} />
                    </button>
                  </li>
                );
              })}
            </ul>
          ) : (
            <>
              <input
                // The filter leads the listbox (WAI-ARIA combobox with a listbox popup).
                // eslint-disable-next-line jsx-a11y/no-autofocus
                autoFocus
                className="form-control"
                type="search"
                role="combobox"
                aria-label={t("models.modelsFilter")}
                placeholder={t("models.modelsFilter")}
                aria-expanded
                aria-controls={`${base}-models`}
                aria-activedescendant={
                  active >= 0 ? optionId(active) : undefined
                }
                aria-autocomplete="list"
                value={filter}
                onChange={(event) => {
                  setFilter(event.target.value);
                  setActive(-1);
                }}
                onKeyDown={keys}
              />
              {shown.length === 0 ? (
                <p className="type-small muted">{t("models.modelsNoMatch")}</p>
              ) : (
                <ul
                  id={`${base}-models`}
                  role="listbox"
                  aria-label={t("models.modelsList")}
                  className="model-options"
                >
                  {shown.map((model, index) => (
                    // The filter owns the keyboard (aria-activedescendant), so options take clicks only.
                    // eslint-disable-next-line jsx-a11y/click-events-have-key-events
                    <li
                      key={model.id}
                      id={optionId(index)}
                      role="option"
                      aria-selected={model.id === value}
                      data-active={index === active || undefined}
                      className="model-option"
                      onClick={() => pick(model.id)}
                    >
                      <Option model={model} current={model.id === value} />
                    </li>
                  ))}
                </ul>
              )}
            </>
          )}
        </div>
      )}
    </div>
  );
}

/** One model: its id, its display name when different, and "Current" in words (UI-GUIDELINES §9). */
function Option({
  model,
  current,
}: {
  model: { id: string; display_name?: string | null };
  current: boolean;
}) {
  const t = useT();
  return (
    <>
      <code className="type-code">{model.id}</code>
      {model.display_name && model.display_name !== model.id && (
        <span className="type-small muted">{model.display_name}</span>
      )}
      {current && (
        <span className="type-small model-current">
          {t("models.currentModel")}
        </span>
      )}
    </>
  );
}

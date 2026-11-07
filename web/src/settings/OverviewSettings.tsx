"use client";

import { useQuery } from "@tanstack/react-query";
import Link from "next/link";
import { useRouter } from "next/navigation";
import { useEffect, type ReactNode } from "react";

import { useLanguage, useT } from "../i18n/I18nProvider";
import type { MessageKey, Translate } from "../i18n/translate";
import type { AdminRoute, User } from "../protocol/generated/api";
import { Button } from "../ui/Button";
import { Skeleton } from "../ui/Skeleton";

import type { OverviewApi } from "./overview";

/** The readiness checks' names, as Status shows them. */
const CHECKS: Record<string, MessageKey> = {
  database: "health.database",
  secret_store: "health.secretStore",
  backend_preflight: "health.backend",
  image_present: "health.image",
  runtime_revision: "health.runtimeRevision",
};

/** One part of the overview, loaded on its own: a failure says so on its own line and the rest still shows. */
function usePart<T>(key: string, load: (signal: AbortSignal) => Promise<T>) {
  return useQuery({
    queryKey: ["settings-overview", key],
    queryFn: ({ signal }) => load(signal),
    retry: false,
  });
}

/**
 * Settings › Overview (WEB-U-077, UI-GUIDELINES §7): where an administrator lands. Whether tasks can start, the models
 * and people at a glance, and what is missing, each with the action that fixes it. A member lands on Account instead.
 */
export function OverviewSettings({ me, api }: { me: User; api: OverviewApi }) {
  const router = useRouter();
  const admin = me.role === "admin";
  useEffect(() => {
    if (!admin) router.replace("/settings/account");
  }, [admin, router]);
  return admin ? <Overview api={api} teams={me.features.agent_teams} /> : null;
}

function Overview({ api, teams }: { api: OverviewApi; teams: boolean }) {
  const t = useT();
  const readiness = usePart("readiness", (signal) => api.readiness(signal));
  const routes = usePart("routes", (signal) => api.routes(signal));
  const people = usePart("people", (signal) => api.people(signal));
  const access = usePart("internet", (signal) => api.internetAccess(signal));
  const rotation = usePart("rotation", (signal) => api.rotationDue(signal));
  const desktop = usePart("desktop", (signal) => api.desktop(signal));
  const failing = readiness.data
    ? Object.entries(readiness.data.checks)
        .filter(([, passed]) => !passed)
        .map(([check]) => (CHECKS[check] ? t(CHECKS[check]) : check))
    : [];
  const { language } = useLanguage();
  const models = routes.data && modelsLine(t, language, routes.data);
  const missing: { text: string; action: string; href: string }[] = [];
  if (access.data && !access.data.disabled && access.data.rules === 0)
    missing.push({
      text: t("overview.noInternet"),
      action: t("overview.allowSites"),
      href: "/settings/internet-access",
    });
  // Connectors are optional: an install without any needs no attention for it (WEB-U-116, F22).
  if (rotation.data)
    missing.push({
      text: t("overview.rotationDue", { count: rotation.data }),
      action: t("overview.reviewKeys"),
      href: "/settings/keys/shared",
    });
  return (
    <div className="stack">
      <p className="type-small muted">{t("overview.intro")}</p>
      {/* §5c: the summary lines read as one table: 40 px rows, no gap between them. */}
      <div className="overview-lines">
        <Line
          state={
            readiness.data
              ? readiness.data.ready
                ? "ok"
                : "danger"
              : undefined
          }
          loading={readiness.isPending}
          failed={readiness.isError ? t("overview.statusFailed") : null}
          retry={() => void readiness.refetch()}
          link={{ text: t("overview.viewStatus"), href: "/settings/status" }}
        >
          {readiness.data &&
            (readiness.data.ready
              ? t("overview.ready", {
                  count: Object.keys(readiness.data.checks).length,
                })
              : t("overview.notReady", {
                  count: failing.length,
                  checks: failing.join(", "),
                }))}
        </Line>
        <Line
          label={t("settings.models")}
          loading={routes.isPending}
          failed={routes.isError ? t("overview.modelsFailed") : null}
          retry={() => void routes.refetch()}
          state={models?.state}
          link={{
            // While any model isn't verified, the action is to check them.
            text: t(models?.check ? "overview.check" : "overview.manage"),
            href: "/settings/models",
          }}
        >
          {models?.text}
        </Line>
        <Line
          label={t("settings.users")}
          loading={people.isPending}
          failed={people.isError ? t("overview.peopleFailed") : null}
          retry={() => void people.refetch()}
          link={{ text: t("overview.manage"), href: "/settings/people" }}
        >
          {people.data &&
            [
              t("overview.people", { count: people.data.active }),
              ...(people.data.invited
                ? [t("overview.invited", { count: people.data.invited })]
                : []),
            ].join(" · ")}
        </Line>
        {/* c16's two install-time choices, named where an administrator lands, only when the install has them: the
            Desktop (built with its image) and agent teams (their switch). Never a line for what isn't there (WEB-U-318). */}
        {desktop.data && (
          <Line
            label={t("overview.desktop")}
            loading={false}
            failed={null}
            link={{
              text: t("settings.environments"),
              href: "/settings/environments",
            }}
          >
            {t("overview.desktopOn")}
          </Line>
        )}
        {teams && (
          <Line
            label={t("overview.teams")}
            loading={false}
            failed={null}
            link={{
              text: t("overview.helperLimits"),
              href: "/settings/limits",
            }}
          >
            {t("overview.teamsOn")}
          </Line>
        )}
      </div>
      {missing.length > 0 && (
        <section
          className="stack overview-attention"
          aria-label={t("overview.attention")}
        >
          <h2 className="type-heading">{t("overview.attention")}</h2>
          <ul className="overview-list">
            {missing.map((item) => (
              <li key={item.href}>
                <span>{item.text}</span>
                <Link
                  href={item.href}
                  className="button"
                  data-variant="secondary"
                >
                  {item.action}
                </Link>
              </li>
            ))}
          </ul>
        </section>
      )}
    </div>
  );
}

/** One line of the overview: its label, its sentence (or why it couldn't load) and the link to its page. */
/** Each check state's count, in the order the card lists them: the most urgent first, verified last. */
const MODEL_STATES: readonly [AdminRoute["probe"]["state"], MessageKey][] = [
  ["failed", "overview.modelsCheckFailed"],
  ["stale", "overview.modelsOutOfDate"],
  ["never", "overview.modelsNotChecked"],
  ["verified", "overview.modelsVerified"],
];

/**
 * The Models card, saying what's true (GW-I-140): one state alone by itself ("1 check failed", "All 2 verified"),
 * several counted, most urgent first ("2 models: 1 check failed, 1 verified"), joined as the reader's language lists
 * things ("、" in Chinese). A failed check is a warning; any model not verified makes
 * the action a check.
 */
export function modelsLine(
  t: Translate,
  language: string,
  routes: readonly AdminRoute[],
): { text: string; state: "ok" | "warn" | undefined; check: boolean } {
  const counts = MODEL_STATES.map(
    ([state, key]) =>
      [
        state,
        key,
        routes.filter((route) => route.probe.state === state).length,
      ] as const,
  ).filter(([, , count]) => count > 0);
  const count = (state: AdminRoute["probe"]["state"]) =>
    counts.find(([found]) => found === state)?.[2] ?? 0;
  const verified = count("verified");
  const state = count("failed")
    ? "warn"
    : routes.length && verified === routes.length
      ? "ok"
      : undefined;
  const check = verified < routes.length;
  const [only] = counts;
  if (!only) return { text: t("overview.modelsEmpty"), state, check };
  if (counts.length === 1) {
    const [found, key, n] = only;
    const text =
      found === "verified" && n > 1
        ? t("overview.modelsAllVerified", { count: n })
        : t(key, { count: n });
    return { text, state, check };
  }
  const states = new Intl.ListFormat(language, {
    type: "conjunction",
    style: "narrow",
  }).format(counts.map(([, key, n]) => t(key, { count: n })));
  return {
    text: t("overview.modelsMixed", { count: routes.length, states }),
    state,
    check,
  };
}

function Line({
  label,
  state,
  loading,
  failed,
  retry,
  link,
  children,
}: {
  label?: string;
  state?: "ok" | "warn" | "danger" | undefined;
  loading: boolean;
  failed: string | null;
  /** Reads the line again after a failure, as the other settings pages' Try again (WEB-U-360). */
  retry?: () => void;
  link: { text: string; href: string };
  children: ReactNode;
}) {
  const t = useT();
  return (
    <div className="overview-line" data-state={state}>
      {label && <span className="type-label muted">{label}</span>}
      {loading ? (
        <Skeleton label={t("skeleton.loading")} />
      ) : failed ? (
        <span className="overview-failed">
          <span className="muted">{failed}</span>
          {retry && (
            <Button variant="ghost" onClick={retry}>
              {t("common.tryAgain")}
            </Button>
          )}
        </span>
      ) : (
        <span>
          {state && <span className="lease-dot" aria-hidden="true" />}
          {children}
        </span>
      )}
      <Link href={link.href} className="type-small">
        {link.text}
      </Link>
    </div>
  );
}

"use client";

import { useQuery, useQueryClient } from "@tanstack/react-query";
import { ArrowLeft } from "lucide-react";
import Link from "next/link";
import { useRouter } from "next/navigation";
import { useEffect, useState } from "react";

import { useT } from "../i18n/I18nProvider";
import type { User } from "../protocol/generated/api";
import { Icon } from "../ui/Icon";
import { Skeleton } from "../ui/Skeleton";

import type { RouteItem, RoutesApi } from "./admin/routes";
import { LoadFailure } from "./LoadFailure";
import { routeLabel } from "./route-label";
import { RouteForm, type Preset } from "./RouteForm";

const LIST = "/settings/models";
/** The routes list's query, shared by the list and each route's page. */
export const ROUTES_KEY = ["admin-routes"];
/** What the list says when a page sends the administrator back to it ("Saved route …"). */
export const ROUTE_NOTICE_KEY = ["admin-routes-notice"];
/** A route just added: the list offers to check it, beside its notice (the Models page review, P2 #6). */
export const ROUTE_CHECK_NEXT_KEY = ["admin-routes-check-next"];
/** How the list asks a route's page to open: with Paste key ready, or a check's suggested setting applied. */
export const ROUTE_OPEN_KEY = ["admin-routes-open"];
export type RouteOpening = { name: string; paste?: boolean; preset?: Preset };

/** Adds (`name` null) or edits a model route, as a page of its own (WEB-U-115, the first hour's F9). */
export function RoutePage({
  me,
  api,
  name,
}: {
  me: User;
  api: RoutesApi;
  name: string | null;
}) {
  const t = useT();
  if (me.role !== "admin")
    return <p className="muted">{t("models.adminsOnly")}</p>;
  return <Route api={api} name={name} />;
}

function Route({ api, name }: { api: RoutesApi; name: string | null }) {
  const t = useT();
  const cache = useQueryClient();
  const router = useRouter();
  const list = useQuery({
    queryKey: ROUTES_KEY,
    queryFn: ({ signal }) => api.routes(signal),
    retry: false,
  });
  const secretsQuery = {
    queryKey: ["admin-secret-refs"],
    queryFn: ({ signal }: { signal: AbortSignal }) => api.secretRefs(signal),
    retry: false,
  } as const;
  const secrets = useQuery(secretsQuery);
  // How the list asked this page to open, if it did: taken once.
  const [opening] = useState(() => {
    const asked = cache.getQueryData<RouteOpening>(ROUTE_OPEN_KEY);
    return asked && asked.name === name ? asked : null;
  });
  useEffect(() => {
    cache.removeQueries({ queryKey: ROUTE_OPEN_KEY, exact: true });
  }, [cache]);
  if (list.isPending) return <Skeleton label={t("models.loading")} />;
  if (list.isError)
    return (
      <LoadFailure
        message={t("models.failed")}
        onRetry={() => void list.refetch()}
      />
    );
  const items = list.data.items;
  const item: RouteItem | null | undefined =
    name === null ? null : items.find((each) => each.name === name);
  if (item === undefined)
    return (
      <div className="stack settings-form-page">
        <Link href={LIST} className="back-link type-small">
          <Icon glyph={ArrowLeft} size={16} />
          {t("settings.models")}
        </Link>
        <p>{t("models.notFound")}</p>
      </div>
    );
  return (
    <RouteForm
      item={item}
      names={items.map((each) => each.name)}
      labels={Object.fromEntries(
        items.map((each) => [each.name, routeLabel(each)]),
      )}
      secretRefs={secrets.data ?? []}
      loadSecretRefs={() => cache.ensureQueryData(secretsQuery)}
      api={api}
      pasteOpen={opening?.paste ?? false}
      {...(opening?.preset ? { preset: opening.preset } : {})}
      onStale={async (stale) =>
        (await list.refetch()).data?.items.find((each) => each.name === stale)
      }
      onSaved={(saved) => {
        cache.setQueryData(
          ROUTE_NOTICE_KEY,
          t(name === null ? "models.savedNew" : "models.saved", {
            name: routeLabel(saved),
          }),
        );
        if (name === null) cache.setQueryData(ROUTE_CHECK_NEXT_KEY, saved.name);
        void cache.invalidateQueries({ queryKey: ROUTES_KEY });
        router.push(LIST);
      }}
    />
  );
}

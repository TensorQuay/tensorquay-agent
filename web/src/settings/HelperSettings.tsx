"use client";

import { useQuery } from "@tanstack/react-query";
import { useState, type FormEvent } from "react";

import { newActionKey } from "../api/action-key";
import { ApiError } from "../api/error";
import { useT } from "../i18n/I18nProvider";
import type { MessageKey } from "../i18n/translate";
import { Button } from "../ui/Button";
import { Dialog } from "../ui/Dialog";
import { FormRefusal } from "../ui/FormRefusal";
import { Input } from "../ui/Input";
import { Select } from "../ui/Select";
import { Skeleton } from "../ui/Skeleton";

import type { LimitsApi, TeamSettings } from "./admin/limits";
import type { RouteItem } from "./admin/routes";
import { LoadFailure } from "./LoadFailure";

export const TEAM_SETTINGS_KEY = ["admin-agent-teams"];
/** The Models page's own cache key, so both pages share one read of the routes. */
const ROUTES_KEY = ["admin-routes"];
/** The gateway's bound on the acceptance check (ADR-0034 S6). */
const MAX_CHECK_BYTES = 4096;
/** The lead task's own model: the helpers' model when none is set. */
const LEAD = "";

export type Setting = "member_route" | "acceptance_check";
const NAMES: Record<Setting, MessageKey> = {
  member_route: "limits.memberRoute",
  acceptance_check: "limits.acceptanceCheck",
};

const routeLabel = (routes: RouteItem[] | undefined, name: string) =>
  routes?.find((item) => item.name === name)?.display_name ?? name;

/** A setting's value in words: a route by its display name, a command as written, or what applies without one. */
function shown(
  t: ReturnType<typeof useT>,
  settings: TeamSettings,
  setting: Setting,
  routes: RouteItem[] | undefined,
) {
  if (setting === "member_route")
    return settings.member_route === null
      ? t("limits.leadModel")
      : routeLabel(routes, settings.member_route);
  return settings.acceptance_check ?? t("limits.noCheck");
}

/** The helpers' settings and the routes their model is chosen from, read only while agent teams are on. */
export function useHelperSettings(api: LimitsApi) {
  const settings = useQuery({
    queryKey: TEAM_SETTINGS_KEY,
    queryFn: ({ signal }) => api.agentTeams(signal),
    retry: false,
  });
  const routes = useQuery({
    queryKey: ROUTES_KEY,
    queryFn: ({ signal }) => api.routes(signal),
    retry: false,
  });
  return { settings, routes: routes.data?.items };
}

/**
 * The helpers' two settings that aren't numbers (ADR-0034 S6), as the last rows of the helper limits: the helpers'
 * model and the check run when the lead says the work is done, each edited on its own.
 */
export function HelperSettingRows({
  read,
  onEdit,
}: {
  read: ReturnType<typeof useHelperSettings>;
  onEdit: (setting: Setting) => void;
}) {
  const t = useT();
  const { settings, routes } = read;
  if (settings.isPending)
    return (
      <tr>
        <td colSpan={3}>
          <Skeleton label={t("limits.teamSettingsLoading")} />
        </td>
      </tr>
    );
  if (settings.isError)
    return (
      <tr>
        <td colSpan={3}>
          <LoadFailure
            message={t("limits.teamSettingsFailed")}
            onRetry={() => void settings.refetch()}
          />
        </td>
      </tr>
    );
  return (
    <>
      {(["member_route", "acceptance_check"] as const).map((setting) => {
        const name = t(NAMES[setting]);
        const code =
          setting === "acceptance_check" &&
          settings.data.acceptance_check !== null;
        return (
          <tr key={setting}>
            <td>{name}</td>
            <td>
              <span className={code ? "type-code" : undefined}>
                {shown(t, settings.data, setting, routes)}
              </span>
            </td>
            <td className="data-cell-action">
              <Button
                variant="ghost"
                aria-label={t("limits.editName", { name })}
                onClick={() => onEdit(setting)}
              >
                {t("limits.edit")}
              </Button>
            </td>
          </tr>
        );
      })}
    </>
  );
}

/** Edits one setting with the version read, then says what was saved; outside the table, as every dialog. */
export function HelperSettingEdit({
  setting,
  read,
  api,
  onClose,
  onSaved,
}: {
  setting: Setting;
  read: ReturnType<typeof useHelperSettings>;
  api: LimitsApi;
  onClose: () => void;
  onSaved: (copy: string) => void;
}) {
  const t = useT();
  const { settings, routes } = read;
  if (!settings.data) return null;
  return (
    <HelperSettingForm
      setting={setting}
      settings={settings.data}
      routes={routes ?? []}
      api={api}
      onClose={onClose}
      onReload={async () => (await settings.refetch()).data}
      onSaved={(saved) =>
        onSaved(
          t("limits.saved", {
            name: t(NAMES[setting]),
            value: shown(t, saved, setting, routes),
          }),
        )
      }
    />
  );
}

/** The gateway's refusal of a write as one sentence. */
function refusal(error: unknown): MessageKey {
  if (!(error instanceof ApiError)) return "limits.saveFailed";
  if (error.code === "conflict") return "limits.settingConflict";
  if (error.code === "unknown_reference") return "limits.routeUnknown";
  if (error.code === "teams_disabled") return "limits.teamsTurnedOff";
  if (error.status === 400) return "limits.checkInvalid";
  return "limits.saveFailed";
}

/** Edits one setting; the other is sent as read. The check is one line, trimmed, and empty means no check. */
function HelperSettingForm({
  setting,
  settings,
  routes,
  api,
  onSaved,
  onReload,
  onClose,
}: {
  setting: Setting;
  settings: TeamSettings;
  routes: RouteItem[];
  api: LimitsApi;
  onSaved: (saved: TeamSettings) => void;
  onReload: () => Promise<TeamSettings | undefined>;
  onClose: () => void;
}) {
  const t = useT();
  const [current, setCurrent] = useState(settings);
  const [route, setRoute] = useState(settings.member_route ?? LEAD);
  const [check, setCheck] = useState(settings.acceptance_check ?? "");
  const [problem, setProblem] = useState<MessageKey | null>(null);
  const [stale, setStale] = useState(false);
  const [pending, setPending] = useState(false);
  const name = t(NAMES[setting]);
  async function submit(event: FormEvent) {
    event.preventDefault();
    const command = check.trim();
    if (
      setting === "acceptance_check" &&
      (/[\r\n]/.test(command) ||
        new TextEncoder().encode(command).length > MAX_CHECK_BYTES)
    ) {
      setProblem("limits.checkInvalid");
      return;
    }
    setProblem(null);
    setPending(true);
    try {
      onSaved(
        await api.putAgentTeams(
          {
            member_route:
              setting === "member_route"
                ? route === LEAD
                  ? null
                  : route
                : current.member_route,
            acceptance_check:
              setting === "acceptance_check"
                ? command || null
                : current.acceptance_check,
            version: current.version,
          },
          newActionKey(),
        ),
      );
    } catch (error) {
      setProblem(refusal(error));
      setStale(error instanceof ApiError && error.code === "conflict");
    } finally {
      setPending(false);
    }
  }
  async function reload() {
    const fresh = await onReload();
    if (!fresh) return;
    setCurrent(fresh);
    setRoute(fresh.member_route ?? LEAD);
    setCheck(fresh.acceptance_check ?? "");
    setProblem(null);
    setStale(false);
  }
  const words = problem ? t(problem, { bytes: MAX_CHECK_BYTES }) : undefined;
  return (
    <Dialog
      open
      title={t("limits.editTitle", { name })}
      onClose={() => {
        if (!pending) onClose();
      }}
    >
      <form
        className="stack"
        noValidate
        onSubmit={(event) => void submit(event)}
      >
        {setting === "member_route" ? (
          <Select label={name} value={route} onChange={setRoute} error={words}>
            <option value={LEAD}>{t("limits.leadModel")}</option>
            {/* A stored route the list no longer has is still shown, so the choice never reads as another. */}
            {route !== LEAD && !routes.some((item) => item.name === route) && (
              <option value={route}>{route}</option>
            )}
            {routes.map((item) => (
              <option key={item.name} value={item.name}>
                {item.display_name ?? item.name}
              </option>
            ))}
          </Select>
        ) : (
          <Input
            label={t("limits.checkCommand")}
            className="type-code"
            spellCheck={false}
            autoComplete="off"
            value={check}
            onChange={(event) => setCheck(event.target.value)}
            aria-describedby="helper-check-hint"
            {...(words ? { error: words } : {})}
          />
        )}
        <p id="helper-check-hint" className="type-small muted">
          {t(
            setting === "member_route"
              ? "limits.memberRouteHint"
              : "limits.checkHint",
          )}
        </p>
        <FormRefusal />
        <div className="dialog-actions">
          {stale && (
            <Button
              variant="secondary"
              type="button"
              onClick={() => void reload()}
            >
              {t("limits.reload")}
            </Button>
          )}
          <Button
            variant="secondary"
            type="button"
            disabled={pending}
            onClick={onClose}
          >
            {t("common.cancel")}
          </Button>
          <Button type="submit" loading={pending}>
            {t("limits.saveSetting")}
          </Button>
        </div>
      </form>
    </Dialog>
  );
}

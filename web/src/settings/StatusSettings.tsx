"use client";

import { useQuery } from "@tanstack/react-query";
import Link from "next/link";
import { useId } from "react";

import { useT } from "../i18n/I18nProvider";
import type { MessageKey } from "../i18n/translate";
import type { ReadyDetail, User } from "../protocol/generated/api";
import { Button } from "../ui/Button";
import { Skeleton } from "../ui/Skeleton";

import type { TemplatesApi } from "./admin/templates";
import { unavailableKey } from "./environment-copy";
import { settingsApi } from "./requests";

/**
 * The readiness checks an administrator can act on, in reading order, each with what its failure means and what to
 * do, as the gateway runs them (the database, the key store's refs, the engine's preflight, the runtime image and its
 * protocol revision). Others still count toward whether new tasks can start.
 */
const CHECKS: readonly { key: string; label: MessageKey; fix: MessageKey }[] = [
  { key: "database", label: "health.database", fix: "health.fixDatabase" },
  {
    key: "secret_store",
    label: "health.secretStore",
    fix: "health.fixSecretStore",
  },
  {
    key: "backend_preflight",
    label: "health.backend",
    fix: "health.fixBackend",
  },
  { key: "image_present", label: "health.image", fix: "health.fixImage" },
  {
    key: "runtime_revision",
    label: "health.runtimeRevision",
    fix: "health.fixRevision",
  },
];

/** How often an open Status page reads the readiness detail again. */
export const STATUS_REFRESH_MS = 30_000;

/** Runtime status for administrators (FR-22): the gateway's readiness checks and its sandbox backend. */
export function StatusSettings({
  me,
  environments,
}: {
  me: User;
  /** The environments list, for the default's display name (readiness names it by its slug only). */
  environments?: TemplatesApi;
}) {
  const t = useT();
  if (me.role !== "admin")
    return <p className="muted">{t("health.adminsOnly")}</p>;
  return <Status environments={environments} />;
}

function Status({ environments }: { environments?: TemplatesApi | undefined }) {
  const t = useT();
  const status = useQuery({
    queryKey: ["readiness"],
    queryFn: ({ signal }) => settingsApi.readiness(signal),
    retry: false,
    // Read again while the page is open and visible, so "Checked at" and the tasks working on a step stay current
    // while an administrator waits for a gap to restart in (WEB-U-292); a hidden tab doesn't poll.
    refetchInterval: STATUS_REFRESH_MS,
  });
  if (status.isPending) return <Skeleton label={t("health.loading")} />;
  // Only a page with no read yet shows the failure alone: after a good read, a failed one keeps that read on screen with
  // a line saying so, so the live lines stay the same elements and the next good read is heard.
  if (status.isError && !status.data)
    return (
      <div className="stack">
        <p>{t("health.failed")}</p>
        <Button variant="secondary" onClick={() => void status.refetch()}>
          {t("common.tryAgain")}
        </Button>
      </div>
    );
  const { checks, sandbox, templates } = status.data;
  const microVm = sandbox.backend === "firecracker";
  const failed = CHECKS.filter(({ key }) => checks[key] !== true);
  const passed = CHECKS.filter(({ key }) => checks[key] === true);
  // A check this page doesn't name still counts: "can start" only when the gateway says ready.
  const ready = status.data.ready;
  const checkedAt = new Intl.DateTimeFormat(t.locale, {
    timeStyle: "short",
  }).format(new Date(status.dataUpdatedAt));
  return (
    <div className="stack">
      <p className="type-small muted">{t("health.intro")}</p>
      {/* The one line first: can new tasks start right now. */}
      <div className="stack status-verdict">
        {/* Polite and live, so a change after Check again or a 30 s read is heard; a read that leaves it as it was says
            nothing (an accessibility review before 17.0). Checked at, below, isn't live. */}
        <p
          role="status"
          className="type-body status-verdict-line"
          data-tone={ready ? "ok" : "danger"}
        >
          {ready
            ? t("health.canStart")
            : t("health.cannotStart", { count: Math.max(failed.length, 1) })}
        </p>
        <p className="type-small muted">
          {t("health.checkedAt", { time: checkedAt })}
        </p>
        {/* Always there and live, empty while reads succeed, so a failed read is heard when it happens. */}
        {/* Quiet: an older read is shown, nothing is wrong with the installation (WEB-U-361). */}
        <p role="status" className="type-small muted">
          {status.isError ? t("health.readFailed") : null}
        </p>
      </div>
      {/* Only a failing check is explained, with what to do. */}
      {failed.map(({ key, label, fix }) => (
        <section
          key={key}
          className="stack status-problem"
          aria-labelledby={`status-${key}`}
        >
          <h2 id={`status-${key}`} className="type-heading">
            {t(label)}
          </h2>
          <p>
            {t(fix, {
              engine: microVm ? "microvm" : "docker",
            })}
          </p>
        </section>
      ))}
      {passed.length > 0 && (
        <p className="type-small muted">
          {t("health.passing", {
            checks: passed.map(({ label }) => t(label)).join(", "),
          })}
        </p>
      )}
      <p className="type-small muted">
        {t(microVm ? "health.runsMicroVm" : "health.runsDocker")}
      </p>
      {templates.default !== null && (
        <DefaultEnvironment
          {...templates}
          environments={environments}
          ready={ready}
          problems={failed.length}
        />
      )}
      <KeptGitCredentials kept={status.data.git_credentials_kept} />
      <ActiveRuns active={status.data.runs_active} />
      <div>
        <Button
          variant="secondary"
          loading={status.isFetching}
          onClick={() => void status.refetch()}
        >
          {t("health.checkAgain")}
        </Button>
      </div>
    </div>
  );
}

/**
 * How many tasks are working on a step (GW-I-229), so an administrator restarts in a gap: a restart starts each of those
 * steps over (a first-hour check; the admin guide's Restarting the gateway). A count only, never which tasks or whose;
 * nothing when it couldn't be read.
 */
function ActiveRuns({ active }: { active: ReadyDetail["runs_active"] }) {
  const t = useT();
  // Named once, by its heading (an accessibility review before 17.0).
  const heading = useId();
  if (active === null) return null;
  return (
    <section className="stack" aria-labelledby={heading}>
      <h2 id={heading} className="type-heading">
        {t("health.activeTitle")}
      </h2>
      {/* A polite live line, so a screen reader hears the count when it changes while someone waits for a gap; the 30 s
          reads that leave it as it was say nothing, and "Checked at" above stays out of it (WEB-U-297). */}
      <p role="status">
        {active === 0
          ? t("health.activeNone")
          : t("health.activeSome", { count: active })}
      </p>
    </section>
  );
}

/**
 * Git credentials an earlier release stored that couldn't become keys (ADR-0037 §7), kept unused: how many, and what
 * to do. Diagnostics, never "Not ready"; nothing when there are none or the count couldn't be read.
 */
function KeptGitCredentials({
  kept,
}: {
  kept: ReadyDetail["git_credentials_kept"];
}) {
  const t = useT();
  const heading = useId();
  if (kept === null || kept.not_web_address + kept.waiting === 0) return null;
  return (
    <section className="stack" aria-labelledby={heading}>
      <h2 id={heading} className="type-heading">
        {t("health.gitKeptTitle")}
      </h2>
      {kept.not_web_address > 0 && (
        <p>{t("health.gitKeptAddress", { count: kept.not_web_address })}</p>
      )}
      {kept.waiting > 0 && (
        <p>{t("health.gitKeptWaiting", { count: kept.waiting })}</p>
      )}
    </section>
  );
}

/** The default environment (ADR-0032): whether new tasks can start in it, and why not. Diagnostics, never "Not ready". */
function DefaultEnvironment({
  default: name,
  default_offered: offered,
  default_reason: reason,
  checks_pending: pending,
  environments,
  ready,
  problems,
}: ReadyDetail["templates"] & {
  environments?: TemplatesApi | undefined;
  /** Whether new tasks can start: until they can, an offered default isn't "Available" yet (WEB-U-174). */
  ready: boolean;
  /** The problems the page lists above; none when the failing check is one it doesn't name. */
  problems: number;
}) {
  const t = useT();
  // The Environments page's own query, so both pages share it; without it the slug stands in.
  const list = useQuery({
    queryKey: ["admin-templates"],
    queryFn: ({ signal }) =>
      (environments as TemplatesApi).environments(signal),
    enabled: Boolean(environments),
    retry: false,
  });
  const display = list.data?.items.find(
    (item) => item.name === name,
  )?.display_name;
  return (
    <section className="stack" aria-labelledby="status-default-environment">
      <h2 id="status-default-environment" className="type-heading">
        {t("health.defaultEnvironment")}
      </h2>
      <p className="environment-pills">
        {display ? (
          <span>{display}</span>
        ) : (
          <code className="type-code">{name}</code>
        )}
        <span
          className="status-pill type-label"
          data-tone={!offered ? "danger" : ready ? "ok" : "muted"}
        >
          {!offered
            ? t("health.notAvailable")
            : ready
              ? t("health.available")
              : problems > 0
                ? t("health.defaultWaiting", { count: problems })
                : t("health.defaultWaitingUnlisted")}
        </span>
      </p>
      {!offered && (
        <>
          <p className="type-small">{t(unavailableKey(reason))}</p>
          <p className="type-small">
            <Link href="/settings/environments">
              {t("health.openEnvironments")}
            </Link>
          </p>
        </>
      )}
      {pending !== null && pending > 0 && (
        <p className="type-small muted">
          {t("health.checksPending", { count: pending })}
        </p>
      )}
    </section>
  );
}

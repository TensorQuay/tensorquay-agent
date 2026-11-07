"use client";

import { Bot, FolderOpen, X } from "lucide-react";
import Link from "next/link";
import { useEffect, useId, useRef, useState } from "react";

import { useSession } from "../auth/use-session";
import { useAttachments } from "../files/use-attachments";
import { useT } from "../i18n/I18nProvider";
import { APP_NAME } from "../lib/app";
import { TabTitle } from "../shell/TabTitle";
import { requestFailureMessage } from "../transcript/action-failure";
import { Composer } from "../transcript/Composer";
import { OptimisticMessage } from "../transcript/OptimisticMessage";
import { Button } from "../ui/Button";
import { ErrorMessage } from "../ui/ErrorMessage";
import { IconAction } from "../ui/IconAction";
import { MenuButton } from "../ui/MenuButton";
import { useWorkspaces } from "../workspaces/use-workspaces";
import { workspaceName } from "../workspaces/workspace-state";
import { WorkspacePicker } from "../workspaces/WorkspacePicker";

import { ConversationBar } from "./ConversationBar";
import { EnvironmentPicker } from "./EnvironmentPicker";
import { defaultRoute } from "./model-labels";
import { ModelPicker } from "./ModelPicker";
import { useFirstMessage } from "./use-first-message";
import { useModels } from "./use-models";
import { useProfiles } from "./use-profiles";
import { useTemplates } from "./use-templates";
import "../i18n/deferred-catalogue";

/**
 * Reads models, environments and workspaces; its first send creates the conversation (on the chosen workspace, or a
 * new one) and posts the prompt. `workspace` preselects one ("New task" on a workspace, a fork's toast).
 */
export function NewConversation({
  workspace: initialWorkspace = "",
}: {
  workspace?: string | undefined;
} = {}) {
  const session = useSession();
  const admin = session.data?.role === "admin";
  const teamsOn = session.data?.features.agent_teams ?? false;
  const workspaces = useWorkspaces();
  const [workspace, setWorkspace] = useState(initialWorkspace);
  const chosen = workspaces.data?.items.find((item) => item.id === workspace);
  // Asked for beside the session, not after it: one round trip before "+" can offer a profile, not two (WEB-U-196). A
  // signed-out answer is a 401, which goes to sign-in as the session's own does.
  const models = useModels();
  const templates = useTemplates();
  const profiles = useProfiles();
  const [selectedProfile, setProfile] = useState("");
  // A profile fixes the route and environment it names (ADR-0012); the pickers show them, disabled, with the reason.
  const profile =
    profiles.usable.find((item) => item.id === selectedProfile) ?? null;
  const [selectedTemplate, setTemplate] = useState("");
  // The list holds only offered environments, the default first; a choice that left it falls back (ADR-0032).
  const offered = templates.data?.items;
  const template =
    profile?.template ??
    (offered?.some((item) => item.name === selectedTemplate)
      ? selectedTemplate
      : offered?.[0]?.name) ??
    "";
  const [draft, setDraft] = useState("");
  const container = useRef<HTMLElement>(null);
  const t = useT();
  const [selected, setSelected] = useState("");
  const route =
    profile?.model_route ?? (selected || defaultRoute(models.data?.items));
  const setBy = profile
    ? t("picker.setByProfile", { name: profile.display_name })
    : undefined;
  const action = useFirstMessage(route, template, workspace, profile);
  // A Desktop with a route that can't see images leaves the agent blind on the screen (16e's first hour, 8b): said
  // where the person picks them (WEB-U-370).
  const blind =
    Boolean(
      offered
        ?.find((item) => item.name === template)
        ?.capabilities.includes("desktop"),
    ) &&
    models.data?.items.find((item) => item.name === route)?.vision === false;
  // §3 step 1b: files chosen here are held, then uploaded to the task once it exists.
  const attachments = useAttachments(
    null,
    undefined,
    session.data?.limits.upload_bytes,
  );
  const error =
    session.error ??
    models.error ??
    (templates.isError ? t("controls.environmentsFailed") : null);
  const copying = chosen?.state === "forking";
  const ready = Boolean(route && template) && !copying;
  const failed = session.isError || models.isError || templates.isError;
  const loading = !failed && !(models.data && templates.data);
  // A prompt sent before both catalogues arrive is held visibly and sent once they do.
  const [held, setHeld] = useState<string | null>(null);
  async function send(text: string) {
    setDraft("");
    if (!ready) {
      setHeld(text);
      return true;
    }
    const sent = await action.send(
      text,
      attachments.items.length > 0 ? attachments.uploadHeld : undefined,
    );
    if (!sent) setDraft(text);
    else attachments.clear();
    return sent;
  }
  const released = useRef<string | null>(null);
  // Adjusted while rendering: the action takes over a released prompt, and loading that ends without a route or
  // environment returns the prompt to the draft beside the visible error.
  if (held !== null && (action.text === held || (!loading && !ready))) {
    setHeld(null);
    if (action.text !== held) setDraft(held);
  }
  useEffect(() => {
    if (held === null || loading || !ready || released.current === held) return;
    released.current = held;
    const text = held;
    // A prompt held for the catalogues takes its files along too.
    void action
      .send(
        text,
        attachments.items.length > 0 ? attachments.uploadHeld : undefined,
      )
      .then((sent) => {
        if (!sent) setDraft(text);
        else attachments.clear();
      });
  }, [action, attachments, held, loading, ready]);
  const shown = action.text ?? held;
  // The UI standard's composer on New task (step 2 B2): the model and environment are its pickers; a workspace and an
  // agent profile come from its "+" as chips, each opening its picker at once and going back to the default by its ×.
  const id = useId();
  const [addingWorkspace, setAddingWorkspace] = useState(false);
  const [addingProfile, setAddingProfile] = useState(false);
  const showWorkspace = workspace !== "" || addingWorkspace;
  const showProfile = selectedProfile !== "" || addingProfile;
  const busy = Boolean(chosen?.lease) && !copying;
  /** What New task can't start without, in the order the composer says it. */
  const missing =
    models.data?.items.length === 0
      ? "models"
      : templates.data && !template
        ? "environments"
        : null;
  return (
    <section
      className="conversation-page conversation-start"
      aria-label={t("start.region")}
      ref={container}
    >
      <TabTitle>{t("tab.newTask", { product: APP_NAME })}</TabTitle>
      <ConversationBar
        title={t("shell.newTask")}
        // The model and environment in the top bar, as on a task's header (the owner's correction, WEB-U-113).
        choices={
          <span
            role="group"
            aria-label={t("controls.choices")}
            className="header-choices"
          >
            <span className="choice-row">
              <ModelPicker
                placement="below-end"
                models={models.data?.items}
                route={route}
                onChange={setSelected}
                disabled={action.pending || Boolean(profile?.model_route)}
                disabledReason={profile?.model_route ? setBy : undefined}
                unavailable={models.isError}
              />
            </span>
            <span className="choice-row">
              <EnvironmentPicker
                placement="below-end"
                choices={templates.choices}
                value={template}
                onChange={setTemplate}
                disabled={action.pending || Boolean(profile?.template)}
                disabledReason={profile?.template ? setBy : undefined}
              />
            </span>
          </span>
        }
      />
      <div className={`start-body ${shown ? "has-prompt" : ""}`}>
        {!shown && <h2 className="type-display">{t("start.greeting")}</h2>}
        {shown && (
          <div className="start-prompt">
            <OptimisticMessage
              text={shown}
              failed={Boolean(action.error)}
              message={
                action.error ? requestFailureMessage(action.error) : undefined
              }
              onRetry={() => {
                if (action.text) void send(action.text);
              }}
            />
          </div>
        )}
        <ErrorMessage error={error} />
        {(session.isError || models.isError || templates.isError) && (
          <Button
            variant="secondary"
            onClick={() => {
              void (session.isError
                ? session.refetch()
                : models.isError
                  ? models.refetch()
                  : templates.refetch());
            }}
          >
            {t("common.tryAgain")}
          </Button>
        )}
        <Composer
          initial={!shown}
          attachments={attachments}
          value={draft}
          onChange={setDraft}
          run={{ id: null, status: "idle" }}
          sandbox="ready"
          sending={action.pending || held !== null}
          cancelling={false}
          onSend={send}
          onStop={() => Promise.resolve()}
          canSend={ready || (loading && !copying)}
          more={[
            {
              id: "workspace",
              label: t("start.addWorkspace"),
              icon: FolderOpen,
              onSelect: () => setAddingWorkspace(true),
            },
            ...(profiles.usable.length > 0
              ? [
                  {
                    id: "profile",
                    label: t("start.addProfile"),
                    icon: Bot,
                    onSelect: () => setAddingProfile(true),
                  },
                ]
              : []),
          ]}
          chips={
            showWorkspace || showProfile ? (
              <div
                className="composer-chips"
                role="group"
                aria-label={t("start.choices")}
              >
                {showWorkspace && (
                  // Its status sits right under the chip it is about (WEB-U-107).
                  <span className="composer-chip-item">
                    <span className="composer-chip">
                      <WorkspacePicker
                        workspaces={workspaces.data?.items}
                        value={workspace}
                        onChange={(value) => {
                          setWorkspace(value);
                          setAddingWorkspace(false);
                        }}
                        disabled={action.pending}
                        describedBy={[
                          `${id}-workspace-hint`,
                          ...(busy ? [`${id}-workspace-busy`] : []),
                        ].join(" ")}
                        defaultOpen={addingWorkspace}
                        onDismiss={() => setAddingWorkspace(false)}
                      />
                      {/* Finding N2: what a workspace is, for someone who has never met one. */}
                      <span hidden id={`${id}-workspace-hint`}>
                        {t("start.workspaceHint")}
                      </span>
                      <IconAction
                        glyph={X}
                        name={t("start.removeWorkspace", {
                          name: chosen
                            ? workspaceName(chosen)
                            : t("workspaces.new"),
                        })}
                        tip={t("start.removeWorkspace", {
                          name: chosen
                            ? workspaceName(chosen)
                            : t("workspaces.new"),
                        })}
                        onClick={() => {
                          setWorkspace("");
                          setAddingWorkspace(false);
                        }}
                      />
                    </span>
                    {busy && (
                      <span
                        className="type-small muted composer-chip-note"
                        id={`${id}-workspace-busy`}
                      >
                        {t("start.workspaceBusy")}
                      </span>
                    )}
                  </span>
                )}
                {showProfile && (
                  <span className="composer-chip">
                    <MenuButton
                      label={t("picker.profile")}
                      value={selectedProfile || "none"}
                      onChange={(value) => {
                        setProfile(value === "none" ? "" : value);
                        setAddingProfile(false);
                      }}
                      disabled={action.pending}
                      defaultOpen={addingProfile}
                      onDismiss={() => setAddingProfile(false)}
                      options={[
                        { id: "none", label: t("picker.noProfile") },
                        ...profiles.usable.map((item) => ({
                          id: item.id,
                          label: item.display_name,
                          ...(item.description
                            ? {
                                detail: (
                                  <span className="type-small muted">
                                    {item.description}
                                  </span>
                                ),
                              }
                            : {}),
                        })),
                      ]}
                    />
                    <IconAction
                      glyph={X}
                      name={t("start.removeProfile", {
                        name: profile?.display_name ?? t("picker.noProfile"),
                      })}
                      tip={t("start.removeProfile", {
                        name: profile?.display_name ?? t("picker.noProfile"),
                      })}
                      onClick={() => {
                        setProfile("");
                        setAddingProfile(false);
                      }}
                    />
                  </span>
                )}
              </div>
            ) : null
          }
          disabledReason={
            copying
              ? t("start.workspaceCopying")
              : missing === "models"
                ? t(admin ? "start.noModelsAdmin" : "start.noModels")
                : missing === "environments"
                  ? t(
                      admin
                        ? "start.noEnvironmentsAdmin"
                        : "start.noEnvironments",
                    )
                  : null
          }
        />
        {/* The administrator adds a model or looks at the environments themselves, so is shown the way, never told to
            ask an administrator (WEB-U-317, as WEB-U-153 for the internet line). */}
        {admin && !copying && missing && (
          <p className="type-small start-internet">
            {missing === "models" ? (
              <Link href="/settings/models">{t("start.addModelRoute")}</Link>
            ) : (
              <Link href="/settings/environments">
                {t("start.openEnvironments")}
              </Link>
            )}
          </p>
        )}
        {blind && (
          <p className="type-small muted start-internet">
            {t("start.desktopBlind")}
          </p>
        )}
        {/* A fresh install reaches no site (WEB-U-119, the first hour's F20): said before a task fails on it. */}
        {session.data?.internet === "none" && (
          <p className="type-small muted start-internet">
            {/* An administrator can allow sites themselves, so isn't told to ask one (WEB-U-153). */}
            {t(
              session.data.role === "admin"
                ? "start.noInternetAdmin"
                : "start.noInternet",
            )}
            {session.data.role === "admin" && (
              <>
                {" "}
                <Link href="/settings/internet-access">
                  {t("start.allowSites")}
                </Link>
              </>
            )}
          </p>
        )}
        {!shown && (
          <div className="example-tasks">
            {/* A general-purpose agent: examples for data, writing and code, not only code (the owner's principle). On
                 files the task has (a chosen workspace that holds some), else from nothing (candidate12 F2, WEB-U-128). */}
            {(chosen && (chosen.size_bytes ?? 0) > 0
              ? [
                  t("start.exampleExplain"),
                  t("start.exampleData"),
                  t("start.exampleWrite"),
                ]
              : [
                  t("start.exampleSpreadsheet"),
                  t("start.examplePlan"),
                  t("start.exampleScript"),
                ]
            )
              // With agent teams on, a team starts when a person asks a task to lead: one example says how (WEB-U-318).
              .concat(teamsOn ? [t("start.exampleTeam")] : [])
              .map((example) => (
                <Button
                  key={example}
                  variant="secondary"
                  onClick={() => {
                    setDraft(example);
                    container.current?.querySelector("textarea")?.focus();
                  }}
                >
                  {example}
                </Button>
              ))}
          </div>
        )}
      </div>
    </section>
  );
}

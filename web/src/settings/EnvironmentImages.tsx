"use client";

import { useQuery } from "@tanstack/react-query";
import { useEffect, useId, useRef, useState } from "react";

import { capabilities, type Capability } from "../conversations/capabilities";
import { useLanguage, useT } from "../i18n/I18nProvider";
import { formatSize } from "../i18n/message";
import { sentenceGap } from "../i18n/sentences";
import type { MessageKey } from "../i18n/translate";
import { Icon } from "../ui/Icon";
import { Input } from "../ui/Input";
import { MenuButton, type MenuChoice } from "../ui/MenuButton";

import {
  ALL_CAPABILITIES,
  IMAGES_KEY,
  type EnvironmentImage,
  type EnvironmentItem,
  type TemplatesApi,
} from "./admin/templates";

/** The host's images (built-ins and its own), one query for the image choice and the capabilities beside it. */
export const useHostImages = (api: TemplatesApi) =>
  useQuery({
    queryKey: IMAGES_KEY,
    queryFn: ({ signal }) => api.images(signal),
    retry: false,
  });

/** The tools an image's label declares that this version knows, in the pickers' order: Data and Desktop too (WEB-U-385). */
export const declared = (image: EnvironmentImage): Capability[] =>
  ALL_CAPABILITIES.filter((capability) =>
    image.capabilities.includes(capability),
  );

/** Why an image can't be an environment, in the page's words (the gateway's `reason` is fixed English). */
function unusable(image: EnvironmentImage): MessageKey {
  if (image.runtime === "too_old") return "environments.imageTooOld";
  if (image.runtime === "none") return "environments.imageNoRuntime";
  if (!image.capabilities_valid) return "environments.imageBadLabel";
  return "environments.imageUnusable";
}

/**
 * An environment's image, chosen rather than typed (WEB-U-097): the built-ins as cards, then the host's own images in
 * a picker, each saying what it is; the image ID itself only under Advanced, open when it is none of the choices.
 */
export function ImageChoice({
  api,
  items,
  creating,
  image,
  disabled,
  error,
  resolved,
  onImage,
}: {
  api: TemplatesApi;
  items: readonly EnvironmentItem[];
  creating: boolean;
  /** The image as the draft has it (an ID, or a name with its digest). */
  image: string;
  disabled: boolean;
  error: string | undefined;
  /** The local image ID a name@digest resolved to, when it differs. */
  resolved: string | null;
  /**
   * A choice sets the image and, when it declares them, its tools; a built-in's card also gives the built-in's own size
   * (null: the installation's defaults), which a new environment takes (WEB-U-386).
   */
  onImage: (
    image: string,
    tools?: Capability[],
    sizes?: EnvironmentImage["sizes"],
  ) => void;
}) {
  const t = useT();
  const { language } = useLanguage();
  const hint = useId();
  const listed = useHostImages(api);
  const all = listed.data?.items ?? [];
  const builtIns = all.filter((each) => each.built_in !== null && each.usable);
  // A new environment starts from a built-in, chosen and seen to be (WEB-U-184): the installation's default one, else
  // the first. Once only, so a cleared field stays cleared.
  const started = useRef(false);
  const start =
    builtIns.find(
      (each) =>
        items.find((item) => item.name === each.environment)?.is_default,
    ) ?? builtIns[0];
  useEffect(() => {
    if (started.current || !creating || disabled || image !== "" || !start)
      return;
    started.current = true;
    onImage(start.id, declared(start), start.sizes);
  }, [creating, disabled, image, start, onImage]);
  const own = all.filter((each) => each.built_in === null);
  const chosen = all.some((each) => each.id === image);
  // Advanced opens by itself once, when typing is the only way: the list failed, or the image is none of the choices.
  const [opened, setOpened] = useState<boolean | null>(null);
  const needed =
    listed.isError || (listed.isSuccess && image.trim() !== "" && !chosen);
  const open = opened ?? needed;
  const names = new Map(items.map((item) => [item.name, item.display_name]));
  const date = (iso: string) =>
    new Intl.DateTimeFormat(t.locale, { dateStyle: "medium" }).format(
      new Date(iso),
    );
  const choices: MenuChoice[] = own.map((each) => ({
    id: each.id,
    label: each.tags[0] ?? t("environments.untagged"),
    detail: (
      <span className="type-small muted">
        {[
          each.based_on &&
            t("environments.derivedFrom", {
              base: names.get(each.based_on) ?? each.based_on,
            }),
          t("environments.imageBuilt", { date: date(each.created) }),
          formatSize(each.size_bytes, language),
          each.used_by.length > 0 &&
            t("environments.imageUsedBy", {
              names: each.used_by
                .map((name) => names.get(name) ?? name)
                .join(", "),
            }),
        ]
          .filter(Boolean)
          .join(" · ")}
      </span>
    ),
    ...(each.usable ? {} : { disabled: t(unusable(each)) }),
  }));
  return (
    // The built-ins are a list of choices: the whole content width (WEB-U-131).
    <div className="page-section-list stack">
      {builtIns.length > 0 && (
        <div
          role="radiogroup"
          aria-label={t("environments.builtInCards")}
          className="stack"
        >
          <p className="type-label" aria-hidden="true">
            {t("environments.builtInCards")}
          </p>
          <div className="environment-cards">
            {builtIns.map((each) => {
              const item = items.find(
                (candidate) => candidate.name === each.environment,
              );
              const title =
                each.display_name ?? item?.display_name ?? each.tags[0] ?? "";
              return (
                <label
                  key={each.id}
                  className="environment-card"
                  data-checked={each.id === image || undefined}
                >
                  <input
                    type="radio"
                    name="built-in"
                    className="sr-only"
                    checked={each.id === image}
                    disabled={disabled}
                    onChange={() =>
                      onImage(each.id, declared(each), each.sizes)
                    }
                  />
                  <span className="type-body-strong">{title}</span>
                  {item?.description && (
                    <span className="type-small muted">{item.description}</span>
                  )}
                  <span className="environment-tools type-small">
                    {declared(each).map((value) => (
                      <span key={value}>
                        <Icon glyph={capabilities[value].icon} size={16} />
                        {t(capabilities[value].label)}
                      </span>
                    ))}
                  </span>
                </label>
              );
            })}
          </div>
        </div>
      )}
      {listed.isSuccess &&
        (own.length > 0 ? (
          <div className="stack">
            <p className="type-label" aria-hidden="true">
              {t("environments.ownImages")}
            </p>
            <MenuButton
              label={t("environments.ownImages")}
              options={choices}
              value={image}
              placeholder={t("environments.pickImage")}
              disabled={disabled}
              onChange={(id) => {
                const picked = own.find((each) => each.id === id);
                if (picked)
                  onImage(
                    picked.id,
                    picked.capabilities_valid ? declared(picked) : undefined,
                  );
              }}
            />
          </div>
        ) : (
          <p className="type-small muted">{t("environments.noImages")}</p>
        ))}
      {listed.isError && (
        <p className="type-small" role="status">
          {t("environments.imagesFailed")}
        </p>
      )}
      <details
        className="route-group"
        open={open}
        onToggle={(event) => setOpened(event.currentTarget.open)}
      >
        <summary className="type-small">{t("environments.advanced")}</summary>
        <div className="stack">
          <Input
            label={t("environments.imageId")}
            value={image}
            onChange={(event) => onImage(event.target.value)}
            autoComplete="off"
            spellCheck={false}
            disabled={disabled}
            aria-describedby={hint}
            {...(error ? { error } : {})}
          />
          <p id={hint} className="type-small muted">
            {t("environments.imageHint")}
          </p>
          {resolved && (
            <p className="type-small muted">
              {t("environments.imageResolved")}
              {sentenceGap(t.locale)}
              <code className="type-code">{resolved}</code>
            </p>
          )}
          {creating && <BuildHelp />}
        </div>
      </details>
    </div>
  );
}

/** A starter image build (F9): Workstation plus packages, as root, then back to the runtime user. Code, not copy. */
const STARTER = `FROM tqa-runtime:workstation
USER 0:0
RUN apt-get update \\
 && apt-get install -y --no-install-recommends pandoc python3-pandas \\
 && rm -rf /var/lib/apt/lists/*
USER 1000:1000`;
const COMMANDS = `docker build -t my-environment .
docker image inspect --format '{{.Id}}' my-environment`;

/** How to build an environment's image and what the Image ID field wants, closed until asked for (F9, the UI part). */
function BuildHelp() {
  const t = useT();
  return (
    <details className="route-group build-help">
      <summary className="type-body-strong">
        {t("environments.buildHelp")}
      </summary>
      <div className="stack">
        <p className="type-small">{t("environments.buildStart")}</p>
        <pre className="type-code build-help-code">{STARTER}</pre>
        <p className="type-small">{t("environments.buildOnHost")}</p>
        <pre className="type-code build-help-code">{COMMANDS}</pre>
        <p className="type-small">{t("environments.buildDigest")}</p>
      </div>
    </details>
  );
}

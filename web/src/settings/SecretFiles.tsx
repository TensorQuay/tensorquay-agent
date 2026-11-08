"use client";

import { useId } from "react";

import { useT } from "../i18n/I18nProvider";
import { formatSize } from "../i18n/message";
import type { MessageKey, Translate } from "../i18n/translate";

import type { EnvironmentItem } from "./admin/templates";

type Check = EnvironmentItem["check"];
type Rule = NonNullable<Check["secret_files"]>["items"][number]["rule"];

/**
 * The scanner's closed rules in words (WEB-U-060). Pairs rather than an object: the commit guard reads a key ending in
 * `_token` or `_password` with a quoted value as a secret. `complete` fails the typecheck when the contract adds a rule
 * without its sentence here.
 */
const RULE_COPY = [
  ["private_key", "environments.rulePrivateKey"],
  ["git_credentials", "environments.ruleCodeHostLogins"],
  ["netrc", "environments.ruleNetrc"],
  ["aws_credentials", "environments.ruleAwsCredentials"],
  ["docker_auth", "environments.ruleDockerAuth"],
  ["npmrc_token", "environments.ruleNpmrcToken"],
  ["pypirc_password", "environments.rulePypircPassword"],
  ["ssh_key", "environments.ruleSshKey"],
] as const satisfies readonly (readonly [Rule, MessageKey])[];
const complete: Exclude<Rule, (typeof RULE_COPY)[number][0]> extends never
  ? true
  : never = true;
void complete;
const RULES = new Map<Rule, MessageKey>(RULE_COPY);

/** Whether a refused check names the scan bound it hit (reason image_scan_limit). */
export const hasScanLimit = (item: EnvironmentItem) =>
  Boolean(item.check.scan_limit);

/**
 * The credential files an environment's image holds, under its check: on a refusal every hit, acknowledged ones marked;
 * on a verified one the acknowledged hits as a warning. For an image too large to scan, the setting to raise.
 */
export function SecretFiles({ item }: { item: EnvironmentItem }) {
  const t = useT();
  const id = useId();
  const check = item.check;
  const limit = check.scan_limit;
  if (check.state === "refused" && limit)
    return (
      <p className="type-small">
        {limit.bound === "entries"
          ? t("environments.scanLimitEntries", {
              limit: new Intl.NumberFormat(t.locale).format(limit.limit),
            })
          : t("environments.scanLimitBytes", {
              size: formatSize(limit.limit, t.locale),
            })}
      </p>
    );
  const found = check.secret_files;
  if (!found || found.items.length === 0) return null;
  const title =
    check.state === "refused"
      ? t("environments.secretFilesFound")
      : t("environments.secretFilesAcknowledged");
  return (
    <div className="stack secret-files">
      <p id={id} className="type-small">
        {title}
      </p>
      <ul aria-labelledby={id} className="secret-file-list type-small">
        {found.items.map((file) => (
          <li key={`${file.path} ${file.rule}`}>
            <code className="type-code secret-ref">{file.path}</code>
            {`: ${t(RULES.get(file.rule) ?? "environments.ruleOther")}`}
            {check.state === "refused" && file.acknowledged
              ? ` ${t("environments.secretAcknowledged")}`
              : ""}
          </li>
        ))}
      </ul>
      {found.more > 0 && (
        <p className="type-small muted">
          {t("environments.secretMore", { count: found.more })}
        </p>
      )}
    </div>
  );
}

/**
 * The same words as parts of one line, for a list row that keeps them to one line (17.3): the scan bound's sentence, or
 * the list's title, each file with what it is (acknowledged ones marked) and the count of the rest; none without them.
 */
export function secretFilesWords(
  item: EnvironmentItem,
  t: Translate,
): string[] {
  const check = item.check;
  const limit = check.scan_limit;
  if (check.state === "refused" && limit)
    return [
      limit.bound === "entries"
        ? t("environments.scanLimitEntries", {
            limit: new Intl.NumberFormat(t.locale).format(limit.limit),
          })
        : t("environments.scanLimitBytes", {
            size: formatSize(limit.limit, t.locale),
          }),
    ];
  const found = check.secret_files;
  if (!found || found.items.length === 0) return [];
  return [
    check.state === "refused"
      ? t("environments.secretFilesFound")
      : t("environments.secretFilesAcknowledged"),
    ...found.items.map(
      (file) =>
        `${file.path}: ${t(RULES.get(file.rule) ?? "environments.ruleOther")}${
          check.state === "refused" && file.acknowledged
            ? ` ${t("environments.secretAcknowledged")}`
            : ""
        }`,
    ),
    ...(found.more > 0
      ? [t("environments.secretMore", { count: found.more })]
      : []),
  ];
}

import { useT } from "../i18n/I18nProvider";

import type { EnvironmentItem } from "./admin/templates";
import { rootfsImage } from "./environment-copy";

/**
 * An environment whose microVM disk image isn't built (`rootfs_missing`): what that means for people, and the exact
 * command an operator runs on the gateway's host to build it (deploy/firecracker/README.md).
 */
export function EnvironmentRootfsMissing({ item }: { item: EnvironmentItem }) {
  const t = useT();
  return (
    <>
      <p className="type-small">
        {t("environments.rootfsMissingLine", { name: item.display_name })}
      </p>
      <code className="type-code code-wrap">
        {t("environments.rootfsBuildCommand", { image: rootfsImage(item) })}
      </code>
      <p className="type-small">{t("environments.rootfsMissingThen")}</p>
    </>
  );
}

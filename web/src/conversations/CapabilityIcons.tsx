import { useT } from "../i18n/I18nProvider";
import { Icon } from "../ui/Icon";

import { capabilities, capabilityNames, type Capability } from "./capabilities";
import "../i18n/deferred-catalogue";

/** Labels a compact set of declared capabilities as one accessible image. */
export function CapabilityIcons({ values }: { values: readonly Capability[] }) {
  const t = useT();
  return (
    <span
      className="environment-capabilities"
      role="img"
      aria-label={t("capability.label", {
        names: capabilityNames(values, t),
      })}
    >
      {values.length ? (
        values.map((value) => (
          <span key={value} title={t(capabilities[value].label)}>
            <Icon glyph={capabilities[value].icon} size={16} />
          </span>
        ))
      ) : (
        <span className="type-small muted">{t("capability.none")}</span>
      )}
    </span>
  );
}

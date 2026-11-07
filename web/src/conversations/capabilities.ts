import {
  Braces,
  ChartColumn,
  Code,
  Globe,
  Monitor,
  Terminal,
  Wrench,
  type LucideIcon,
} from "lucide-react";

import { english, type MessageKey, type Translate } from "../i18n/translate";
import type { Event } from "../protocol/generated/protocol";

/** The closed, declared capability vocabulary from the shared environment event. */
export type Capability = Extract<
  Event,
  { type: "environment.changed" }
>["capabilities"][number];

/** Presentation never probes the running environment to invent capabilities. */
export const capabilities: Record<
  Capability,
  { label: MessageKey; icon: LucideIcon }
> = {
  shell: { label: "capability.shell", icon: Terminal },
  python: { label: "capability.python", icon: Code },
  node: { label: "capability.node", icon: Braces },
  build: { label: "capability.build", icon: Wrench },
  browser: { label: "capability.browser", icon: Globe },
  data: { label: "capability.data", icon: ChartColumn },
  desktop: { label: "capability.desktop", icon: Monitor },
};

/** Gives capability icons and menu descriptions the same human names, in the reader's own list ("Shell, Python"). */
export function capabilityNames(
  values: readonly Capability[],
  t: Translate = english,
): string {
  return (
    values
      .map((value) => t(capabilities[value].label))
      .join(t("capability.separator")) || t("capability.none")
  );
}

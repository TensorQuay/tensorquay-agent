import type { LucideIcon } from "lucide-react";
import type { ReactNode } from "react";

import { Icon } from "./Icon";

/**
 * An empty view (the design direction): a 20 px icon that names the thing, never the logo; one sentence; one primary
 * action.
 */
export function EmptyState({
  icon,
  message,
  action,
}: {
  icon: LucideIcon;
  message: string;
  action: ReactNode;
}) {
  return (
    <div className="empty-state">
      <Icon glyph={icon} size={20} className="empty-icon" />
      <p>{message}</p>
      {action}
    </div>
  );
}

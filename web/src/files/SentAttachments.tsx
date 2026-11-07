import { FileText } from "lucide-react";

import { useT } from "../i18n/I18nProvider";
import { Icon } from "../ui/Icon";

/** The files a message named, by their name inside uploads/. */
export function SentAttachments({ paths }: { paths: readonly string[] }) {
  const t = useT();
  if (paths.length === 0) return null;
  return (
    <ul className="attachment-chips" aria-label={t("attach.list")}>
      {paths.map((path) => {
        const name = path.slice(path.lastIndexOf("/") + 1);
        return (
          <li className="attachment-chip" data-status="sent" key={path}>
            <Icon glyph={FileText} size={16} />
            <span className="attachment-name" title={name}>
              {name}
            </span>
          </li>
        );
      })}
    </ul>
  );
}

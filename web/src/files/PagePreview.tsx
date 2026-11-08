"use client";

import { RotateCw } from "lucide-react";
import { useState } from "react";

import { api } from "../api/client";
import { useT } from "../i18n/I18nProvider";
import { formatSize } from "../i18n/message";
import { Button } from "../ui/Button";
import { Icon } from "../ui/Icon";

import "./changes-catalogue";

/**
 * An agent's web page, framed from `/files/view` (the file previews design). The gateway's headers sandbox it (an
 * opaque origin, no network, no forms, popups or top-level navigation) even if the URL is opened on its own; the frame
 * repeats the sandbox. Scripts run, since a dashboard without them is blank: `allow-scripts` and nothing else, so they
 * never reach this page, the session or the API. Never add `allow-same-origin` (with `allow-scripts` the page could
 * lift its own sandbox), `allow-popups`, `allow-top-navigation`, `allow-forms`, `allow-modals` or `allow-downloads`.
 * A page over the preview limit isn't framed: the gateway would answer with an error, not the page.
 */
export function PagePreview({
  conversationId,
  path,
  name,
  size,
  etag,
  limit,
}: {
  conversationId: string;
  path: string;
  name: string;
  size: number;
  etag: string;
  limit: number;
}) {
  const t = useT();
  // Each reload is a new frame, so the page starts again from the file as it is now.
  const [load, setLoad] = useState(0);
  // A frame loads once; a second load is the page leaving itself (a link followed, which the sandbox blocks, so the
  // frame goes blank): say so, with Reload (a pass over the pages before 17.0; WEB-U-254). Counted per frame.
  const [loads, setLoads] = useState({ frame: "", count: 0 });
  const frame = `${etag}-${load}`;
  const left = loads.frame === frame && loads.count > 1;
  if (size > limit)
    return (
      <p className="type-small muted files-state">
        {t("files.pageTooLarge", {
          size: formatSize(size, t.locale),
          limit: formatSize(limit, t.locale),
        })}
      </p>
    );
  return (
    <div className="file-kind stack">
      <div className="file-kind-bar">
        <span className="type-small muted">{t("files.pageSandboxed")}</span>
        <Button variant="ghost" onClick={() => setLoad(load + 1)}>
          <Icon glyph={RotateCw} size={16} />
          {t("files.reload")}
        </Button>
      </div>
      {left && (
        <p className="type-small muted" role="status">
          {t("files.pageLeft")}
        </p>
      )}
      <iframe
        key={frame}
        className="file-page"
        title={t("files.pageFrame", { name })}
        src={api.viewUrl(conversationId, path)}
        sandbox="allow-scripts"
        referrerPolicy="no-referrer"
        onLoad={() =>
          setLoads((was) => ({
            frame,
            count: was.frame === frame ? was.count + 1 : 1,
          }))
        }
      />
    </div>
  );
}

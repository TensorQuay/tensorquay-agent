"use client";

import { useQuery } from "@tanstack/react-query";
import { RotateCw } from "lucide-react";
import { useEffect, useRef, useState } from "react";

import { api } from "../api/client";
import { mintSitePass } from "../api/previews";
import { useT } from "../i18n/I18nProvider";
import { formatSize } from "../i18n/message";
import { Button } from "../ui/Button";
import { Icon } from "../ui/Icon";

import "./changes-catalogue";
import { blockedHosts, pageHosts } from "./page-hosts";
import { type SiteReport, siteReportLine, siteReportPart } from "./site-report";

/** The hosts the offline line names before it counts the rest. */
const NAMED_HOSTS = 3;

/**
 * An agent's web page, framed from `/files/view`. The gateway's headers sandbox it (an opaque origin, no network, no
 * forms, popups or top-level navigation) even if the URL is opened on its own; the frame repeats the sandbox. Scripts
 * run, since a dashboard without them is blank: `allow-scripts` and nothing else, so they
 * never reach this page, the session or the API. Never add `allow-same-origin` (with `allow-scripts` the page could
 * lift its own sandbox), `allow-popups`, `allow-top-navigation`, `allow-forms`, `allow-modals` or `allow-downloads`.
 * A page over the preview limit isn't framed: the gateway would answer with an error, not the page.
 *
 * The frame is offline, so a page that loads from the internet says so on a row of its own under the bar, naming the
 * hosts as text (WEB-U-467); the bar stays as it is. They're read from the page's text: `text` when the panel has all
 * of it, else its download.
 *
 * A page is framed from its folder served as a site (WEB-U-470: "most sites have many files"): a short-lived pass's
 * address, so the page's own CSS, scripts, pictures, fonts and links load, and outside hosts as the install's network
 * setting allows; the line then names the hosts that setting blocks. Scripts run by default; Stop scripts reloads the
 * page without them. If the site can't be opened (the pass refused), the page is framed from the single-page view as
 * before, offline.
 */
export function PagePreview({
  conversationId,
  path,
  name,
  size,
  etag,
  limit,
  text,
}: {
  conversationId: string;
  path: string;
  name: string;
  size: number;
  etag: string;
  limit: number;
  /** The page's whole text, or null when the panel's preview of it was cut short or isn't text. */
  text: string | null;
}) {
  const t = useT();
  const framed = size <= limit;
  // Nothing to say while it's read, or if it can't be: the sandbox note stands.
  const hosts = useQuery({
    queryKey: ["pageHosts", conversationId, path, etag],
    queryFn: async ({ signal }) =>
      pageHosts(
        text ??
          (await (await api.fileBlob(conversationId, path, signal)).text()),
      ),
    enabled: framed,
    staleTime: Infinity,
    // One more read after a failure (a dropped connection), so one bad moment doesn't hide the line for this version.
    retry: 1,
  }).data;
  // The page's folder as a site; until it answers, nothing is framed (no flash of the offline view). The pass lives 15
  // minutes from its last use; it's minted again after 10.
  const slash = path.lastIndexOf("/");
  const folder = slash < 0 ? "" : path.slice(0, slash);
  const site = useQuery({
    queryKey: ["sitePass", conversationId, folder],
    queryFn: ({ signal }) => mintSitePass(api, conversationId, folder, signal),
    enabled: framed,
    staleTime: 10 * 60 * 1000,
    retry: false,
    // A refused pass stays refused for this view: Expand's second frame falls back at once instead of asking again.
    retryOnMount: false,
  });
  // Each reload is a new frame, so the page starts again from the file as it is now.
  const [load, setLoad] = useState(0);
  const [scripts, setScripts] = useState(true);
  // A frame loads once; a second load is the page leaving itself (a link followed, which the sandbox blocks, so the
  // frame goes blank): say so, with Reload (a pass over the pages before 17.0; WEB-U-254). Counted per frame.
  const [loads, setLoads] = useState({ frame: "", count: 0 });
  const frame = `${etag}-${load}-${scripts ? "run" : "stop"}`;
  const served = site.data;
  // In the site, a link to another of its pages is followed in the frame; only the single-page view is left blank.
  const left = !served && loads.frame === frame && loads.count > 1;
  const blocked =
    served?.network && hosts ? blockedHosts(hosts, served.network) : [];
  const named = (list: readonly string[], beyond = 0) => {
    const more = Math.max(0, list.length - NAMED_HOSTS) + beyond;
    return new Intl.ListFormat(t.locale, { type: "conjunction" }).format([
      ...list.slice(0, NAMED_HOSTS),
      ...(more > 0 ? [t("files.pageHostsMore", { count: more })] : []),
    ]);
  };
  // What the page reported it couldn't load (the gateway's reporter, 18.1): only from this frame, whose origin is
  // opaque, and only for this frame; a reload, Stop scripts or another page starts again with nothing said.
  const frameRef = useRef<HTMLIFrameElement>(null);
  const [report, setReport] = useState<{ frame: string } & SiteReport>({
    frame: "",
  });
  useEffect(() => {
    if (!served) return;
    const listen = (event: MessageEvent) => {
      const source = frameRef.current?.contentWindow;
      if (!source || event.source !== source || event.origin !== "null") return;
      const part = siteReportPart(event.data);
      if (!part) return;
      setReport((was) => ({
        ...(was.frame === frame ? was : { frame }),
        ...part,
      }));
    };
    window.addEventListener("message", listen);
    return () => window.removeEventListener("message", listen);
  }, [served, frame]);
  const said: SiteReport = served && report.frame === frame ? report : {};
  const line = siteReportLine(said);
  const src = served
    ? `${served.url}${path
        .slice(slash + 1)
        .split("/")
        .map(encodeURIComponent)
        .join("/")}`
    : site.isError
      ? api.viewUrl(conversationId, path)
      : null;
  if (!framed)
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
        <span className="type-small muted">
          {t(scripts ? "files.pageSandboxed" : "files.pageScriptsStopped")}
        </span>
        <Button variant="ghost" onClick={() => setScripts(!scripts)}>
          {t(scripts ? "files.stopScripts" : "files.runScripts")}
        </Button>
        <Button variant="ghost" onClick={() => setLoad(load + 1)}>
          <Icon glyph={RotateCw} size={16} />
          {t("files.reload")}
        </Button>
      </div>
      {served
        ? blocked.length > 0 && (
            <p className="type-small muted">
              {t("files.siteBlocked", { hosts: named(blocked) })}
            </p>
          )
        : site.isError &&
          hosts &&
          hosts.length > 0 && (
            <p className="type-small muted">
              {t("files.pageOffline", { hosts: named(hosts) })}
            </p>
          )}
      {line === "root" && said.root && (
        <p className="type-small muted" role="status">
          {said.root.paths.length > 0
            ? t("files.siteRootPaths", {
                paths: named(said.root.paths, said.root.more),
              })
            : t("files.siteRootPathsUnnamed")}
        </p>
      )}
      {line === "missing" && said.missing && (
        <p className="type-small muted" role="status">
          {t("files.siteMissing", {
            paths: named(said.missing.paths, said.missing.more),
          })}
        </p>
      )}
      {line === "empty" && (
        <p className="type-small muted" role="status">
          {t("files.siteEmpty")}
        </p>
      )}
      {left && (
        <p className="type-small muted" role="status">
          {t("files.pageLeft")}
        </p>
      )}
      {/* Framed once its address is known: a frame without one would load a blank page first and count it. */}
      {src && (
        <iframe
          key={frame}
          ref={frameRef}
          className="file-page"
          title={t("files.pageFrame", { name })}
          src={src}
          sandbox={scripts ? "allow-scripts" : ""}
          referrerPolicy="no-referrer"
          onLoad={() =>
            setLoads((was) => ({
              frame,
              count: was.frame === frame ? was.count + 1 : 1,
            }))
          }
        />
      )}
    </div>
  );
}

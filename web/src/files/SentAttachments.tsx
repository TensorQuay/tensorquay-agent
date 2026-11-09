import { useQuery } from "@tanstack/react-query";
import { FileText } from "lucide-react";
import {
  useEffect,
  useEffectEvent,
  useRef,
  useState,
  type ReactNode,
} from "react";

import { api } from "../api/client";
import { fileContent } from "../api/previews";
import { useT } from "../i18n/I18nProvider";
import { Icon } from "../ui/Icon";

import { extension, IMAGE_TYPES } from "./file-kind";
import { MAX_IMAGE_BYTES, useImageBytes } from "./image-bytes";
import "./changes-catalogue";

/** The image types a sent message shows as pictures (WEB-U-464), as the model's image input accepts them. */
const PICTURE = /\.(png|jpe?g|gif|webp)$/i;

/**
 * One sent image, drawn from its bytes as the Files panel draws one. Nothing is read until it nears the screen. Then
 * its size and version are read first, into the cache the panel's preview reads (opening the file reads nothing again);
 * its bytes only up to the panel's limit; and an object URL of the type its name gives, so the picture needs nothing the
 * page's policy doesn't already allow. A file that can't be read, or one over the limit, gives way to the file's chip; a
 * picture already drawn stays when a later read of it fails.
 */
function Picture({
  conversationId,
  path,
  alt,
  onBroken,
}: {
  conversationId: string;
  path: string;
  alt: string;
  onBroken: () => void;
}) {
  // Near the screen (or a browser that can't tell): a long task's pictures load as they are scrolled to.
  const [near, setNear] = useState(
    () => typeof IntersectionObserver === "undefined",
  );
  const waiting = useRef<HTMLSpanElement>(null);
  useEffect(() => {
    const placeholder = waiting.current;
    if (near || !placeholder) return;
    const watch = new IntersectionObserver(
      (entries) => {
        if (entries.some((entry) => entry.isIntersecting)) setNear(true);
      },
      { rootMargin: "400px 0px" },
    );
    watch.observe(placeholder);
    return () => watch.disconnect();
  }, [near]);
  const file = useQuery({
    queryKey: ["file", conversationId, path],
    queryFn: ({ signal }) => fileContent(api, conversationId, path, signal),
    enabled: near,
    // A sent file is read again when the Files panel refreshes it, not on every focus of the window.
    staleTime: Infinity,
    retry: false,
  });
  const bytes = useImageBytes(conversationId, path, file.data);
  // Only a read that never succeeded: a refresh failing later leaves the picture as it was.
  const unshown =
    (file.isError && file.data === undefined) ||
    (bytes.isError && bytes.data === undefined) ||
    (file.data !== undefined && file.data.size > MAX_IMAGE_BYTES);
  // The latest callback, read when the picture can't be shown, so a parent's re-render never reads the file again.
  const fail = useEffectEvent(() => onBroken());
  useEffect(() => {
    if (unshown) fail();
  }, [unshown]);
  const image = useRef<HTMLImageElement>(null);
  const data = bytes.data;
  // Made from bytes already read, and revoked with the picture: no object URL outlives it.
  useEffect(() => {
    if (!data || !image.current) return;
    const url = URL.createObjectURL(
      new Blob([data], {
        type: IMAGE_TYPES[extension(path)] ?? "application/octet-stream",
      }),
    );
    image.current.src = url;
    return () => URL.revokeObjectURL(url);
  }, [data, path]);
  return data ? (
    // eslint-disable-next-line @next/next/no-img-element -- the workspace's own bytes (blob:); next/image can't serve them
    <img ref={image} alt={alt} decoding="async" onError={onBroken} />
  ) : (
    <span
      ref={waiting}
      className="sent-picture-waiting"
      role="img"
      aria-label={alt}
    />
  );
}

/**
 * The files a message named, by their name inside uploads/ (WEB-U-464), in one list: pictures first, then the other
 * files' chips. Loaded only for a message that has files, so a task's page without any never pays for it. With a
 * conversation and a Files panel, an image shows as its picture and every file is a button that opens its preview in
 * the panel; without a panel nothing is drawn as a control. A picture that can't be shown falls back to the file's chip.
 */
export function SentAttachments({
  paths,
  conversationId,
  onOpenFile,
}: {
  paths: readonly string[];
  conversationId?: string | undefined;
  onOpenFile?: ((path: string) => void) | undefined;
}) {
  const t = useT();
  const [broken, setBroken] = useState<ReadonlySet<string>>(new Set());
  if (paths.length === 0) return null;
  const name = (path: string) => path.slice(path.lastIndexOf("/") + 1);
  const pictured = (path: string) =>
    conversationId !== undefined && PICTURE.test(path) && !broken.has(path);
  /** A button that opens the file's preview, where the page has a Files panel; nothing otherwise. */
  const opener = (path: string, className: string, children: ReactNode) =>
    onOpenFile ? (
      <button
        type="button"
        className={className}
        aria-label={t("attach.open", { name: name(path) })}
        onClick={() => onOpenFile(path)}
      >
        {children}
      </button>
    ) : null;
  return (
    <ul
      className="attachment-chips sent-attachments"
      aria-label={t("attach.list")}
    >
      {conversationId !== undefined &&
        paths.filter(pictured).map((path) => {
          const picture = (
            <Picture
              conversationId={conversationId}
              path={path}
              alt={name(path)}
              onBroken={() => setBroken((before) => new Set(before).add(path))}
            />
          );
          return (
            <li key={path}>
              {opener(path, "sent-picture", picture) ?? (
                <span className="sent-picture">{picture}</span>
              )}
            </li>
          );
        })}
      {paths
        .filter((path) => !pictured(path))
        .map((path) => {
          const label = (
            <>
              <Icon glyph={FileText} size={16} />
              <span className="attachment-name" title={name(path)}>
                {name(path)}
              </span>
            </>
          );
          return (
            <li className="attachment-chip" data-status="sent" key={path}>
              {opener(path, "attachment-open", label) ?? label}
            </li>
          );
        })}
    </ul>
  );
}

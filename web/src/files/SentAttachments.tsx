import { FileText } from "lucide-react";
import { useEffect, useEffectEvent, useState } from "react";

import { api } from "../api/client";
import { useT } from "../i18n/I18nProvider";
import { Icon } from "../ui/Icon";

import { extension, IMAGE_TYPES } from "./file-kind";
import "./changes-catalogue";

/** The image types a sent message shows as pictures (WEB-U-464), as the model's image input accepts them. */
const PICTURE = /\.(png|jpe?g|gif|webp)$/i;

/**
 * One sent image, drawn from its bytes as the Files panel draws one (an object URL of the type its name gives), so the
 * picture needs nothing the page's policy doesn't already allow; a failed read gives way to the file's chip.
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
  const [src, setSrc] = useState<string>();
  // The latest callback, read when the bytes fail, so a parent's re-render never reads the file again.
  const fail = useEffectEvent(() => onBroken());
  useEffect(() => {
    const stop = new AbortController();
    let url: string | undefined;
    api.fileBlob(conversationId, path, stop.signal).then(
      async (blob) => {
        const type = IMAGE_TYPES[extension(path)] ?? "application/octet-stream";
        url = URL.createObjectURL(
          new Blob([await blob.arrayBuffer()], { type }),
        );
        if (!stop.signal.aborted) setSrc(url);
      },
      () => {
        if (!stop.signal.aborted) fail();
      },
    );
    return () => {
      stop.abort();
      if (url) URL.revokeObjectURL(url);
    };
  }, [conversationId, path]);
  return src ? (
    // eslint-disable-next-line @next/next/no-img-element -- the workspace's own bytes (blob:); next/image can't serve them
    <img src={src} alt={alt} decoding="async" onError={onBroken} />
  ) : (
    <span className="sent-picture-waiting" role="img" aria-label={alt} />
  );
}

/**
 * The files a message named, by their name inside uploads/ (WEB-U-464). Loaded only for a message that has files, so
 * a task's page without any never pays for it. With a conversation and a Files panel, an image shows as its picture
 * and every file opens its preview in the panel; a picture that can't load falls back to the file's chip.
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
  const pictures = paths.filter(pictured);
  const chips = paths.filter((path) => !pictured(path));
  const open = (path: string, children: React.ReactNode, className: string) =>
    onOpenFile ? (
      <button
        type="button"
        className={className}
        aria-label={t("attach.open", { name: name(path) })}
        onClick={() => onOpenFile(path)}
      >
        {children}
      </button>
    ) : (
      <span className={className}>{children}</span>
    );
  return (
    <>
      {pictures.length > 0 && conversationId !== undefined && (
        <ul className="sent-pictures" aria-label={t("attach.list")}>
          {pictures.map((path) => (
            <li key={path}>
              {open(
                path,
                <Picture
                  conversationId={conversationId}
                  path={path}
                  alt={name(path)}
                  onBroken={() =>
                    setBroken((before) => new Set(before).add(path))
                  }
                />,
                "sent-picture",
              )}
            </li>
          ))}
        </ul>
      )}
      {chips.length > 0 && (
        <ul className="attachment-chips" aria-label={t("attach.list")}>
          {chips.map((path) => (
            <li className="attachment-chip" data-status="sent" key={path}>
              {open(
                path,
                <>
                  <Icon glyph={FileText} size={16} />
                  <span className="attachment-name" title={name(path)}>
                    {name(path)}
                  </span>
                </>,
                "attachment-open",
              )}
            </li>
          ))}
        </ul>
      )}
    </>
  );
}

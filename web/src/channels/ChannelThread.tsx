"use client";

import "./catalogue";
import { X } from "lucide-react";

import { useT } from "../i18n/I18nProvider";
import { Button } from "../ui/Button";
import { Icon } from "../ui/Icon";

import { ChannelComposer } from "./ChannelComposer";
import { ChannelTimeline } from "./ChannelTimeline";
import type { ChannelMessage, ChannelPost, Member } from "./contract";

/**
 * A post's thread (WEB-U-523, the mock of 17 Nov 2026): the post and its replies, in the panel beside the log (a sheet
 * on a phone), and a reply box for those who may post. The log itself shows only "N replies".
 */
export function ChannelThread({
  seq,
  messages,
  members,
  fileHref,
  posts,
  onPost,
  onClose,
}: {
  seq: number;
  messages: readonly ChannelMessage[];
  members: readonly Member[];
  fileHref: (seq: number, index: number) => string;
  /** Whether the viewer may post here. */
  posts: boolean;
  onPost: (write: ChannelPost) => Promise<void>;
  onClose: () => void;
}) {
  const t = useT();
  const title = t("channels.thread");
  const thread = messages.filter(
    (message) => message.seq === seq || message.parent === seq,
  );
  return (
    <aside className="channel-panel channel-thread stack" aria-label={title}>
      <header className="channel-panel-head">
        <h2 className="type-body-strong">{title}</h2>
        <Button
          variant="ghost"
          aria-label={t("channels.closePanel", { name: title })}
          onClick={onClose}
        >
          <Icon glyph={X} size={16} />
        </Button>
      </header>
      <ChannelTimeline
        messages={thread}
        members={members}
        fileHref={fileHref}
        label={t("channels.repliesTo", { seq })}
      />
      {posts && (
        <ChannelComposer
          key={seq}
          memberRole="member"
          state="open"
          replyTo={seq}
          onCancelReply={() => undefined}
          onPost={onPost}
          thread
        />
      )}
    </aside>
  );
}

"use client";

import "./catalogue";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useRouter } from "next/navigation";
import { useCallback, useEffect, useRef, useState } from "react";

import { newActionKey } from "../api/action-key";
import { useSession } from "../auth/use-session";
import { useT } from "../i18n/I18nProvider";
import { APP_NAME } from "../lib/app";
import { parseApi } from "../protocol/validate";
import { NavigationButton } from "../shell/NavigationButton";
import { TabTitle } from "../shell/TabTitle";
import { Skeleton } from "../ui/Skeleton";

import { ChannelActions } from "./ChannelActions";
import { ChannelComposer } from "./ChannelComposer";
import { ChannelFirstStep } from "./ChannelFirstStep";
import { ChannelIdleAgents } from "./ChannelIdleAgents";
import { ChannelMissing } from "./ChannelMissing";
import { ChannelProposal, proposalKey } from "./ChannelProposal";
import { ChannelSide, TaskClaims, tasksKey } from "./ChannelSide";
import { RULES } from "./ChannelsPage";
import { ChannelTimeline } from "./ChannelTimeline";
import type {
  ChannelMessage,
  ChannelPost,
  ChannelStream,
  ChannelsApi,
} from "./contract";
import { purposeBeyond } from "./suggest-title";
import "../protocol/generated/validators.channels";

/** A closed stream (the browser gave up reconnecting) is opened again after this long, from the last seq. */
const REOPEN_MS = 5000;

/** A stream frame's message, or null for any other frame. */
function streamed(data: unknown): ChannelMessage | null {
  try {
    const frame: unknown = JSON.parse(String(data));
    if (
      !frame ||
      typeof frame !== "object" ||
      (frame as { type?: unknown }).type !== "channel.message"
    )
      return null;
    // The frame is a ChannelMessage with its `type` added: validate it without that key.
    const rest: Record<string, unknown> = { ...frame };
    delete rest.type;
    return parseApi(rest, "ChannelMessage");
  } catch {
    return null;
  }
}
const ended = (data: unknown) => {
  try {
    const frame = JSON.parse(String(data)) as {
      type?: unknown;
      state?: unknown;
    };
    return frame.type === "channel.state" && frame.state === "ended";
  } catch {
    return false;
  }
};

/**
 * A channel's log: read page by page from the start, then followed on the channel's stream from the last seq (G8,
 * ADR-0014 point 11). A reconnect replays from the last event id, and a message the page already has is not added
 * twice; a post the page made joins at once.
 */
function useChannelLog(api: ChannelsApi, id: string, onEnded: () => void) {
  const [messages, setMessages] = useState<ChannelMessage[]>([]);
  /** Whether the first reading has finished, so an empty log is known to be empty. */
  const [loaded, setLoaded] = useState(false);
  const last = useRef(0);
  const add = useCallback((more: readonly ChannelMessage[]) => {
    if (more.length === 0) return;
    setMessages((current) => {
      const known = new Set(current.map((item) => item.seq));
      const merged = [
        ...current,
        ...more.filter((item) => !known.has(item.seq)),
      ].sort((a, b) => a.seq - b.seq);
      last.current = merged.at(-1)?.seq ?? 0;
      return merged;
    });
  }, []);
  const ending = useRef(onEnded);
  ending.current = onEnded;
  useEffect(() => {
    const abort = new AbortController();
    let source: ChannelStream | null = null;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const follow = () => {
      if (abort.signal.aborted) return;
      source = api.stream(id, last.current);
      source.onmessage = (event) => {
        const message = streamed(event.data);
        if (message) add([message]);
        else if (ended(event.data)) ending.current();
      };
      source.onerror = () => {
        // The browser reconnects on its own, replaying from the last event id; only a closed stream is reopened.
        if (source?.readyState === 2) timer = setTimeout(follow, REOPEN_MS);
      };
    };
    async function read() {
      try {
        for (;;) {
          const page = await api.messages(id, last.current, abort.signal);
          add(page.messages);
          last.current = Math.max(last.current, page.next_since);
          if (!page.more) break;
        }
      } catch {
        // The stream replays whatever the reading missed, from the last seq read.
      }
      if (!abort.signal.aborted) setLoaded(true);
      follow();
    }
    void read();
    return () => {
      abort.abort();
      source?.close();
      clearTimeout(timer);
    };
  }, [api, id, add]);
  return { messages, add, loaded };
}

/** One channel's page (ADR-0014; WEB-U-063): its bar, its log in seq order with threads, and the composer. */
export function ChannelPage({ api, id }: { api: ChannelsApi; id: string }) {
  const t = useT();
  const channel = useQuery({
    queryKey: ["channel", id],
    queryFn: ({ signal }) => api.channel(id, signal),
    retry: false,
  });
  const members = useQuery({
    queryKey: ["channel-members", id],
    queryFn: ({ signal }) => api.members(id, signal),
    retry: false,
    // No members hint on the stream yet (the channels API contract): again on focus and every 15 s while shown.
    refetchOnWindowFocus: true,
    refetchInterval: 15_000,
  });
  const cache = useQueryClient();
  // An ended channel reads only: its state comes again from the gateway (the stream's channel.state).
  const log = useChannelLog(api, id, () => {
    void cache.invalidateQueries({ queryKey: ["channel", id] });
  });
  const [replyTo, setReplyTo] = useState<number | null>(null);
  /** A message put in the box from the first step; a new one remounts the composer with it. */
  const [draft, setDraft] = useState<{ n: number; text: string }>({
    n: 0,
    text: "",
  });
  const viewer = useSession().data;
  const me = viewer?.id ?? "";
  const router = useRouter();
  // A vote or decision reaching the log refreshes its proposal's state (the channels API contract).
  const seen = useRef(0);
  useEffect(() => {
    for (const message of log.messages)
      if (message.seq > seen.current) {
        if (message.body.type === "vote" || message.body.type === "decision")
          void cache.invalidateQueries({
            queryKey: proposalKey(id, message.body.proposal_seq),
          });
        // A task, claim or report changes the board (G6).
        if (["task", "claim", "report"].includes(message.body.type))
          void cache.invalidateQueries({ queryKey: tasksKey(id) });
      }
    seen.current = log.messages.at(-1)?.seq ?? seen.current;
  }, [log.messages, cache, id]);
  if (channel.isPending) return <Skeleton label={t("channels.opening")} />;
  if (channel.isError)
    return (
      <ChannelMissing
        error={channel.error}
        retry={() => {
          void channel.refetch();
        }}
      />
    );
  const item = channel.data;
  async function post(write: ChannelPost) {
    const stored = await api.post(id, write, newActionKey());
    log.add([stored]);
    setReplyTo(null);
  }
  async function claim(seq: number) {
    log.add([await api.claim(id, seq, newActionKey())]);
  }
  async function end() {
    cache.setQueryData(["channel", id], await api.endChannel(id));
  }
  async function remove() {
    await api.deleteChannel(id);
    void cache.invalidateQueries({ queryKey: ["channels"] });
    router.push("/channels");
  }
  // ADR-0014 point 13: its owner or an administrator ends or deletes a channel (WEB-U-066).
  const administers = item.owner.id === me || viewer?.role === "admin";
  const posts =
    item.state === "open" && item.role !== null && item.role !== "observer";
  // WEB-U-063: a new channel's empty log gives a first step, and the box has the focus.
  const firstStep = posts && log.loaded && log.messages.length === 0;
  // WEB-U-250: the title is usually the purpose's start; the line under it says only what it adds.
  const beyond = purposeBeyond(item.purpose, item.name);
  return (
    <div className="channel-page">
      <TabTitle>
        {t("channels.pageTab", { name: item.name, product: APP_NAME })}
      </TabTitle>
      <section className="channel-bar" aria-label={t("channels.header")}>
        <NavigationButton />
        <div className="channel-name">
          <h1 className="type-heading">{item.name}</h1>
          {beyond && (
            <p className="type-small muted" title={item.purpose}>
              {beyond}
            </p>
          )}
        </div>
        <p className="channel-facts type-small muted">
          <span
            className="status-pill type-label"
            data-tone={item.state === "open" ? "ok" : "muted"}
          >
            {t(item.state === "open" ? "channels.open" : "channels.ended")}
          </span>
          <span>{t(RULES[item.rule])}</span>
          <span>{t("channels.members", { count: item.member_count })}</span>
        </p>
        {administers && (
          <ChannelActions channel={item} onEnd={end} onDelete={remove} />
        )}
      </section>
      <ChannelTimeline
        messages={log.messages}
        members={members.data?.items ?? []}
        after={(message) =>
          message.body.type === "proposal" ? (
            <ChannelProposal
              api={api}
              id={id}
              seq={message.seq}
              members={members.data?.items ?? []}
              me={me}
              memberRole={item.role}
              ended={item.state === "ended"}
              onVote={post}
            />
          ) : message.body.type === "task" ? (
            <TaskClaims api={api} id={id} seq={message.seq} />
          ) : null
        }
        {...(item.state === "open" &&
        item.role !== null &&
        item.role !== "observer"
          ? { onReply: setReplyTo }
          : {})}
      />
      {firstStep && (
        <ChannelFirstStep
          purpose={item.purpose}
          onUse={(text) => setDraft(({ n }) => ({ n: n + 1, text }))}
        />
      )}
      <ChannelSide
        api={api}
        id={id}
        channel={item}
        members={members.data?.items ?? []}
        me={me}
        memberRole={item.role}
        ended={item.state === "ended"}
        onClaim={claim}
        onPost={post}
      />
      {/* WEB-U-426: an agent reads the channel only while its task runs: the panel names those that aren't. */}
      {posts && <ChannelIdleAgents members={members.data?.items ?? []} />}
      <ChannelComposer
        key={draft.n}
        initialText={draft.text}
        focusBox={firstStep || draft.n > 0}
        memberRole={item.role}
        state={item.state}
        replyTo={replyTo}
        onCancelReply={() => setReplyTo(null)}
        onPost={post}
      />
    </div>
  );
}

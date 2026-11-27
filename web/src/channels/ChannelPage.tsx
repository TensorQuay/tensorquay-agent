"use client";

import "./catalogue";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useRouter } from "next/navigation";
import { useCallback, useEffect, useRef, useState } from "react";

import { newActionKey } from "../api/action-key";
import { pathFor } from "../api/operations";
import { useSession } from "../auth/use-session";
import { useT } from "../i18n/I18nProvider";
import { APP_NAME } from "../lib/app";
import { parseApi } from "../protocol/validate";
import { settingsEndpoint } from "../settings/operations";
import { TabTitle } from "../shell/TabTitle";
import { Button } from "../ui/Button";
import { Skeleton } from "../ui/Skeleton";

import { ChannelActions } from "./ChannelActions";
import { ChannelBudget } from "./ChannelBudget";
import { ChannelComposer } from "./ChannelComposer";
import { ChannelFirstStep } from "./ChannelFirstStep";
import { ChannelHeader, type HeaderPanel } from "./ChannelHeader";
import { ChannelMissing } from "./ChannelMissing";
import { ChannelProposal, proposalKey } from "./ChannelProposal";
import { ChannelSide, TaskClaims, tasksKey } from "./ChannelSide";
import { ChannelThread } from "./ChannelThread";
import { ChannelTimeline, liveBudgetNotice } from "./ChannelTimeline";
import type {
  ChannelMessage,
  ChannelPost,
  ChannelStream,
  ChannelsApi,
} from "./contract";
import { purposeBeyond } from "./suggest-title";
import { unfinished } from "./task-state";
import { useChannelFollow } from "./use-channel-follow";
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
 * A channel's log: read page by page from the start, then followed on the channel's stream from the last seq (ADR-0014
 * point 11). A reconnect replays from the last event id, and a message the page already has is not added
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

/** The notices that change a channel's budget or today's spend. */
const BUDGET_NOTICES: ReadonlySet<string> = new Set([
  "wake_budget_set",
  "wake_budget_80",
  "wake_budget_spent",
]);

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
    // Each agent's live state (working, waiting, idle; WEB-U-519) comes with the members: again on focus and every 5 s
    // while shown, so the header's "N working" follows the runs.
    refetchOnWindowFocus: true,
    refetchInterval: 5_000,
  });
  const cache = useQueryClient();
  // An agent starting or stopping work changes the reader's own task list (the sidebar), which reads the tasks again
  // only every 30 s while none runs: asked for at once here, so a post's woken agents show as working (WEB-U-545).
  const runs = (members.data?.items ?? [])
    .filter((item) => item.kind === "conversation")
    .map((item) => `${item.id}:${item.state ?? ""}`)
    .join(",");
  const lastRuns = useRef<string | null>(null);
  useEffect(() => {
    if (!members.isSuccess) return;
    if (lastRuns.current !== null && lastRuns.current !== runs)
      void cache.invalidateQueries({ queryKey: ["conversations"] });
    lastRuns.current = runs;
  }, [runs, members.isSuccess, cache]);
  // An ended channel reads only: its state comes again from the gateway (the stream's channel.state).
  const log = useChannelLog(api, id, () => {
    void cache.invalidateQueries({ queryKey: ["channel", id] });
  });
  // The page opens at the newest message and follows new ones while the reader is at the bottom (WEB-U-541).
  const pageRef = useRef<HTMLDivElement>(null);
  const toNewest = useChannelFollow(
    pageRef,
    log.loaded && channel.isSuccess,
    log.messages.at(-1)?.seq ?? 0,
  );
  /** The panel open beside the log: the to-dos or the members (WEB-U-519), a post's thread (WEB-U-523), or none. */
  const [budgetOpen, setBudgetOpen] = useState(false);
  const [panel, setPanel] = useState<HeaderPanel | { thread: number } | null>(
    null,
  );
  const board = useQuery({
    queryKey: tasksKey(id),
    queryFn: ({ signal }) => api.tasks(id, signal),
    retry: false,
  });
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
        // A task, claim or report changes the board (ADR-0014 point 8).
        if (["task", "claim", "report"].includes(message.body.type))
          void cache.invalidateQueries({ queryKey: tasksKey(id) });
        // A budget notice changes the channel's budget or spend: the header and Wake budget… read it again, so they
        // never disagree with the log (the review of 21 Nov 2026).
        if (
          message.body.type === "notice" &&
          BUDGET_NOTICES.has(message.body.kind)
        )
          void cache.invalidateQueries({ queryKey: ["channel", id] });
      }
    seen.current = log.messages.at(-1)?.seq ?? seen.current;
  }, [log.messages, cache, id]);
  // The viewer's role comes with the channel; Members (read every 5 s) shows a change first, for example the owner
  // making them lead: the channel is read again, so the budget field and Raise the budget follow.
  const listedRole = members.data?.items.find(
    (member) => member.kind === "user" && member.id === me,
  )?.role;
  const channelRole = channel.data?.role;
  useEffect(() => {
    if (
      listedRole !== undefined &&
      channelRole !== undefined &&
      listedRole !== channelRole
    )
      void cache.invalidateQueries({ queryKey: ["channel", id] });
  }, [listedRole, channelRole, cache, id]);
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
  async function post(write: ChannelPost, files: readonly string[] = []) {
    const stored = await api.post(id, write, newActionKey(), files);
    log.add([stored]);
    toNewest();
  }
  async function claim(seq: number) {
    log.add([await api.claim(id, seq, newActionKey())]);
  }
  /** Sets the wake budget and shows the channel as the gateway answers it (C-1, WEB-U-529). */
  async function setBudget(budget: number) {
    cache.setQueryData(["channel", id], await api.setBudget(id, budget));
  }
  async function end() {
    cache.setQueryData(["channel", id], await api.endChannel(id));
  }
  async function remove() {
    await api.deleteChannel(id);
    void cache.invalidateQueries({ queryKey: ["channels"] });
    router.push("/channels");
  }
  const fileHref = (seq: number, index: number) =>
    pathFor(settingsEndpoint("channelFile"), {
      id,
      seq: String(seq),
      index: String(index),
    });
  // ADR-0014 point 13: its owner or an administrator ends or deletes a channel (WEB-U-066).
  const administers = item.owner.id === me || viewer?.role === "admin";
  /** Who sets the wake budget: the owner, an administrator, or a person who leads the channel (C-1). */
  const budgets = administers || item.role === "lead";
  /** The used-up notice still true: only it says when agents wake again and offers Raise the budget. */
  const liveSpent = liveBudgetNotice(log.messages, item);
  const posts =
    item.state === "open" && item.role !== null && item.role !== "observer";
  // WEB-U-063: a new channel's empty log gives a first step, and the box has the focus.
  const firstStep = posts && log.loaded && log.messages.length === 0;
  // WEB-U-250: the title is usually the purpose's start; the line under it says only what it adds.
  const beyond = purposeBeyond(item.purpose, item.name);
  return (
    <div
      ref={pageRef}
      className="channel-page"
      data-panel={
        panel ? (typeof panel === "object" ? "thread" : panel) : undefined
      }
    >
      <TabTitle>
        {t("channels.pageTab", { name: item.name, product: APP_NAME })}
      </TabTitle>
      <ChannelHeader
        channel={item}
        members={members.data?.items ?? []}
        openTasks={
          board.data ? board.data.items.filter(unfinished).length : null
        }
        panel={typeof panel === "string" ? panel : null}
        onPanel={setPanel}
        purpose={beyond || null}
        actions={
          <ChannelActions
            channel={item}
            manage={administers}
            onBudget={() => setBudgetOpen(true)}
            onEnd={end}
            onDelete={remove}
          />
        }
      />
      {budgetOpen && (
        <ChannelBudget
          channel={item}
          may={budgets && item.state === "open"}
          onSave={setBudget}
          onClose={() => setBudgetOpen(false)}
        />
      )}
      <ChannelTimeline
        messages={log.messages}
        liveSpent={liveSpent}
        members={members.data?.items ?? []}
        fileHref={fileHref}
        onThread={(seq) => setPanel({ thread: seq })}
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
          ) : message.body.type === "notice" &&
            message.body.kind === "wake_budget_spent" &&
            message.seq === liveSpent &&
            budgets &&
            item.state === "open" ? (
            // What the used-up notice says to do can be done here (C-1): for those who may.
            <Button variant="secondary" onClick={() => setBudgetOpen(true)}>
              {t("channels.budgetRaise")}
            </Button>
          ) : null
        }
        {...(item.state === "open" &&
        item.role !== null &&
        item.role !== "observer"
          ? { onReply: (seq: number) => setPanel({ thread: seq }) }
          : {})}
      />
      {firstStep && (
        <ChannelFirstStep
          purpose={item.purpose}
          onUse={(text) => setDraft(({ n }) => ({ n: n + 1, text }))}
        />
      )}
      {typeof panel === "object" && panel !== null && (
        <ChannelThread
          seq={panel.thread}
          messages={log.messages}
          members={members.data?.items ?? []}
          fileHref={fileHref}
          posts={posts}
          onPost={post}
          onClose={() => setPanel(null)}
        />
      )}
      {typeof panel === "string" && (
        <ChannelSide
          view={panel}
          onClose={() => setPanel(null)}
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
      )}
      <div className="channel-composer-dock">
        <ChannelComposer
          key={draft.n}
          initialText={draft.text}
          focusBox={firstStep || draft.n > 0}
          memberRole={item.role}
          state={item.state}
          replyTo={null}
          onCancelReply={() => undefined}
          onUpload={(file) => api.upload(id, file)}
          onRemoveUpload={(upload) => api.removeUpload(id, upload)}
          onPost={post}
        />
      </div>
    </div>
  );
}

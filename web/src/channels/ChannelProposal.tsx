"use client";

import { useQuery } from "@tanstack/react-query";

import type { ChannelPost, ChannelRole, ChannelsApi, Member } from "./contract";
import { ProposalPanel } from "./ProposalPanel";

/** The query key of one proposal's state, refetched when a vote or decision on it reaches the log. */
export const proposalKey = (id: string, seq: number) => [
  "channel-proposal",
  id,
  seq,
];

/** A proposal's state from the gateway, under its post (WEB-U-064). */
export function ChannelProposal({
  api,
  id,
  seq,
  members,
  me,
  memberRole,
  ended,
  onVote,
}: {
  api: ChannelsApi;
  id: string;
  seq: number;
  members: readonly Member[];
  me: string;
  memberRole: ChannelRole | null;
  ended: boolean;
  onVote: (write: ChannelPost) => Promise<void>;
}) {
  const state = useQuery({
    queryKey: proposalKey(id, seq),
    queryFn: ({ signal }) => api.proposal(id, seq, signal),
    retry: false,
  });
  if (!state.data) return null;
  return (
    <ProposalPanel
      proposal={state.data}
      members={members}
      me={me}
      memberRole={memberRole}
      ended={ended}
      onVote={onVote}
    />
  );
}

import { api, type ApiClient } from "../api/client";
import { pathFor } from "../api/operations";
import { openEventStream } from "../api/stream";
import { parseApi } from "../protocol/validate";
import { settingsEndpoint } from "../settings/operations";

import type { ChannelsApi } from "./contract";
import "../protocol/generated/validators.channels";

/** The channel pages' API on one client (the gateway's human channels API; ADR-0014). */
export function gatewayChannelsFor(client: ApiClient): ChannelsApi {
  return {
    channels: (signal) =>
      client.request(
        settingsEndpoint("channels"),
        (value) => parseApi(value, "ChannelList"),
        { signal },
      ),
    createChannel: (write, key) =>
      client.request(
        settingsEndpoint("channelCreate"),
        (value) => parseApi(value, "Channel"),
        { body: JSON.stringify(write), key },
      ),
    channel: (id, signal) =>
      client.request(
        settingsEndpoint("channel"),
        (value) => parseApi(value, "Channel"),
        { parameters: { id }, signal },
      ),
    members: (id, signal) =>
      client.request(
        settingsEndpoint("channelMembers"),
        (value) => parseApi(value, "ChannelMembers"),
        { parameters: { id }, signal },
      ),
    messages: (id, since, signal) =>
      client.request(
        settingsEndpoint("channelMessages"),
        (value) => parseApi(value, "ChannelMessagePage"),
        {
          parameters: { id },
          query: new URLSearchParams({ since: String(since), limit: "200" }),
          signal,
        },
      ),
    stream: (id, after) =>
      openEventStream(
        pathFor(
          settingsEndpoint("channelEvents"),
          { id },
          new URLSearchParams({ stream: "1", after: String(after) }),
        ),
      ),
    proposal: (id, seq, signal) =>
      client.request(
        settingsEndpoint("channelProposal"),
        (value) => parseApi(value, "ChannelProposalState"),
        { parameters: { id, seq: String(seq) }, signal },
      ),
    tasks: (id, signal) =>
      client.request(
        settingsEndpoint("channelTasks"),
        (value) => parseApi(value, "ChannelTaskBoard"),
        { parameters: { id }, signal },
      ),
    claim: (id, seq, key) =>
      client.request(
        settingsEndpoint("channelClaim"),
        (value) => parseApi(value, "ChannelMessage"),
        { parameters: { id, seq: String(seq) }, key },
      ),
    post: (id, write, key) =>
      client.request(
        settingsEndpoint("channelPost"),
        (value) => parseApi(value, "ChannelMessage"),
        { parameters: { id }, body: JSON.stringify(write), key },
      ),
    setMember: (id, kind, member, role) =>
      client.request(
        settingsEndpoint("channelMemberPut"),
        (value) => parseApi(value, "ChannelMember"),
        { parameters: { id, kind, member }, body: JSON.stringify({ role }) },
      ),
    removeMember: (id, kind, member) =>
      client.request(settingsEndpoint("channelMemberDelete"), () => undefined, {
        parameters: { id, kind, member },
      }),
    myTasks: async (signal) =>
      (await client.conversations(undefined, signal)).items
        .filter((item) => item.state === "active")
        .map((item) => ({
          id: item.id,
          ...(item.title ? { title: item.title } : {}),
          ...(item.last_activity_at
            ? { active_at: item.last_activity_at }
            : {}),
        })),
    endChannel: (id) =>
      client.request(
        settingsEndpoint("channelEnd"),
        (value) => parseApi(value, "Channel"),
        { parameters: { id } },
      ),
    deleteChannel: (id) =>
      client.request(settingsEndpoint("channelDelete"), () => undefined, {
        parameters: { id },
      }),
    addPerson: (id, email, role) =>
      client.request(
        settingsEndpoint("channelMemberByEmail"),
        (value) => parseApi(value, "ChannelMember"),
        { parameters: { id }, body: JSON.stringify({ email, role }) },
      ),
  };
}

export const gatewayChannels: ChannelsApi = gatewayChannelsFor(api);

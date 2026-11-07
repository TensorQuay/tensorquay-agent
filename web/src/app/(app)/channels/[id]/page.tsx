"use client";

import { useParams } from "next/navigation";

import { ChannelPage } from "../../../../channels/ChannelPage";
import { gatewayChannels } from "../../../../channels/gateway";
import { PanelBoundary } from "../../../../ui/PanelBoundary";

export default function Channel() {
  const { id } = useParams<{ id: string }>();
  return (
    <PanelBoundary key={id}>
      <ChannelPage id={id} api={gatewayChannels} />
    </PanelBoundary>
  );
}

"use client";

import { ChannelsPage } from "../../../channels/ChannelsPage";
import { gatewayChannels } from "../../../channels/gateway";
import { PanelBoundary } from "../../../ui/PanelBoundary";

export default function Channels() {
  return (
    <PanelBoundary>
      <ChannelsPage api={gatewayChannels} />
    </PanelBoundary>
  );
}

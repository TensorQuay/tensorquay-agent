import { beforeEach, describe, expect, it, vi } from "vitest";

const client = vi.hoisted(() => ({
  models: vi.fn(() => Promise.resolve({ items: [] })),
  templates: vi.fn(() => Promise.resolve({ items: [] })),
  request: vi.fn(() => Promise.resolve({ items: [] })),
}));
const credentials = vi.hoisted(() => ({
  connectors: vi.fn(() => Promise.resolve({ items: [] })),
}));
vi.mock("../api/client", () => ({ api: client }));
vi.mock("./connector-credentials", () => ({ gatewayCredentials: credentials }));

import { gatewayProfileChoices } from "./profile-gateway";

beforeEach(() => vi.clearAllMocks());

// WEB-U-104: the list and each profile's page ask the gateway for the same choices, each from its own call.
describe("WEB-U-104 a profile's choices from the gateway", () => {
  it("asks each choice of the call that answers it, passing the page's signal", async () => {
    const signal = new AbortController().signal;
    await gatewayProfileChoices.models(signal);
    expect(client.models).toHaveBeenCalledExactlyOnceWith(signal);
    await gatewayProfileChoices.templates(signal);
    expect(client.templates).toHaveBeenCalledExactlyOnceWith(signal);
    await gatewayProfileChoices.connectors(signal);
    expect(credentials.connectors).toHaveBeenCalledExactlyOnceWith(signal);
    await gatewayProfileChoices.servers?.(signal);
    await gatewayProfileChoices.engines?.(signal);
    expect(
      client.request.mock.calls.map(
        (call: unknown[]) => (call[0] as { path: string }).path,
      ),
    ).toEqual(["/api/v1/admin/mcp/servers", "/api/v1/admin/decision/engines"]);
  });
});

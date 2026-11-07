import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { fakeGateway, me } from "../test/workspaces";

import { NewConversation } from "./NewConversation";

vi.mock("next/navigation", () => ({
  useRouter: () => ({ push: vi.fn() }),
  usePathname: () => "/",
}));

afterEach(() => vi.unstubAllGlobals());

const desktop = {
  name: "desktop",
  display_name: "Desktop",
  capabilities: ["shell", "browser", "desktop"],
  is_default: true,
  description: null,
  cpus: 2,
  memory_mib: 4096,
};
const workstation = {
  ...desktop,
  name: "workstation",
  display_name: "Workstation",
  capabilities: ["shell"],
};
const route = (vision: boolean) => ({
  name: "local",
  display_name: "Local model",
  description: null,
  verified: true,
  model: "local-model",
  protocol: "openai_chat",
  context_window: null,
  vision,
  native_tools: true,
  reasoning: null,
  runs_on: { where: "this_computer", provider: null },
});

function start(environment: object, vision: boolean) {
  const gateway = fakeGateway({
    "GET /api/v1/auth/me": () => Response.json({ ...me, internet: "some" }),
    "GET /api/v1/models": () => Response.json({ items: [route(vision)] }),
    "GET /api/v1/templates": () => Response.json({ items: [environment] }),
    "GET /api/v1/workspaces": () => Response.json({ items: [] }),
  });
  vi.stubGlobal("fetch", gateway.fetch);
  render(
    <QueryClientProvider
      client={
        new QueryClient({ defaultOptions: { queries: { retry: false } } })
      }
    >
      <NewConversation />
    </QueryClientProvider>,
  );
}

const BLIND =
  "This model can't see images, so on a Desktop the agent can't see the screen. Choose a model that can.";

// WEB-U-370 (16e's first hour, 8b): a Desktop with a route that can't see images leaves the agent blind, and nothing
// said so; New task now says it in one line where the person picks them.
describe("WEB-U-370 a Desktop with a model that can't see", () => {
  it("says so in one line", async () => {
    start(desktop, false);
    expect(await screen.findByText(BLIND)).toBeVisible();
  });

  it("says nothing when the model can see", async () => {
    start(desktop, true);
    await screen.findByRole("textbox", { name: "Message" });
    expect(screen.queryByText(BLIND)).toBeNull();
  });

  it("says nothing off a Desktop", async () => {
    start(workstation, false);
    await screen.findByRole("textbox", { name: "Message" });
    expect(screen.queryByText(BLIND)).toBeNull();
  });
});

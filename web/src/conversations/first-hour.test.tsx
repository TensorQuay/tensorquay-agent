import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { templates } from "../test/templates";
import { fakeGateway, me } from "../test/workspaces";

import { NewConversation } from "./NewConversation";

vi.mock("next/navigation", () => ({
  useRouter: () => ({ push: vi.fn() }),
  usePathname: () => "/",
}));

afterEach(() => vi.unstubAllGlobals());

const route = {
  name: "local",
  model: "local-model",
  protocol: "openai_chat",
  display_name: null,
  verified: null,
  context_window: 32768,
  vision: false,
  native_tools: true,
  runs_on: { where: "hosted", provider: null },
  description: null,
  reasoning: null,
};

function start(
  person: object,
  {
    models = [route],
    offered = templates,
  }: { models?: object[]; offered?: { items: readonly object[] } } = {},
) {
  const gateway = fakeGateway({
    "GET /api/v1/auth/me": () => Response.json(person),
    "GET /api/v1/models": () => Response.json({ items: models }),
    "GET /api/v1/templates": () => Response.json(offered),
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

const admin = { ...me, role: "admin", internet: "some" };

// The c16 first hour (P2-1): on a fresh install the person reading New task is often the administrator, so a missing
// model or environment is said to them as theirs to add, with the way there; a member is still told to ask (WEB-U-317).
describe("WEB-U-317 an administrator is never told to ask an administrator", () => {
  it("tells an administrator with no model route where to add one", async () => {
    start(admin, { models: [] });
    expect(
      await screen.findByText(
        "No model routes are set up yet: add one in Settings › Administration › Models.",
      ),
    ).toBeVisible();
    expect(screen.queryByText(/Ask your administrator/)).toBeNull();
    expect(
      screen.getByRole("link", { name: "Add a model route" }),
    ).toHaveAttribute("href", "/settings/models");
  });

  it("tells an administrator with no environment where to look", async () => {
    start(admin, { offered: { items: [] } });
    expect(
      await screen.findByText(
        "No environments are available yet: see Settings › Administration › Environments.",
      ),
    ).toBeVisible();
    expect(screen.queryByText(/Ask your administrator/)).toBeNull();
    expect(
      screen.getByRole("link", { name: "Open Environments" }),
    ).toHaveAttribute("href", "/settings/environments");
  });

  it("still tells a member to ask, with no settings link", async () => {
    start({ ...me, internet: "some" }, { models: [] });
    expect(
      await screen.findByText(
        "No model routes are available. Ask your administrator to configure one.",
      ),
    ).toBeVisible();
    expect(
      screen.queryByRole("link", { name: "Add a model route" }),
    ).toBeNull();
  });
});

// The c16 first hour (P2-2): with agent teams on, a team starts when a person asks a task to lead, so one example says
// how; with them off, nothing about teams shows (WEB-U-318).
describe("WEB-U-318 New task shows how to start a team while agent teams are on", () => {
  const TEAM =
    "Lead two helpers: have each compare one note-taking app, then recommend one";

  it("offers a team example while agent teams are on", async () => {
    start({ ...me, internet: "some", features: { agent_teams: true } });
    expect(await screen.findByRole("button", { name: TEAM })).toBeVisible();
  });

  it("offers none while they're off", async () => {
    start({ ...me, internet: "some" });
    await screen.findByRole("button", {
      name: "Make a sample sales spreadsheet and chart the totals by region",
    });
    expect(screen.queryByRole("button", { name: TEAM })).toBeNull();
  });
});

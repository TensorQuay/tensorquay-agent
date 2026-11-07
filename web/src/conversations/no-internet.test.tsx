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

function start(person: object) {
  const gateway = fakeGateway({
    "GET /api/v1/auth/me": () => Response.json(person),
    "GET /api/v1/models": () => Response.json({ items: [] }),
    "GET /api/v1/templates": () => Response.json(templates),
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

const NONE =
  "Tasks can't reach the internet: no sites are allowed yet. An administrator can allow them.";

// WEB-U-119 (the first hour's F20): a fresh install reaches nothing, so New task says so before a task fails on it.
describe("WEB-U-119 New task says when tasks have no internet", () => {
  it("tells a member in plain words, with no hosts and no settings link", async () => {
    start({ ...me, internet: "none" });
    expect(await screen.findByText(NONE)).toBeVisible();
    expect(screen.queryByRole("link", { name: "Allow sites" })).toBeNull();
  });

  // TEST: WEB-U-153
  // WEB-U-153 (the 13f screen review, #2): an administrator isn't told to ask an administrator.
  it("gives an administrator the way to allow sites, in words for them", async () => {
    start({ ...me, role: "admin", internet: "none" });
    await screen.findByText(
      "Tasks can't reach the internet: no sites are allowed yet.",
    );
    expect(screen.queryByText(/An administrator can allow them/)).toBeNull();
    expect(screen.getByRole("link", { name: "Allow sites" })).toHaveAttribute(
      "href",
      "/settings/internet-access",
    );
  });

  it("says nothing when some sites are allowed", async () => {
    start({ ...me, internet: "some" });
    await screen.findByRole("textbox", { name: "Message" });
    expect(screen.queryByText(NONE)).toBeNull();
  });
});

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, within } from "@testing-library/react";
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
  const cache = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  // The person is known from the first render, as in the app's shell, so what New task leaves out is left out for them.
  cache.setQueryData(["session"], person);
  render(
    <QueryClientProvider client={cache}>
      <NewConversation />
    </QueryClientProvider>,
  );
}

const NOTICE = "Tasks can't reach the internet: no sites are allowed yet.";
const MEMBER =
  "Tasks can't reach the internet: no sites are allowed yet. Ask an administrator to allow sites.";

// WEB-U-119 and WEB-U-153: a fresh install reaches nothing, so New task says so before a task fails on it, in an inline
// note: an administrator gets the way to allow sites; a member, who can't, is told to ask one, with no action (17.3).
describe("WEB-U-119 New task says when tasks have no internet", () => {
  it("tells a member in the same note, with no action, to ask an administrator", async () => {
    start({ ...me, internet: "none" });
    const line = await screen.findByText(MEMBER);
    const notice = line.closest(".start-notice");
    if (!(notice instanceof HTMLElement)) throw new Error("No notice");
    expect(notice.querySelector("svg")).not.toBeNull();
    expect(within(notice).queryByRole("link")).toBeNull();
    expect(screen.queryByRole("link", { name: "Allow sites" })).toBeNull();
  });

  // TEST: WEB-U-153
  it("gives an administrator an inline notice: an info icon, one line, and Allow sites as a small ghost button", async () => {
    start({ ...me, role: "admin", internet: "none" });
    const line = await screen.findByText(NOTICE);
    const notice = line.closest(".start-notice");
    if (!(notice instanceof HTMLElement)) throw new Error("No notice");
    expect(notice.querySelector("svg")).not.toBeNull();
    expect(screen.queryByText(/An administrator can allow them/)).toBeNull();
    const allow = within(notice).getByRole("link", { name: "Allow sites" });
    expect(allow).toHaveAttribute("href", "/settings/internet-access");
    expect(allow).toHaveClass("button");
    expect(allow).toHaveAttribute("data-variant", "ghost");
  });

  it("says nothing when some sites are allowed", async () => {
    start({ ...me, role: "admin", internet: "some" });
    await screen.findByRole("textbox", { name: "Message" });
    expect(screen.queryByText(NOTICE)).toBeNull();
  });
});

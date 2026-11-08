import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import en from "../i18n/locales/en.json";
import { templates } from "../test/templates";
import { fakeGateway, me } from "../test/workspaces";

import { NewConversation } from "./NewConversation";

vi.mock("next/navigation", () => ({
  useRouter: () => ({ push: vi.fn() }),
  usePathname: () => "/",
}));

afterEach(() => vi.unstubAllGlobals());

/**
 * New task with the sidebar's list of the person's tasks as the shell has loaded it (`tasks` of them), or none loaded
 * (`null`). New task reads that list and never asks for one of its own.
 */
function start(tasks: number | null) {
  const gateway = fakeGateway({
    "GET /api/v1/auth/me": () => Response.json({ ...me, internet: "some" }),
    "GET /api/v1/models": () => Response.json({ items: [] }),
    "GET /api/v1/templates": () => Response.json(templates),
    "GET /api/v1/workspaces": () => Response.json({ items: [] }),
  });
  vi.stubGlobal("fetch", gateway.fetch);
  const cache = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  if (tasks !== null)
    cache.setQueryData(["conversations"], {
      pages: [
        {
          items: Array.from({ length: tasks }, (_, index) => ({
            id: `task-${index}`,
          })),
          next_cursor: null,
        },
      ],
      pageParams: [""],
    });
  render(
    <QueryClientProvider client={cache}>
      <NewConversation />
    </QueryClientProvider>,
  );
  return gateway;
}
const placeholder = en.messages["composer.placeholderNew"];
const message = () => screen.findByRole("textbox", { name: "Message" });

// WEB-U-431: New task's placeholder carries the keyboard hint while the person has no tasks yet; their first message
// creates their first task, and the hint goes.
describe("WEB-U-431 New task's keyboard hint", () => {
  it("is in the placeholder while the person has no tasks yet", async () => {
    start(0);
    const input = await message();
    await waitFor(() =>
      expect(input).toHaveAttribute(
        "placeholder",
        `${placeholder}\n${en.messages["composer.hint"]}`,
      ),
    );
  });

  it("is gone once the person has a task", async () => {
    start(1);
    expect(await message()).toHaveAttribute("placeholder", placeholder);
  });

  it("isn't there while no list is known, and New task asks for none", async () => {
    const gateway = start(null);
    expect(await message()).toHaveAttribute("placeholder", placeholder);
    expect(
      gateway.calls.some((call) =>
        call.url.startsWith("/api/v1/conversations"),
      ),
    ).toBe(false);
  });
});

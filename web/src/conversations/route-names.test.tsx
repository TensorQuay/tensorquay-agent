import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { openChoices } from "../test/start-choices";
import { templates } from "../test/templates";
import { fakeGateway, me, task } from "../test/workspaces";

import { NewConversation } from "./NewConversation";

/** Opens New task's Change panel, then finds a picker in it by name. */
async function pick(name: string) {
  await openChoices();
  return screen.findByRole("button", { name });
}

const push = vi.fn();
vi.mock("next/navigation", () => ({
  useRouter: () => ({ push }),
  usePathname: () => "/",
}));

const route = (name: string, extra: object) => ({
  name,
  protocol: "openai_chat",
  description: null,
  context_window: 32768,
  vision: false,
  native_tools: true,
  runs_on: { where: "this_computer", provider: null },
  reasoning: null,
  ...extra,
});
// 15c's known issue: two routes one administrator named "Qwen", the first by name failing its check.
const models = {
  items: [
    route("qwen-a", {
      display_name: "Qwen",
      model: "qwen3-4b",
      verified: false,
    }),
    route("qwen-b", {
      display_name: "Qwen",
      model: "qwen3-8b",
      verified: true,
    }),
  ],
};
const created = task(
  "0195d8c0-0000-7000-8000-0000000000f2",
  "0195d8c0-0000-7000-8000-0000000000f2",
);

function start() {
  const gateway = fakeGateway({
    "GET /api/v1/auth/me": () => Response.json(me),
    "GET /api/v1/models": () => Response.json(models),
    "GET /api/v1/templates": () => Response.json(templates),
    "GET /api/v1/workspaces": () => Response.json({ items: [] }),
    "POST /api/v1/conversations": () => Response.json(created, { status: 201 }),
    [`POST /api/v1/conversations/${created.id}/messages`]: () =>
      Response.json(
        {
          accepted: true,
          conversation_id: created.id,
          message_id: "1",
          held: false,
        },
        { status: 202 },
      ),
  });
  vi.stubGlobal("fetch", gateway.fetch);
  const cache = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  render(
    <QueryClientProvider client={cache}>
      <NewConversation />
    </QueryClientProvider>,
  );
  return gateway;
}

function send(text: string) {
  fireEvent.change(screen.getByRole("textbox", { name: "Message" }), {
    target: { value: text },
  });
  fireEvent.click(screen.getByRole("button", { name: "Send" }));
}

const createdWith = (calls: { url: string; body?: unknown }[]) =>
  calls.find((call) => call.url === "/api/v1/conversations")?.body;

afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
  push.mockReset();
});

describe("WEB-U-321/322 New task's model choice with two routes of one name", () => {
  it("starts on the route that passed its check, named apart from the other, and says nothing of checks when closed", async () => {
    const { calls } = start();
    const button = await pick("Model: Qwen · Qwen3 8b");
    expect(button).not.toHaveTextContent("check");
    send("Hello");
    await waitFor(() => expect(push).toHaveBeenCalled());
    expect(createdWith(calls)).toMatchObject({ model_route: "qwen-b" });
  });

  it("lists both apart, the failing one saying so on its row, and keeps a person's own pick of it", async () => {
    const { calls } = start();
    fireEvent.click(await pick("Model: Qwen · Qwen3 8b"));
    const failing = screen.getByRole("menuitemradio", {
      name: /^Qwen · Qwen3 4b/,
    });
    expect(
      within(failing).getByText("Its last check didn't pass"),
    ).toBeVisible();
    const passing = screen.getByRole("menuitemradio", {
      name: /^Qwen · Qwen3 8b/,
    });
    expect(
      within(passing).queryByText("Its last check didn't pass"),
    ).toBeNull();
    fireEvent.click(failing);
    await screen.findByRole("button", { name: "Model: Qwen · Qwen3 4b" });
    send("Hello");
    await waitFor(() => expect(push).toHaveBeenCalled());
    expect(createdWith(calls)).toMatchObject({ model_route: "qwen-a" });
  });
});

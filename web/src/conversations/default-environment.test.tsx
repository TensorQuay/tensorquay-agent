import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { conversationId } from "../test/frames";
import { openChoices } from "../test/start-choices";
import { openTaskChoices } from "../test/start-choices";
import { templates } from "../test/templates";
import { fakeGateway, me, problem, task } from "../test/workspaces";
import { emptyTranscript } from "../transcript/reducer";

import { ConversationControls } from "./ConversationControls";
import { NewConversation } from "./NewConversation";
import { TaskChoices } from "./TaskChoices";

/** Opens New task's Change panel, then finds a picker in it by name. */
async function pick(name: string) {
  await openChoices();
  return screen.findByRole("button", { name });
}

/** Opens the task's model and environment panel from its header, then finds a picker in it by name. */
async function inHeader(name: string) {
  await openTaskChoices();
  return screen.findByRole("button", { name });
}

const push = vi.fn();
vi.mock("next/navigation", () => ({
  useRouter: () => ({ push }),
  usePathname: () => "/",
}));

const models = {
  items: [
    {
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
    },
  ],
};
// The gateway lists only offered environments, the default first; a default that isn't offered is left out.
const withoutDefault = {
  items: [
    {
      name: "minimal",
      display_name: "Minimal",
      capabilities: ["shell"],
      is_default: false,
      description: null,
      cpus: 1,
      memory_mib: 512,
    },
    {
      name: "python",
      display_name: "Python",
      capabilities: ["shell", "python"],
      is_default: false,
      description: null,
      cpus: 1,
      memory_mib: 512,
    },
  ],
};
const created = task(
  "0195d8c0-0000-7000-8000-0000000000f1",
  "0195d8c0-0000-7000-8000-0000000000f1",
);

function start(
  lists: object[],
  create: () => Response = () => Response.json(created, { status: 201 }),
) {
  let listed = 0;
  const gateway = fakeGateway({
    "GET /api/v1/auth/me": () => Response.json(me),
    "GET /api/v1/models": () => Response.json(models),
    "GET /api/v1/templates": () =>
      Response.json(lists[Math.min(listed++, lists.length - 1)]),
    "GET /api/v1/workspaces": () => Response.json({ items: [] }),
    "POST /api/v1/conversations": create,
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

const templateCalls = (calls: { method: string; url: string }[]) =>
  calls.filter((call) => call.url === "/api/v1/templates").length;

afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
  push.mockReset();
});

// ADR-0032: a default that isn't offered is missing from the list; the new task must not be left unable to send.
describe("WEB-U-048 a new task when the default environment is not offered", () => {
  it("preselects the default while it is offered", async () => {
    const { calls } = start([templates]);
    await pick("Environment: Workstation");
    send("Hello");
    await waitFor(() => expect(push).toHaveBeenCalled());
    expect(
      calls.find((call) => call.url === "/api/v1/conversations")?.body,
    ).toMatchObject({ template: "workstation" });
  });

  // Under the pickers, what the chosen environment is for; its tools on hover.
  it("says what the chosen environment is for, with its tools on hover", async () => {
    start([
      {
        items: [
          {
            ...withoutDefault.items[0],
            description: "Documents, data and web pages",
          },
        ],
      },
    ]);
    // On its line in the picker (the composer's; no panel under it), with its tools on hover.
    fireEvent.click(await pick("Environment: Minimal"));
    const purpose = within(
      screen.getByRole("menuitemradio", { name: /^Minimal/ }),
    ).getByText(/^Documents, data and web pages/);
    expect(purpose).toHaveAttribute("title", "Shell");
  });

  it("preselects the first offered environment, shown before sending", async () => {
    const { calls } = start([withoutDefault]);
    await pick("Environment: Minimal");
    expect(screen.queryByText(/No environments are available/)).toBeNull();
    send("Hello");
    await waitFor(() => expect(push).toHaveBeenCalled());
    expect(
      calls.find((call) => call.url === "/api/v1/conversations")?.body,
    ).toMatchObject({ template: "minimal" });
  });

  it("says so when no environment is offered", async () => {
    start([{ items: [] }]);
    expect(
      await screen.findByText(
        "No environments are available. Ask your administrator to configure one.",
      ),
    ).toBeInTheDocument();
  });

  it("reads template_unavailable on the first message, refreshes the list and moves off the gone choice", async () => {
    let refused = false;
    const { calls } = start([templates, withoutDefault], () => {
      if (refused) return Response.json(created, { status: 201 });
      refused = true;
      return problem(409, "template_unavailable");
    });
    await pick("Environment: Workstation");
    send("Hello");
    expect(
      await screen.findByText(
        /This environment is no longer available\. Choose another environment\./,
      ),
    ).toBeInTheDocument();
    expect(screen.queryByText("template_unavailable")).toBeNull();
    await pick("Environment: Minimal");
    expect(templateCalls(calls)).toBe(2);
    fireEvent.click(screen.getByRole("button", { name: /Retry/ }));
    await waitFor(() => expect(push).toHaveBeenCalled());
    expect(
      calls.filter((call) => call.url === "/api/v1/conversations").at(-1)?.body,
    ).toMatchObject({ template: "minimal" });
  });
});

describe("WEB-U-048 switching to an environment that is no longer offered", () => {
  it("reads template_unavailable like an unknown environment and refreshes the list", async () => {
    const conversation = {
      id: conversationId,
      owner_id: conversationId,
      title: "Environment review",
      model_route: "local",
      template: "minimal",
      pinned: false,
      title_source: "message" as const,
      backend: "docker",
      state: "active",
      created_at: "2026-01-01T00:00:00Z",
      updated_at: "2026-01-01T00:00:00Z",
      last_activity_at: "2026-01-01T00:00:00Z",
      workspace_id: "0195d8c0-0000-7000-8000-0000000000ff",
      mcp_servers: [] as string[],
      connectors: [] as string[],
      team: null,
    } as const;
    const gateway = fakeGateway({
      [`PUT /api/v1/conversations/${conversationId}/environment`]: () =>
        problem(409, "template_unavailable"),
      "GET /api/v1/templates": () => Response.json(withoutDefault),
    });
    vi.stubGlobal("fetch", gateway.fetch);
    const cache = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    cache.setQueryData(["templates"], templates);
    cache.setQueryData(["models"], { items: [] });
    render(
      <QueryClientProvider client={cache}>
        <>
          <ConversationControls
            conversation={conversation}
            transcript={{
              ...emptyTranscript(),
              sandbox: "ready",
              run: { id: null, status: "idle" },
            }}
          />
          <TaskChoices
            conversation={conversation}
            transcript={{
              ...emptyTranscript(),
              sandbox: "ready",
              run: { id: null, status: "idle" },
            }}
          />
        </>
      </QueryClientProvider>,
    );
    fireEvent.click(await inHeader("Environment: Minimal"));
    fireEvent.click(
      await screen.findByRole("menuitemradio", { name: /^Workstation/ }),
    );
    fireEvent.click(
      await screen.findByRole("button", { name: "Switch to Workstation" }),
    );
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "This environment is no longer available. Choose another environment.",
    );
    await waitFor(() => expect(templateCalls(gateway.calls)).toBe(1));
  });
});

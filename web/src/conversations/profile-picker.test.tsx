import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { profile } from "../test/profiles";
import { addFromPlus, openChoices } from "../test/start-choices";
import { templates } from "../test/templates";
import { fakeGateway, me, problem, task } from "../test/workspaces";

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

const models = {
  items: ["local", "cloud"].map((name) => ({
    name,
    model: `${name}-model`,
    protocol: "openai_chat",
    display_name: null,
    verified: null,
    context_window: 32768,
    vision: false,
    native_tools: true,
    runs_on: { where: "hosted", provider: null },
    description: null,
    reasoning: null,
  })),
};
const team = profile("team", {
  display_name: "Team default",
  description: "Our shared setup",
  shared: true,
  model_route: "cloud",
  template: "minimal",
});
const notes = profile("notes", { display_name: "Notes only" });
const legacy = profile("legacy", {
  display_name: "Legacy",
  shared: true,
  usable: false,
  unusable_field: "template",
});
const created = task(
  "0195d8c0-0000-7000-8000-0000000000f1",
  "0195d8c0-0000-7000-8000-0000000000f1",
);

function start(
  create: () => Response = () => Response.json(created, { status: 201 }),
  session: () => Response | Promise<Response> = () => Response.json(me),
) {
  const gateway = fakeGateway({
    "GET /api/v1/auth/me": session,
    "GET /api/v1/models": () => Response.json(models),
    "GET /api/v1/templates": () => Response.json(templates),
    "GET /api/v1/workspaces": () => Response.json({ items: [] }),
    "GET /api/v1/profiles": () =>
      Response.json({ items: [notes, team, legacy] }),
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
async function choose(name: RegExp) {
  // "+" adds the profile chip, its picker open at once (the UI standard's composer).
  await addFromPlus("Agent profile…");
  fireEvent.click(await screen.findByRole("menuitemradio", { name }));
}
function send(text: string) {
  fireEvent.change(screen.getByRole("textbox", { name: "Message" }), {
    target: { value: text },
  });
  fireEvent.click(screen.getByRole("button", { name: "Send" }));
}
const createBody = (calls: { url: string; body: unknown }[]) =>
  calls.filter((call) => call.url === "/api/v1/conversations").at(-1)?.body;

afterEach(() => {
  vi.unstubAllGlobals();
  push.mockReset();
});

// ADR-0012: a new task can start from a profile, which fixes what it sets (WEB-U-050).
describe("WEB-U-050 the profile picker on New task", () => {
  it("offers None and the usable profiles only, with their descriptions", async () => {
    start();
    // "+" adds the profile chip, its picker open at once (the UI standard's composer).
    await addFromPlus("Agent profile…");
    const names = (await screen.findAllByRole("menuitemradio")).map(
      (item) => item.textContent,
    );
    expect(names).toEqual([
      "None",
      expect.stringContaining("Notes only"),
      expect.stringContaining("Team default"),
    ]);
    expect(names[2]).toContain("Our shared setup");
  });

  it("fixes the model and environment a profile sets, and sends the profile without them", async () => {
    const { calls } = start();
    await choose(/Team default/);
    const model = await pick("Model: Cloud model");
    expect(model).toHaveAttribute("aria-disabled", "true");
    expect(model).toHaveAccessibleDescription(
      "Set by the agent profile Team default",
    );
    expect(
      screen.getByRole("button", { name: "Environment: Minimal" }),
    ).toHaveAccessibleDescription("Set by the agent profile Team default");
    send("Hello");
    await waitFor(() => expect(push).toHaveBeenCalled());
    const body = createBody(calls);
    expect(body).toMatchObject({ profile: team.id });
    expect(body).not.toHaveProperty("model_route");
    expect(body).not.toHaveProperty("template");
  });

  it("leaves the pickers free when the profile sets neither", async () => {
    const { calls } = start();
    await choose(/Notes only/);
    expect(await pick("Model: Local model")).not.toHaveAttribute(
      "aria-disabled",
      "true",
    );
    send("Hello");
    await waitFor(() => expect(push).toHaveBeenCalled());
    expect(createBody(calls)).toMatchObject({
      profile: notes.id,
      model_route: "local",
      template: "workstation",
    });
  });

  it("reads profile_unusable in words and refreshes the profiles", async () => {
    const { calls } = start(() => problem(409, "profile_unusable"));
    await choose(/Team default/);
    send("Hello");
    expect(
      await screen.findByText(
        /This agent profile can't be used now\. Choose another one\./,
      ),
    ).toBeInTheDocument();
    await waitFor(() =>
      expect(
        calls.filter((call) => call.url === "/api/v1/profiles"),
      ).toHaveLength(2),
    );
  });
});

// WEB-U-050 failed under load: "+" gains "Agent profile…" only once the profiles arrive, and they were asked for only
// after the session answered, two round trips on a fresh load (a person's too). Its choices are asked for at once now.
describe("WEB-U-196 New task asks for its choices without waiting for the session", () => {
  it("requests the profiles, models and environments while the session is still unanswered", async () => {
    const { calls } = start(undefined, () => new Promise<Response>(() => {}));
    await waitFor(() =>
      expect(calls.map((call) => call.url)).toEqual(
        expect.arrayContaining([
          "/api/v1/profiles",
          "/api/v1/models",
          "/api/v1/templates",
        ]),
      ),
    );
  });
});

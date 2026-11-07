import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { showModels } from "../test/models-app";
import { choose as choosePicked, chosen, picker } from "../test/picker";
import { fakeRoutes, probeResult, route } from "../test/routes";

import type { RoutesApi } from "./admin/routes";
import {
  fromDraft,
  gatewayProblems,
  toDraft,
  withProtocol,
} from "./route-copy";

/** Replaced by the harness once it renders (showModels). */
const router = vi.hoisted(() => ({ push: vi.fn<(href: string) => void>() }));
vi.mock("next/navigation", () => ({ useRouter: () => router }));
vi.mock("next/link", async () => {
  const { createElement } = await import("react");
  return {
    default: ({ href, children, ...rest }: Record<string, unknown>) =>
      createElement("a", { href, ...rest }, children as never),
  };
});

function show(api: RoutesApi) {
  showModels(api, router);
}
/** The route's page (WEB-U-115): its form, from the title down to Save and Cancel. */
const page = () => {
  const form = screen
    .getByRole("group", { name: "Save or cancel" })
    .closest("form");
  if (!form) throw new Error("The page is a form");
  return form;
};
/** The open dialog (a route's check, a delete), else the route's page, where the route's form now is. */
const dialog = () => screen.queryByRole("dialog") ?? page();
/** The choices a standard picker offers (WEB-U-134), read from its open menu, which then closes. */
const options = (label: string) => {
  fireEvent.click(picker(label, dialog()));
  const words = screen
    .getAllByRole("menuitemradio")
    .map((option) => option.textContent);
  fireEvent.keyDown(screen.getByRole("menu"), { key: "Escape" });
  return words;
};
const choose = (label: string, option: string) =>
  choosePicked(label, option, dialog());
async function edit(name: string) {
  await screen.findByRole("table", { name: "Model routes" });
  fireEvent.click(
    await screen.findByRole("link", { name: `Edit route ${name}` }),
  );
  await screen.findByRole("group", { name: "Save or cancel" });
}

// The per-route tool-schema settings in the route form's Advanced group (WEB-U-057).
/** Chooses one of a route's actions from its menu (one actions column: Edit, and the rest under "…"). */
function rowAction(route: string, action: string) {
  fireEvent.click(
    screen.getByRole("button", { name: `More actions for route ${route}` }),
  );
  fireEvent.click(screen.getByRole("menuitem", { name: action }));
}

describe("WEB-U-057 tool calls and instructions in the route form", () => {
  it("round-trips the three fields, leaving the protocol's default role out", () => {
    const fields = {
      ...route("local"),
      strict: "off" as const,
      schema_keywords: "basic" as const,
      instructions_role: "developer" as const,
    };
    const draft = toDraft("local", fields);
    expect(draft).toMatchObject({
      strict: "off",
      schema_keywords: "basic",
      instructions_role: "developer",
    });
    expect(fromDraft(draft).fields).toMatchObject({
      strict: "off",
      schema_keywords: "basic",
      instructions_role: "developer",
    });
    expect(
      fromDraft(toDraft("local", route("local"))).fields,
    ).not.toHaveProperty("instructions_role");
    expect(
      gatewayProblems([
        { field: "strict", reason: "strict_invalid" },
        { field: "schema_keywords", reason: "schema_keywords_invalid" },
        { field: "instructions_role", reason: "instructions_role_invalid" },
      ]),
    ).toEqual({
      strict: "models.choiceInvalid",
      schema_keywords: "models.choiceInvalid",
      instructions_role: "models.choiceInvalid",
    });
  });

  it("keeps a role the new protocol allows and resets one it does not", () => {
    const chat = toDraft("local", route("local"));
    expect(
      withProtocol(
        { ...chat, instructions_role: "developer" },
        "openai_responses",
      ),
    ).toEqual({
      draft: {
        ...chat,
        protocol: "openai_responses",
        instructions_role: "developer",
      },
      reset: false,
    });
    expect(
      withProtocol(
        { ...chat, instructions_role: "system" },
        "openai_responses",
      ),
    ).toEqual({
      // Back to the protocol's default: the field is left out.
      draft: { ...chat, protocol: "openai_responses" },
      reset: true,
    });
    expect(withProtocol(chat, "anthropic_messages").reset).toBe(false);
  });

  it("keeps them in a collapsed Advanced group, offering each protocol's roles", async () => {
    const fake = fakeRoutes({ routes: [route("local")] });
    show(fake.api);
    await edit("Local model");
    const advanced = within(dialog()).getByText("Advanced").closest("details");
    expect(advanced).not.toHaveAttribute("open");
    expect(
      within(advanced as HTMLElement).getByText("Tool calls and instructions"),
    ).toBeInTheDocument();
    expect(options("Strict tool schemas")).toEqual([
      "Only schemas that qualify",
      "Every tool",
      "Never",
    ]);
    expect(options("Schema keywords")).toEqual(["All", "Basic only"]);
    expect(options("Instructions go as")).toEqual([
      "The protocol's default",
      "A system message",
      "A developer message",
    ]);
    choose("Instructions go as", "A system message");
    choose("Protocol", "Responses");
    expect(options("Instructions go as")).toEqual([
      "The protocol's default",
      "The instructions field",
      "A developer message",
    ]);
    expect(chosen("Instructions go as", dialog())).toBe(
      "The protocol's default",
    );
    expect(
      within(dialog()).getByText(
        "“Instructions go as” went back to the protocol's default: this protocol can't send them that way.",
      ),
    ).toBeVisible();
    choose("Protocol", "Messages");
    expect(picker("Instructions go as", dialog())).toHaveAttribute(
      "aria-disabled",
      "true",
    );
    expect(
      within(dialog()).getByText(
        "Messages routes always send them as the instructions field.",
      ),
    ).toBeInTheDocument();
  });

  it("saves what was chosen", async () => {
    const fake = fakeRoutes({ routes: [route("local")] });
    show(fake.api);
    await edit("Local model");
    choose("Strict tool schemas", "Never");
    choose("Schema keywords", "Basic only");
    choose("Instructions go as", "A developer message");
    // Display name is required before a save (F4).
    fireEvent.change(within(dialog()).getByLabelText("Display name"), {
      target: { value: "Local" },
    });
    fireEvent.click(within(dialog()).getByRole("button", { name: "Save" }));
    expect(await screen.findByText("Saved route Local.")).toBeVisible();
    expect(fake.writes.at(-1)?.fields).toMatchObject({
      strict: "off",
      schema_keywords: "basic",
      instructions_role: "developer",
    });
  });
});

const refused = (suggestion: {
  field: string;
  value: string;
  reason: string;
}) =>
  probeResult("failed", {
    ok: false,
    tool_call: "failed",
    failure: {
      code: "upstream_error",
      status: 400,
      excerpt: "fixture excerpt",
      suggestion: suggestion as NonNullable<
        NonNullable<
          ReturnType<typeof probeResult>["observed"]["failure"]
        >["suggestion"]
      >,
    },
  });

// A failed check whose upstream answered 400 may point at a route setting; a hint only (WEB-U-058).
describe("WEB-U-058 the route check's suggestion", () => {
  it("says it in words and opens the route with the setting changed, unsaved, Advanced open", async () => {
    const fake = fakeRoutes({
      routes: [route("local")],
      probes: {
        local: refused({
          field: "schema_keywords",
          value: "basic",
          reason: "the endpoint refused a JSON Schema keyword",
        }),
      },
    });
    show(fake.api);
    await screen.findByRole("table", { name: "Model routes" });
    rowAction("Local model", "Check route Local model");
    fireEvent.click(
      within(dialog()).getByRole("button", { name: "Run check" }),
    );
    expect(
      await within(dialog()).findByText(
        "The provider refused a JSON Schema keyword. Try Schema keywords: Basic only.",
      ),
    ).toBeVisible();
    expect(within(dialog()).queryByText(/the endpoint refused/)).toBeNull();
    fireEvent.click(
      within(dialog()).getByRole("button", {
        name: "Edit the route with this setting",
      }),
    );
    // The route's own page opens, with the setting applied (WEB-U-115).
    await screen.findByRole("heading", { name: "Edit route Local model" });
    const form = page();
    expect(
      within(form).getByText("Advanced").closest("details"),
    ).toHaveAttribute("open");
    const keywords = picker("Schema keywords", form);
    expect(chosen("Schema keywords", form)).toBe("Basic only");
    await waitFor(() => expect(keywords).toHaveFocus());
    expect(
      within(form).getByText(
        "Changed Schema keywords to Basic only. Save the route, then check it again.",
      ),
    ).toBeVisible();
    // A hint only: nothing was written until the person saves.
    expect(fake.writes.filter((write) => write.fields)).toEqual([]);
    fireEvent.change(within(form).getByLabelText("Display name"), {
      target: { value: "Local" },
    });
    fireEvent.click(within(form).getByRole("button", { name: "Save" }));
    expect(await screen.findByText("Saved route Local.")).toBeVisible();
    expect(fake.writes.at(-1)?.fields?.schema_keywords).toBe("basic");
  });

  it("offers strict off in words too, and only a plain line for a suggestion this web does not know", async () => {
    const fake = fakeRoutes({
      routes: [route("local"), route("other")],
      probes: {
        local: refused({ field: "strict", value: "off", reason: "x" }),
        other: refused({
          field: "instructions_role",
          value: "developer",
          reason: "x",
        }),
      },
    });
    show(fake.api);
    await screen.findByRole("table", { name: "Model routes" });
    rowAction("Local model", "Check route Local model");
    fireEvent.click(
      within(dialog()).getByRole("button", { name: "Run check" }),
    );
    expect(
      await within(dialog()).findByText(
        "The provider refused a tool schema in strict mode. Try Strict tool schemas: Never.",
      ),
    ).toBeVisible();
    fireEvent.click(within(dialog()).getByRole("button", { name: "Done" }));
    rowAction("Other model", "Check route Other model");
    fireEvent.click(
      within(dialog()).getByRole("button", { name: "Run check" }),
    );
    expect(
      await within(dialog()).findByText(
        "The provider refused this route's tool settings.",
      ),
    ).toBeVisible();
    expect(
      within(dialog()).queryByRole("button", {
        name: "Edit the route with this setting",
      }),
    ).toBeNull();
  });
});

// WEB-U-133 (F16): the one click for a model that thinks unasked fills Max output in, unsaved, like any suggestion.
describe("WEB-U-133 room for a model that thinks", () => {
  it("opens the route with Max output 16384 filled in and focused, unsaved", async () => {
    const fake = fakeRoutes({
      routes: [route("local", { max_output: 2048 })],
      probes: {
        local: probeResult("verified", { warnings: ["thinks_unasked"] }),
      },
    });
    show(fake.api);
    await screen.findByRole("table", { name: "Model routes" });
    rowAction("Local model", "Check route Local model");
    fireEvent.click(
      within(dialog()).getByRole("button", { name: "Run check" }),
    );
    fireEvent.click(
      await within(dialog()).findByRole("button", {
        name: /^Set Max output to 16\s384$/,
      }),
    );
    await screen.findByRole("heading", { name: "Edit route Local model" });
    const form = page();
    const output = within(form).getByLabelText("Max output");
    expect(output).toHaveValue("16384");
    await waitFor(() => expect(output).toHaveFocus());
    expect(
      within(form).getByText(
        "Changed Max output to 16 384, so the model has room to think before it answers. Save the route, then check it again.",
      ),
    ).toBeVisible();
    expect(fake.writes.filter((write) => write.fields)).toEqual([]);
  });
});

// WEB-U-269: the one click for a local model's context fills Context window in with the size its server gives, focused
// and unsaved, like any suggestion.
describe("WEB-U-269 Context window from the server's size", () => {
  it("opens the route with Context window filled in and focused, unsaved", async () => {
    const fake = fakeRoutes({
      routes: [route("local")],
      probes: {
        // Enough for tasks, but not what the route says: the one click (below the minimum it isn't offered, WEB-U-290).
        local: probeResult("verified", {
          warnings: ["context_window_differs"],
          server_context: {
            server: "ollama",
            context_length: 32768,
            minimum: 16384,
          },
        }),
      },
    });
    show(fake.api);
    await screen.findByRole("table", { name: "Model routes" });
    rowAction("Local model", "Check route Local model");
    fireEvent.click(
      within(dialog()).getByRole("button", { name: "Run check" }),
    );
    fireEvent.click(
      await within(dialog()).findByRole("button", {
        name: "Set Context window to 32\u202f768",
      }),
    );
    await screen.findByRole("heading", { name: "Edit route Local model" });
    const form = page();
    const context = within(form).getByLabelText("Context window");
    expect(context).toHaveValue("32768");
    await waitFor(() => expect(context).toHaveFocus());
    expect(
      within(form).getByText(
        "Changed Context window to 32 768, the size the model server gives this model. Save the route, then check it again.",
      ),
    ).toBeVisible();
    expect(fake.writes.filter((write) => write.fields)).toEqual([]);
  });
});

// WEB-U-281 (the Models page review's P1): from the gateway's container, localhost is the container itself, so a model
// server on this computer at http://localhost:… is never reached. The failed check says so and offers the container
// host's name in one click; the route's page opens with Address changed, focused and unsaved.
describe("WEB-U-281 a route at localhost the check can't reach", () => {
  // The gateway says whether host.docker.internal resolves from its container (GW-U-154); the one click needs it to.
  const unreachable = probeResult("failed", {
    ok: false,
    failure: {
      code: "unreachable",
      status: null,
      excerpt: null,
      container_host: true,
    },
  });

  it("says localhost is the gateway's container and opens the route with host.docker.internal", async () => {
    const fake = fakeRoutes({
      routes: [route("local", { base_url: "http://localhost:11434/v1" })],
      probes: { local: unreachable },
    });
    show(fake.api);
    await screen.findByRole("table", { name: "Model routes" });
    rowAction("Local model", "Check route Local model");
    fireEvent.click(
      within(dialog()).getByRole("button", { name: "Run check" }),
    );
    const result = await within(dialog()).findByRole("region", {
      name: "Check result",
    });
    expect(result).toHaveTextContent(
      "The check failed. The server could not be reached.",
    );
    expect(
      within(result).getByText(
        "Here localhost is the gateway's own container, not this computer. The gateway reaches this computer at host.docker.internal: http://host.docker.internal:11434/v1.",
      ),
    ).toBeVisible();
    fireEvent.click(
      within(result).getByRole("button", { name: "Use host.docker.internal" }),
    );
    await screen.findByRole("heading", { name: "Edit route Local model" });
    const form = page();
    const address = within(form).getByLabelText("Address");
    expect(address).toHaveValue("http://host.docker.internal:11434/v1");
    await waitFor(() => expect(address).toHaveFocus());
    expect(
      within(form).getByText(
        "Changed the address to http://host.docker.internal:11434/v1. Save the route, then check it again.",
      ),
    ).toHaveRole("status");
    // The change is beside Address; Advanced stays closed.
    expect(
      within(form).getByText("Advanced").closest("details"),
    ).not.toHaveAttribute("open");
    expect(fake.writes.filter((write) => write.fields)).toEqual([]);
    fireEvent.click(within(form).getByRole("button", { name: "Save" }));
    expect(await screen.findByText("Saved route Local model.")).toBeVisible();
    expect(
      fake.writes
        .filter((write) => write.fields)
        .map((write) => write.fields?.base_url),
    ).toEqual(["http://host.docker.internal:11434/v1"]);
  });

  it.each([
    ["a hosted address", "https://models.example.invalid/v1"],
    [
      "one already at the container host",
      "http://host.docker.internal:11434/v1",
    ],
    ["one on the network", "http://ollama:11434/v1"],
  ])("offers nothing for %s", async (_, base_url) => {
    const fake = fakeRoutes({
      routes: [route("local", { base_url })],
      probes: { local: unreachable },
    });
    show(fake.api);
    await screen.findByRole("table", { name: "Model routes" });
    rowAction("Local model", "Check route Local model");
    fireEvent.click(
      within(dialog()).getByRole("button", { name: "Run check" }),
    );
    const result = await within(dialog()).findByRole("region", {
      name: "Check result",
    });
    expect(within(result).queryByText(/gateway's own container/)).toBeNull();
    expect(
      within(result).queryByRole("button", {
        name: "Use host.docker.internal",
      }),
    ).toBeNull();
  });
  // WEB-U-287: the one click only where the gateway reaches host.docker.internal; otherwise words, and no button.
  it.each([
    [
      false,
      "Here localhost is the gateway's own container, not this computer, and host.docker.internal doesn't resolve on this installation. Use this computer's network address instead, with the same port.",
    ],
    [
      undefined,
      "Here localhost is the gateway's own container, not this computer. With Docker Desktop (Mac or Windows), a model server on this computer is at host.docker.internal: http://host.docker.internal:11434/v1. On Linux, use the computer's network address instead.",
    ],
  ])(
    "WEB-U-287: with container_host %s, says so in words and offers no click",
    async (reaches, words) => {
      const fake = fakeRoutes({
        routes: [route("local", { base_url: "http://localhost:11434/v1" })],
        probes: {
          local: probeResult("failed", {
            ok: false,
            failure: {
              code: "unreachable",
              status: null,
              excerpt: null,
              ...(reaches === undefined ? {} : { container_host: reaches }),
            },
          }),
        },
      });
      show(fake.api);
      await screen.findByRole("table", { name: "Model routes" });
      rowAction("Local model", "Check route Local model");
      fireEvent.click(
        within(dialog()).getByRole("button", { name: "Run check" }),
      );
      const result = await within(dialog()).findByRole("region", {
        name: "Check result",
      });
      expect(within(result).getByText(words)).toBeVisible();
      expect(
        within(result).queryByRole("button", {
          name: "Use host.docker.internal",
        }),
      ).toBeNull();
    },
  );
});

// WEB-U-287 (the Models page review, #3): a 404 or 405 at the protocol's own request is endpoint_missing (GW-U-154);
// the result names the route's protocol and the two others to try.
describe("WEB-U-287 a server without the route's protocol endpoint", () => {
  it("names the protocol and the others to try", async () => {
    const fake = fakeRoutes({
      routes: [route("local")],
      probes: {
        local: probeResult("failed", {
          ok: false,
          failure: { code: "endpoint_missing", status: 404, excerpt: null },
        }),
      },
    });
    show(fake.api);
    await screen.findByRole("table", { name: "Model routes" });
    rowAction("Local model", "Check route Local model");
    fireEvent.click(
      within(dialog()).getByRole("button", { name: "Run check" }),
    );
    const result = await within(dialog()).findByRole("region", {
      name: "Check result",
    });
    expect(result).toHaveTextContent(
      "The check failed. The server has no endpoint for this route's protocol at this address.",
    );
    expect(
      within(result).getByText(
        "This route speaks Chat Completions. Check the address (it often ends in /v1), or edit the route and try Responses or Messages under Advanced › Connection.",
      ),
    ).toBeVisible();
  });
});

// WEB-U-289 (the Models page review, P2 #8 and #9): the usage setting is named for what it does, with a hint; a check
// that finds the server refusing it opens the route with the box unticked, focused and its note beside it.
describe("WEB-U-289 asking the server for token usage", () => {
  it("opens the route with the box off, focused, and its note beside it", async () => {
    const fake = fakeRoutes({
      routes: [route("local")],
      probes: {
        local: probeResult("failed", {
          ok: false,
          failure: {
            code: "usage_refused",
            status: 400,
            excerpt: null,
            suggestion: {
              field: "stream_usage",
              value: "false",
              reason: "the server refuses stream_options",
            },
          },
        }),
      },
    });
    show(fake.api);
    await screen.findByRole("table", { name: "Model routes" });
    rowAction("Local model", "Check route Local model");
    fireEvent.click(
      within(dialog()).getByRole("button", { name: "Run check" }),
    );
    expect(
      await within(dialog()).findByText(
        "The rest of the check worked without usage reporting. Turn off “Ask the server for token usage” for this route.",
      ),
    ).toBeVisible();
    fireEvent.click(
      within(dialog()).getByRole("button", {
        name: "Edit the route with this setting",
      }),
    );
    await screen.findByRole("heading", { name: "Edit route Local model" });
    const form = page();
    const box = within(form).getByRole("checkbox", {
      name: "Ask the server for token usage",
    });
    expect(box).not.toBeChecked();
    await waitFor(() => expect(box).toHaveFocus());
    expect(box).toHaveAccessibleDescription(
      "Counts each task's tokens toward budgets and the usage pages. Turn it off only for a server that refuses it.",
    );
    // The note sits in Capabilities with the box, not under Tool calls and instructions.
    const note = within(form).getByText(
      "Turned off “Ask the server for token usage”. Save the route, then check it again.",
    );
    expect(note.closest("section")).toBe(box.closest("section"));
    expect(fake.writes.filter((write) => write.fields)).toEqual([]);
  });
});

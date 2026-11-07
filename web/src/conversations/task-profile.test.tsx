import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { Conversation } from "../protocol/generated/api";
import { conversationId } from "../test/frames";
import { openTaskChoices } from "../test/start-choices";
import { templates } from "../test/templates";
import { shellData } from "../test/workspaces";
import { emptyTranscript } from "../transcript/reducer";

import { ConversationControls } from "./ConversationControls";
import { TaskChoices } from "./TaskChoices";

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
const base: Conversation = {
  id: conversationId,
  owner_id: conversationId,
  title: "Crash report",
  model_route: "cloud",
  template: "minimal",
  pinned: false,
  title_source: "message",
  backend: "docker",
  state: "active",
  created_at: "2026-01-01T00:00:00Z",
  updated_at: "2026-01-01T00:00:00Z",
  last_activity_at: "2026-01-01T00:00:00Z",
  workspace_id: "0195d8c0-0000-7000-8000-0000000000ff",
  mcp_servers: [],
  connectors: [],
  team: null,
};
const fetch = vi.fn<typeof globalThis.fetch>();
beforeEach(() => {
  fetch.mockReset();
  vi.stubGlobal("fetch", fetch);
});
afterEach(() => vi.unstubAllGlobals());

/** Opens the task's model and environment panel from its header, then finds a picker in it by name. */
async function inHeader(name: string) {
  await openTaskChoices();
  return screen.findByRole("button", { name });
}

function page(conversation: Conversation) {
  const cache = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  cache.setQueryData(["templates"], templates);
  cache.setQueryData(["models"], models);
  shellData(cache);
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
}
const refusal = (field: string) =>
  Promise.resolve(
    Response.json(
      {
        error: {
          code: "set_by_profile",
          message: "Private diagnostic",
          retryable: false,
          details: [{ field, reason: "set_by_profile" }],
        },
      },
      { status: 422 },
    ),
  );

// ADR-0012: a task keeps the profile it was made from, which fixes the fields it sets (WEB-U-051).
describe("WEB-U-051 a task made from a profile", () => {
  it("says which profile it came from, and fixes the model and environment the profile sets", async () => {
    page({
      ...base,
      profile: {
        id: "0195d8c0-0000-7000-8000-0000000000aa",
        version: 3,
        // The slug and the name people read differ: the task shows the display name.
        name: "team-default",
        display_name: "Team default",
        sets: ["model_route", "template", "mcp_servers", "connectors"],
      },
    });
    expect(screen.getByText("Agent: Team default")).toBeVisible();
    await openTaskChoices();
    for (const name of ["Model: Cloud model", "Environment: Minimal"]) {
      const picker = screen.getByRole("button", { name });
      expect(picker).toHaveAttribute("aria-disabled", "true");
      expect(picker).toHaveAccessibleDescription(
        "Set by the agent profile Team default",
      );
    }
  });

  it("leaves free what the profile does not set, and a task without a profile says nothing", async () => {
    page({
      ...base,
      profile: {
        id: "0195d8c0-0000-7000-8000-0000000000aa",
        version: 3,
        name: "Notes only",
        display_name: "Notes only",
        sets: ["mcp_servers", "connectors"],
      },
    });
    expect(await inHeader("Model: Cloud model")).not.toHaveAttribute(
      "aria-disabled",
      "true",
    );
    expect(await inHeader("Environment: Minimal")).not.toHaveAttribute(
      "aria-disabled",
      "true",
    );
  });

  it("reads 422 set_by_profile in words when a stale page tries to change a fixed field", async () => {
    fetch.mockImplementationOnce(() => refusal("route"));
    page({ ...base });
    expect(screen.queryByText(/^From /)).toBeNull();
    fireEvent.click(await inHeader("Model: Cloud model"));
    fireEvent.click(
      await screen.findByRole("menuitemradio", { name: /Local model/ }),
    );
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "This task's agent profile sets its model.",
    );
    expect(screen.queryByText(/Private diagnostic/)).toBeNull();
  });

  it("reads 422 set_by_profile on an environment switch", async () => {
    fetch.mockImplementationOnce(() => refusal("template"));
    page({ ...base });
    fireEvent.click(await inHeader("Environment: Minimal"));
    fireEvent.click(
      screen.getByRole("menuitemradio", { name: /^Workstation/ }),
    );
    fireEvent.click(
      await screen.findByRole("button", { name: "Switch to Workstation" }),
    );
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "This task's agent profile sets its environment.",
    );
  });
});

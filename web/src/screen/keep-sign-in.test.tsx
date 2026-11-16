import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { english } from "../i18n/translate";
import type { Conversation } from "../protocol/generated/api";
import {
  clients,
  control,
  FakeScreenClient,
  FakeSocket,
  socket,
} from "../test/screen-socket";

import { screenNoticeWords } from "./notices";

vi.mock("@novnc/novnc", () => ({ default: FakeScreenClient }));
vi.mock("../api/stream", () => ({
  openScreenSocket: (path: string) => new FakeSocket(path),
}));
beforeEach(() => {
  FakeSocket.opened = [];
  clients.length = 0;
});

// WEB-U-475 (ADR-0039 §6, amended for 18.0; PRD item 24): the task's "keep the sign-in" switch is a check item in
// the Screen panel's ⋯, offered whether or not the screen is live; a press sends the task's PATCH once, the answer
// replaces the task's record, and a failure says so under the row and leaves the item as it was. A task without a
// desktop offers no switch, and the two notices have their words.
describe("WEB-U-475 keep the sign-in for this task", () => {
  const task: Conversation = {
    id: "task-1",
    owner_id: "owner",
    title: "Task",
    model_route: "local",
    backend: "docker",
    state: "active",
    created_at: "2026-11-15T10:00:00Z",
    updated_at: "2026-11-15T10:00:00Z",
    last_activity_at: "2026-11-15T10:00:00Z",
    template: "desktop",
    pinned: false,
    keep_sign_in: false,
    workspace_id: "workspace",
    mcp_servers: [],
    connectors: [],
    title_source: "message",
    team: null,
  };

  async function shown(on: boolean, onChange = vi.fn(), failed = false) {
    const { ScreenPanel } = await import("./ScreenPanel");
    render(
      <ScreenPanel
        conversationId="task-1"
        working={false}
        keepSignIn={{ on, pending: false, failed, onChange }}
      />,
    );
    fireEvent.click(
      screen.getByRole("button", { name: "More actions for the screen" }),
    );
    return screen.getByRole("menuitemcheckbox", {
      name: /Keep the sign-in for this task/,
    });
  }

  it("is a check item in the ⋯, off or on from the task, before the screen is live", async () => {
    const onChange = vi.fn();
    const item = await shown(false, onChange);
    expect(item).toHaveAttribute("aria-checked", "false");
    expect(item).toHaveTextContent(
      "The browser stays signed in between the agent's runs, until you turn this off or clear its sign-ins.",
    );
    // Not live: the screen's own actions aren't there, the task's setting is.
    expect(
      screen.queryByRole("menuitem", { name: "Clear the browser's sign-ins" }),
    ).toBeNull();
    fireEvent.click(item);
    expect(onChange).toHaveBeenCalledExactlyOnceWith(true);
  });

  it("shows the switch on, and a press turns it off", async () => {
    const onChange = vi.fn();
    const item = await shown(true, onChange);
    expect(item).toHaveAttribute("aria-checked", "true");
    fireEvent.click(item);
    expect(onChange).toHaveBeenCalledExactlyOnceWith(false);
  });

  it("says when the sign-in ends while the switch is on, with the installation's hours when known", async () => {
    const { ScreenPanel } = await import("./ScreenPanel");
    const { rerender } = render(
      <ScreenPanel
        conversationId="task-1"
        working={false}
        keepSignIn={{
          on: true,
          pending: false,
          failed: false,
          lifetimeSeconds: 8 * 3600,
          onChange: vi.fn(),
        }}
      />,
    );
    expect(
      screen.getByText(/installation's limit of 8 hours\.$/),
    ).toBeVisible();
    rerender(
      <ScreenPanel
        conversationId="task-1"
        working={false}
        keepSignIn={{
          on: true,
          pending: false,
          failed: false,
          onChange: vi.fn(),
        }}
      />,
    );
    expect(screen.getByText(/installation's time limit\.$/)).toBeVisible();
  });

  // The 18.0 candidate's screens (WEB-E2E-125, 16 Nov 2026): in control with the switch on, the panel said both that
  // the sign-in is kept until its ends and that it lasts for the next run only. Only the switch's own line is true.
  it("in control, says a sign-in lasts for the next run only while the switch is off, and never while it is on", async () => {
    const { ScreenPanel } = await import("./ScreenPanel");
    const nextRunOnly =
      "A sign-in here lasts for the agent's next run only. Each run after that starts signed out.";
    const keep = (on: boolean) => ({
      on,
      pending: false,
      failed: false,
      lifetimeSeconds: 8 * 3600,
      onChange: vi.fn(),
    });
    const { rerender } = render(
      <ScreenPanel
        conversationId="task-1"
        working={false}
        keepSignIn={keep(true)}
      />,
    );
    await act(async () => {
      socket().open();
      await Promise.resolve();
    });
    await vi.waitFor(() => expect(clients).toHaveLength(1));
    fireEvent.click(screen.getByRole("button", { name: "Take over" }));
    act(() => socket().frame(control("you")));
    expect(screen.getByRole("status")).toHaveTextContent(/^You have control\./);
    expect(
      screen.getByText(/installation's limit of 8 hours\.$/),
    ).toBeVisible();
    expect(screen.queryByText(nextRunOnly)).toBeNull();
    rerender(
      <ScreenPanel
        conversationId="task-1"
        working={false}
        keepSignIn={keep(false)}
      />,
    );
    expect(screen.getByText(nextRunOnly)).toBeVisible();
    expect(screen.queryByText(/installation's limit of 8 hours\.$/)).toBeNull();
  });

  it("says when a change didn't take, and leaves the item as it was", async () => {
    const item = await shown(false, vi.fn(), true);
    expect(item).toHaveAttribute("aria-checked", "false");
    expect(
      screen.getByText("The setting didn't change. Try again."),
    ).toHaveAttribute("role", "status");
  });

  it("sends the task's PATCH once and replaces the task's record with the answer", async () => {
    const { api } = await import("../api/client");
    vi.restoreAllMocks();
    const execute = vi.fn().mockResolvedValue({ ...task, keep_sign_in: true });
    const patch = vi
      .spyOn(api, "updateConversation")
      .mockReturnValue({ execute } as unknown as ReturnType<
        typeof api.updateConversation
      >);
    const { useKeepSignIn } = await import("./use-keep-sign-in");
    const client = new QueryClient();
    client.setQueryData(["conversation", task.id], task);
    function Switch() {
      const current = client.getQueryData<Conversation>([
        "conversation",
        task.id,
      ]);
      const keep = useKeepSignIn(current ?? task);
      return (
        <button onClick={() => keep.onChange(!keep.on)}>
          {keep.on ? "on" : "off"}
        </button>
      );
    }
    render(
      <QueryClientProvider client={client}>
        <Switch />
      </QueryClientProvider>,
    );
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "off" }));
      await Promise.resolve();
    });
    expect(patch).toHaveBeenCalledExactlyOnceWith("task-1", {
      keep_sign_in: true,
    });
    expect(execute).toHaveBeenCalledOnce();
    expect(
      client.getQueryData<Conversation>(["conversation", task.id])
        ?.keep_sign_in,
    ).toBe(true);
  });

  it("words the two notices", () => {
    expect(
      screenNoticeWords(
        { kind: "screen.sign_in_kept", at: "2026-11-15T10:00:00Z" },
        english,
      ),
    ).toBe("The desktop browser's sign-in is kept for this task.");
    expect(
      screenNoticeWords(
        { kind: "screen.sign_in_released", at: "2026-11-15T10:00:00Z" },
        english,
      ),
    ).toBe(
      "The desktop browser's sign-in is no longer kept: it ends with the next run.",
    );
  });
});

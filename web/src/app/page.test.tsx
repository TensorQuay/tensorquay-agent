import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import type { ComponentProps } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { StartProvider } from "../conversations/StartProvider";
import { Navigation } from "../shell/Navigation";
import { conversationId } from "../test/frames";
import { openChoices } from "../test/start-choices";
import { templates } from "../test/templates";
import { LIMITS } from "../test/user";

import Home from "./(app)/page";

// §3 step 1b: New task's files upload once the task exists; the upload itself is the browser's (XMLHttpRequest).
const upload = vi.hoisted(() =>
  vi.fn((_id: string, file: File) =>
    Promise.resolve({
      name: file.name,
      path: `uploads/${file.name}`,
      size: file.size,
    }),
  ),
);
vi.mock("../api/upload", () => ({ uploadFile: upload }));

const push = vi.fn();
vi.mock("next/navigation", () => ({
  useRouter: () => ({ push }),
  usePathname: () => "/",
}));
vi.mock("next/link", () => ({
  default: ({
    onNavigate,
    href,
    children,
    ...props
  }: ComponentProps<"a"> & { onNavigate?: () => void }) => (
    <a
      {...props}
      href={href}
      onClick={(event) => {
        event.preventDefault();
        onNavigate?.();
      }}
    >
      {children}
    </a>
  ),
}));
const record = {
  id: conversationId,
  owner_id: conversationId,
  title: "Create a greeting",
  model_route: "second",
  template: "workstation",
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
const models = ["local", "second"].map((name) => ({
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
}));
const fetch = vi.fn<typeof globalThis.fetch>();
let created = false;
beforeEach(() => {
  created = false;
  push.mockReset();
  fetch
    .mockReset()
    .mockImplementation((url, init) => Promise.resolve(respond(url, init)));
  vi.stubGlobal("fetch", fetch);
});
function respond(url: RequestInfo | URL, init?: RequestInit): Response {
  const path = requestPath(url);
  if (path.endsWith("/auth/me"))
    return Response.json({
      id: conversationId,
      email: "member@example.invalid",
      role: "member",
      internet: "none",
      limits: LIMITS,
      features: { agent_teams: false },
    });
  if (path.endsWith("/models")) return Response.json({ items: models });
  if (path.endsWith("/templates")) return Response.json(templates);
  if (path.endsWith("/messages"))
    return Response.json(
      {
        accepted: true,
        conversation_id: conversationId,
        message_id: "1",
        held: false,
      },
      { status: 202 },
    );
  if (init?.method === "POST") {
    created = true;
    return Response.json(record, { status: 201 });
  }
  return Response.json({ items: created ? [record] : [], next_cursor: null });
}
function requestPath(url: RequestInfo | URL): string {
  return typeof url === "string"
    ? url
    : url instanceof URL
      ? url.href
      : url.url;
}
function requestBody(value: BodyInit | null | undefined): unknown {
  if (typeof value !== "string") throw new Error("Expected a JSON body");
  return JSON.parse(value) as unknown;
}
afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});
const posts = () =>
  fetch.mock.calls.filter(([, init]) => init?.method === "POST");
function page() {
  const cache = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  const view = render(
    <QueryClientProvider client={cache}>
      <StartProvider>
        <Home />
        <Navigation />
      </StartProvider>
    </QueryClientProvider>,
  );
  return { ...view, cache };
}
async function send() {
  await openChoices();
  await screen.findByRole("button", { name: "Model: Local model" });
  fireEvent.click(screen.getByRole("button", { name: /^Model:/ }));
  fireEvent.click(screen.getByRole("menuitemradio", { name: /Second/ }));
  fireEvent.change(screen.getByRole("textbox", { name: "Message" }), {
    target: { value: "Create a greeting\nCheck it works" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Send" }));
}

describe("WEB-U-017 composer-first start", () => {
  function holdModels() {
    let release: (response: Response) => void = () => undefined;
    fetch.mockImplementation((url, init) =>
      requestPath(url).endsWith("/models")
        ? new Promise<Response>((resolve) => {
            release = resolve;
          })
        : Promise.resolve(respond(url, init)),
    );
    return (response: Response) => release(response);
  }
  async function sendBeforeCatalogue() {
    page();
    await openChoices();
    await screen.findByRole("button", { name: "Environment: Workstation" });
    const input = screen.getByRole("textbox", { name: "Message" });
    fireEvent.change(input, { target: { value: "Create a greeting" } });
    fireEvent.keyDown(input, { key: "Enter" });
    const pending = await screen.findByRole("article", {
      name: "Your message",
    });
    expect(pending).toHaveTextContent("Create a greeting");
    expect(pending).toHaveTextContent("Sending…");
    expect(input).toHaveValue("");
    expect(posts()).toHaveLength(0);
  }
  it("attaches a file on New task, uploads it once the task exists, and names it in the first message", async () => {
    upload.mockClear();
    page();
    await openChoices();
    const file = new File(["id,total\n1,2\n"], "sales.csv", {
      type: "text/csv",
    });
    fireEvent.change(screen.getByLabelText("Choose files to attach"), {
      target: { files: [file] },
    });
    expect(
      within(screen.getByRole("list", { name: "Attached files" })).getByText(
        "sales.csv",
      ),
    ).toBeVisible();
    expect(upload).not.toHaveBeenCalled();
    fireEvent.change(screen.getByRole("textbox", { name: "Message" }), {
      target: { value: "Summarise the sales" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Send" }));
    await waitFor(() => expect(posts()).toHaveLength(2));
    expect(upload).toHaveBeenCalledWith(
      conversationId,
      file,
      expect.anything(),
    );
    expect(requestBody(posts()[1]?.[1]?.body)).toEqual({
      text: "Summarise the sales",
      attachments: ["uploads/sales.csv"],
    });
  });

  it("says a held file could not be uploaded, keeps the prompt, and sends it with the file on retry", async () => {
    upload.mockClear();
    upload.mockRejectedValueOnce(new Error("lost"));
    page();
    await openChoices();
    const file = new File(["x"], "notes.txt");
    fireEvent.change(screen.getByLabelText("Choose files to attach"), {
      target: { files: [file] },
    });
    fireEvent.change(screen.getByRole("textbox", { name: "Message" }), {
      target: { value: "Read my notes" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Send" }));
    expect(
      await screen.findByText(
        /^Not sent\. An attached file could not be uploaded\. Try again\.$/,
      ),
    ).toBeVisible();
    expect(posts()).toHaveLength(1);
    fireEvent.click(screen.getByRole("button", { name: "Retry message" }));
    await waitFor(() => expect(posts()).toHaveLength(2));
    expect(requestBody(posts()[1]?.[1]?.body)).toEqual({
      text: "Read my notes",
      attachments: ["uploads/notes.txt"],
    });
  });

  it("WEB-U-017 holds a prompt sent before the catalogues arrive and sends it when they do", async () => {
    const release = holdModels();
    await sendBeforeCatalogue();
    act(() => {
      release(Response.json({ items: models }));
    });
    await waitFor(() => expect(posts()).toHaveLength(2));
    expect(requestBody(posts()[0]?.[1]?.body)).toEqual({
      title: "Create a greeting",
      model_route: "local",
      template: "workstation",
    });
    expect(requestBody(posts()[1]?.[1]?.body)).toEqual({
      text: "Create a greeting",
    });
  });
  it("WEB-U-017 restores a held prompt when a catalogue cannot load", async () => {
    const release = holdModels();
    await sendBeforeCatalogue();
    act(() => {
      release(
        Response.json(
          {
            error: {
              code: "store_unavailable",
              message: "Private diagnostic",
              retryable: true,
            },
          },
          { status: 503 },
        ),
      );
    });
    expect(await screen.findByRole("alert")).toBeVisible();
    expect(screen.getByRole("button", { name: "Try again" })).toBeEnabled();
    expect(screen.getByRole("textbox", { name: "Message" })).toHaveValue(
      "Create a greeting",
    );
    expect(screen.queryByRole("article", { name: "Your message" })).toBeNull();
    expect(posts()).toHaveLength(0);
  });
  it("WEB-U-018 makes an unavailable catalogue visible and prevents an unconfigured send", async () => {
    fetch.mockImplementation((url, init) =>
      Promise.resolve(
        requestPath(url).endsWith("/templates")
          ? Response.json(
              {
                error: {
                  code: "not_implemented",
                  message: "Private diagnostic",
                  retryable: false,
                },
              },
              { status: 501 },
            )
          : respond(url, init),
      ),
    );
    page();
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Environments could not be loaded. Try again.",
    );
    expect(screen.queryByText("Private diagnostic")).toBeNull();
    expect(screen.getByRole("button", { name: "Try again" })).toBeEnabled();
    fireEvent.change(screen.getByRole("textbox", { name: "Message" }), {
      target: { value: "Keep this draft" },
    });
    expect(screen.getByRole("button", { name: "Send" })).toBeDisabled();
    expect(posts()).toHaveLength(0);
  });
  it("WEB-U-018 selects the environment before creating on first send", async () => {
    page();
    const choices = await openChoices();
    const picker = await screen.findByRole("button", {
      name: "Environment: Workstation",
    });
    expect(within(choices).getByRole("button", { name: /^Environment:/ })).toBe(
      picker,
    );
    expect(within(picker).queryByRole("img")).toBeNull();
    fireEvent.click(picker);
    // The environment's tools read in words on its line in the picker, not as a row of icons.
    expect(
      screen.getByRole("menuitemradio", { name: /^Workstation/ }),
    ).toHaveTextContent("Shell, Python, Node.js, Build tools, Browser");
    fireEvent.click(screen.getByRole("menuitemradio", { name: /^Minimal/ }));
    expect(
      screen.queryByRole("group", { name: "Switch environment" }),
    ).toBeNull();
    expect(posts()).toHaveLength(0);
    await send();
    await waitFor(() => expect(posts()).toHaveLength(2));
    expect(requestBody(posts()[0]?.[1]?.body)).toEqual({
      title: "Create a greeting",
      model_route: "second",
      template: "minimal",
    });
  });
  // UI-GUIDELINES §0: no choices before the first word. What the task will use is one quiet line under the box, and
  // Change opens the workspace, model and environment together.
  // WEB-U-102 as the owner corrected it (WEB-U-113): New task's model and environment are two small pickers in its top
  // bar; the workspace and profile come from the composer's "+" (Attach files, Workspace…, Agent profile…) as chips; the
  // composer holds only "+", the text and Send; no quiet line, no Change panel.
  it("offers the model and environment in the top bar and the rest from +, and example chips only fill the composer", async () => {
    page();
    const composer = screen.getByRole("form", { name: "Message composer" });
    const bar = screen.getByRole("region", { name: "Task header" });
    const choices = await within(bar).findByRole("group", {
      name: "Model and environment",
    });
    expect(
      within(composer).queryByRole("group", { name: "Model and environment" }),
    ).toBeNull();
    expect(
      await within(choices).findByRole("button", {
        name: "Model: Local model",
      }),
    ).toBeVisible();
    expect(
      within(choices).getByRole("button", { name: /^Environment:/ }),
    ).toBeVisible();
    expect(
      screen.queryByRole("button", { name: "Change what this task uses" }),
    ).toBeNull();
    expect(screen.queryByText(/^Uses /)).toBeNull();
    // The top bar holds the two pickers and nothing else to press.
    expect(
      within(bar)
        .queryAllByRole("button")
        .map((button) => button.getAttribute("aria-label")),
    ).toEqual(["Model: Local model", expect.stringMatching(/^Environment:/)]);
    // "+" offers what else a new task can use.
    fireEvent.click(within(composer).getByRole("button", { name: "Add" }));
    expect(
      screen.getAllByRole("menuitem").map((item) => item.textContent),
    ).toEqual(["Attach files", "Workspace…"]);
    fireEvent.keyDown(screen.getAllByRole("menuitem")[0] as HTMLElement, {
      key: "Escape",
    });
    // Choosing a model applies at once.
    fireEvent.click(within(choices).getByRole("button", { name: /^Model:/ }));
    fireEvent.click(
      screen.getByRole("menuitemradio", { name: /Second model/ }),
    );
    expect(
      within(choices).getByRole("button", { name: "Model: Second model" }),
    ).toBeVisible();
    // WEB-U-128 (candidate12 F2): a new task's workspace is new and empty, so its suggestions need nothing in it.
    for (const example of [
      "Make a sample sales spreadsheet and chart the totals by region",
      "Draft a one-page plan for launching a newsletter",
      "Write a short Python script that lists the weekdays of a month, and test it",
    ]) {
      fireEvent.click(screen.getByRole("button", { name: example }));
      expect(screen.getByRole("textbox", { name: "Message" })).toHaveValue(
        example,
      );
      expect(screen.getByRole("textbox", { name: "Message" })).toHaveFocus();
    }
    expect(document.title).toBe("New task · TensorQuay Agent");
    expect(screen.getByRole("navigation")).not.toHaveTextContent(
      /Workspaces|Channels|Admin/,
    );
    expect(posts()).toHaveLength(0);
  });

  it("New task clears an unsent start draft without creating a conversation", async () => {
    page();
    await openChoices();
    await screen.findByRole("button", { name: "Model: Local model" });
    fireEvent.change(screen.getByRole("textbox", { name: "Message" }), {
      target: { value: "An unsent draft" },
    });
    fireEvent.click(screen.getByRole("link", { name: "New task" }));
    expect(screen.getByRole("textbox", { name: "Message" })).toHaveValue("");
    expect(screen.getByRole("textbox", { name: "Message" })).toHaveFocus();
    expect(posts()).toHaveLength(0);
  });
  it("focuses an empty composer without a dialog or creating on open, and uses the selected model only on send", async () => {
    const { cache } = page();
    const input = screen.getByRole("textbox", { name: "Message" });
    expect(input).toHaveAttribute("placeholder", "Describe the task…");
    await openChoices();
    await screen.findByRole("button", { name: "Model: Local model" });
    expect(input).toHaveFocus();
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(posts()).toHaveLength(0);
    fireEvent.change(input, { target: { value: "   " } });
    fireEvent.keyDown(input, { key: "Enter" });
    expect(posts()).toHaveLength(0);
    await send();
    await waitFor(() =>
      expect(push).toHaveBeenCalledWith(`/c/${conversationId}`, {
        scroll: false,
      }),
    );
    expect(posts()).toHaveLength(2);
    expect(requestBody(posts()[0]?.[1]?.body)).toEqual({
      title: "Create a greeting",
      model_route: "second",
      template: "workstation",
    });
    expect(requestBody(posts()[1]?.[1]?.body)).toEqual({
      text: "Create a greeting\nCheck it works",
    });
    const keys = posts().map(([, init]) =>
      new Headers(init?.headers).get("idempotency-key"),
    );
    for (const key of keys) expect(key).toMatch(/^[a-f0-9-]{36}$/);
    expect(keys[0]).not.toBe(keys[1]);
    expect(cache.getQueryData(["conversation", conversationId])).toEqual(
      record,
    );
    expect(
      await within(screen.getByRole("region", { name: "Tasks" })).findByRole(
        "link",
        { name: /Create a greeting/ },
      ),
    ).toHaveAttribute("href", `/c/${conversationId}`);
  });

  it("shows the optimistic prompt before create completes and keeps both keys on a failed-send retry", async () => {
    let finish: ((value: Response) => void) | undefined;
    page();
    await openChoices();
    await screen.findByRole("button", { name: "Model: Local model" });
    fetch.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          finish = resolve;
        }),
    );
    await send();
    expect(
      screen.getByRole("article", { name: "Your message" }),
    ).toHaveTextContent("Create a greeting");
    expect(screen.getByRole("button", { name: "Sending" })).toBeDisabled();
    expect(screen.getByRole("textbox", { name: "Message" })).toHaveValue("");
    expect(
      screen.queryByRole("heading", { name: "What should the agent do?" }),
    ).toBeNull();
    expect(
      screen.queryByRole("button", {
        name: "Clean up a CSV file and chart its totals",
      }),
    ).toBeNull();
    expect(screen.queryByText("Sending your message…")).toBeNull();
    expect(push).not.toHaveBeenCalled();
    fetch.mockRejectedValueOnce(new Error("Connection lost"));
    await act(() => {
      finish?.(Response.json(record, { status: 201 }));
      return Promise.resolve();
    });
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "The connection was lost",
    );
    expect(screen.getByRole("textbox", { name: "Message" })).toHaveValue(
      "Create a greeting\nCheck it works",
    );
    fireEvent.click(screen.getByRole("button", { name: "Retry message" }));
    await waitFor(() => expect(push).toHaveBeenCalledOnce());
    const writes = posts();
    expect(writes.map(([url]) => requestPath(url))).toEqual([
      "/api/v1/conversations",
      `/api/v1/conversations/${conversationId}/messages`,
      `/api/v1/conversations/${conversationId}/messages`,
    ]);
    expect(new Headers(writes[2]?.[1]?.headers).get("idempotency-key")).toBe(
      new Headers(writes[1]?.[1]?.headers).get("idempotency-key"),
    );
  });

  it("retains the create key across uncertain delivery and trims a long title at a word boundary", async () => {
    page();
    await openChoices();
    await screen.findByRole("button", { name: "Model: Local model" });
    const text =
      "Review this workspace and prepare a detailed implementation plan for the next release\nIgnore this second line";
    fireEvent.change(screen.getByRole("textbox", { name: "Message" }), {
      target: { value: text },
    });
    fetch.mockRejectedValueOnce(new Error("Lost create response"));
    fireEvent.click(screen.getByRole("button", { name: "Send" }));
    await screen.findByRole("alert");
    fireEvent.click(screen.getByRole("button", { name: "Retry message" }));
    await waitFor(() => expect(push).toHaveBeenCalledOnce());
    const writes = posts();
    expect(requestBody(writes[0]?.[1]?.body)).toMatchObject({
      title: "Review this workspace and prepare a detailed implementation…",
    });
    expect(writes[1]?.[1]?.body).toBe(writes[0]?.[1]?.body);
    expect(new Headers(writes[1]?.[1]?.headers).get("idempotency-key")).toBe(
      new Headers(writes[0]?.[1]?.headers).get("idempotency-key"),
    );
  });
});

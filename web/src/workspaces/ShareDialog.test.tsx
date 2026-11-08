import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { WorkspaceShare } from "../protocol/generated/api";
import { ana, fakeGateway, problem, workspace } from "../test/workspaces";

import { ShareDialog } from "./ShareDialog";

const sales = workspace({ name: "Sales analysis", share_count: 1 });
const base = `/api/v1/workspaces/${sales.id}/shares`;
const li = {
  user_id: "0195d8c0-0000-7000-8000-0000000000b2",
  email: "li@example.invalid",
  access: "read",
} as const satisfies WorkspaceShare;
const anaShare = {
  user_id: ana.id,
  email: ana.email,
  access: "write",
} as const satisfies WorkspaceShare;

function show(routes: Parameters<typeof fakeGateway>[0]) {
  const gateway = fakeGateway(routes);
  vi.stubGlobal("fetch", gateway.fetch);
  const onClose = vi.fn();
  const cache = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  render(
    <QueryClientProvider client={cache}>
      <ShareDialog workspace={sales} onClose={onClose} />
    </QueryClientProvider>,
  );
  return { ...gateway, onClose };
}
afterEach(() => {
  vi.unstubAllGlobals();
});

// Task 076 (answers-076 Q4 and mockup change 4): share by exact email; no directory of accounts.
describe("WEB-U-025 share dialog", () => {
  // The UI standard's audit: each change applies at once, so nothing is left to confirm; × and Esc close it, no Done.
  it("has no Done: its own close button closes it", async () => {
    const { onClose } = show({});
    expect(screen.queryByRole("button", { name: "Done" })).toBeNull();
    fireEvent.click(
      await screen.findByRole("button", { name: "Close dialog" }),
    );
    expect(onClose).toHaveBeenCalledOnce();
  });

  it("lists the current members and adds one by exact email", async () => {
    let shares: WorkspaceShare[] = [anaShare];
    const { calls } = show({
      [`GET ${base}`]: () => Response.json({ items: shares }),
      [`PUT ${base}`]: () => {
        shares = [anaShare, li];
        return Response.json(li);
      },
    });
    const dialog = await screen.findByRole("dialog", {
      name: "Share “Sales analysis”",
    });
    expect(await within(dialog).findByText(ana.email)).toBeInTheDocument();
    const email = within(dialog).getByRole("textbox", { name: "Email" });
    // No suggestions from any account list.
    expect(email).toHaveAttribute("autocomplete", "off");
    expect(email).not.toHaveAttribute("list");
    fireEvent.change(email, { target: { value: " li@example.invalid " } });
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Access: Can view" }),
    );
    fireEvent.click(screen.getByRole("menuitemradio", { name: /Can edit/ }));
    fireEvent.click(within(dialog).getByRole("button", { name: "Share" }));
    expect(await within(dialog).findByText(li.email)).toBeInTheDocument();
    expect(calls.find((call) => call.method === "PUT")).toEqual({
      method: "PUT",
      url: base,
      body: { email: "li@example.invalid", access: "write" },
    });
    expect(email).toHaveValue("");
  });

  // A screen review before 17.0: a quick share granted write access. The least access is the default.
  it("WEB-U-173 shares as Can view unless Can edit is chosen", async () => {
    const { calls } = show({
      [`GET ${base}`]: () => Response.json({ items: [] }),
      [`PUT ${base}`]: () => Response.json({ ...li, access: "read" }),
    });
    const dialog = await screen.findByRole("dialog", {
      name: "Share “Sales analysis”",
    });
    expect(
      within(dialog).getByRole("button", { name: "Access: Can view" }),
    ).toBeVisible();
    fireEvent.change(within(dialog).getByRole("textbox", { name: "Email" }), {
      target: { value: "li@example.invalid" },
    });
    fireEvent.click(within(dialog).getByRole("button", { name: "Share" }));
    await waitFor(() =>
      expect(calls.find((call) => call.method === "PUT")?.body).toEqual({
        email: "li@example.invalid",
        access: "read",
      }),
    );
  });

  // After a share with Can edit, the next person starts at Can view again.
  it("WEB-U-173 starts the next share at Can view again", async () => {
    show({
      [`GET ${base}`]: () => Response.json({ items: [] }),
      [`PUT ${base}`]: () => Response.json(li),
    });
    const dialog = await screen.findByRole("dialog", {
      name: "Share “Sales analysis”",
    });
    fireEvent.change(within(dialog).getByRole("textbox", { name: "Email" }), {
      target: { value: "li@example.invalid" },
    });
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Access: Can view" }),
    );
    fireEvent.click(screen.getByRole("menuitemradio", { name: /Can edit/ }));
    fireEvent.click(within(dialog).getByRole("button", { name: "Share" }));
    await waitFor(() =>
      expect(
        within(dialog).getByRole("textbox", { name: "Email" }),
      ).toHaveValue(""),
    );
    expect(
      within(dialog).getByRole("button", { name: "Access: Can view" }),
    ).toBeVisible();
  });

  it("puts an unknown email's error under the field and marks the field", async () => {
    show({
      [`GET ${base}`]: () => Response.json({ items: [] }),
      [`PUT ${base}`]: () => problem(404, "user_not_found"),
    });
    const dialog = await screen.findByRole("dialog");
    expect(
      await within(dialog).findByText("Not shared with anyone yet."),
    ).toBeInTheDocument();
    const email = within(dialog).getByRole("textbox", { name: "Email" });
    fireEvent.change(email, { target: { value: "nobody@example.invalid" } });
    fireEvent.click(within(dialog).getByRole("button", { name: "Share" }));
    const error = await within(dialog).findByText(
      "No account uses that email.",
    );
    expect(email).toHaveAttribute("aria-invalid", "true");
    expect(email.getAttribute("aria-describedby")).toContain(error.id);
    // Directly under the field: the field's own wrapper holds it.
    expect(email.closest(".field")).toContainElement(error);
    // Typing again clears it.
    fireEvent.change(email, { target: { value: "nobody@example.invali" } });
    expect(email).not.toHaveAttribute("aria-invalid");
  });

  it("changes a member's access with the app's picker and removes a member by name", async () => {
    let shares: WorkspaceShare[] = [anaShare, li];
    const { calls } = show({
      [`GET ${base}`]: () => Response.json({ items: shares }),
      [`PUT ${base}/${li.user_id}`]: () => {
        shares = [anaShare, { ...li, access: "write" }];
        return Response.json(shares[1]);
      },
      [`DELETE ${base}/${ana.id}`]: () => {
        shares = shares.filter((share) => share.user_id !== ana.id);
        return new Response(null, { status: 204 });
      },
    });
    const dialog = await screen.findByRole("dialog");
    fireEvent.click(
      await within(dialog).findByRole("button", {
        name: "Access for li@example.invalid: Can view",
      }),
    );
    fireEvent.click(screen.getByRole("menuitemradio", { name: /Can edit/ }));
    expect(
      await within(dialog).findByRole("button", {
        name: "Access for li@example.invalid: Can edit",
      }),
    ).toBeInTheDocument();
    const remove = within(dialog).getByRole("button", {
      name: "Remove ana@example.invalid",
    });
    expect(remove).toHaveAttribute("data-variant", "ghost");
    expect(remove).toHaveClass("danger-ghost");
    fireEvent.click(remove);
    await vi.waitFor(() =>
      expect(within(dialog).queryByText(ana.email)).toBeNull(),
    );
    expect(
      calls.filter((call) => call.method !== "GET").map((call) => call.body),
    ).toEqual([{ access: "write" }, null]);
  });
});

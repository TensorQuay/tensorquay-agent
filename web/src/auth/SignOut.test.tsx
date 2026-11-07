import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { api, ApiClient } from "../api/client";
import { conversationId } from "../test/frames";

import { AccountActions } from "./AccountActions";

const replace = vi.fn();
vi.mock("next/navigation", () => ({ useRouter: () => ({ replace }) }));
afterEach(() => {
  vi.restoreAllMocks();
  replace.mockReset();
});

function pendingResponse() {
  let respond: (response: Response) => void = () => {
    throw new Error("The request has not started");
  };
  const response = new Promise<Response>((resolve) => {
    respond = resolve;
  });
  return { response, respond };
}

function setup() {
  const logout = pendingResponse();
  const catalogue = pendingResponse();
  const redirect = vi.fn<(path: string) => void>();
  const currentPath = () => `/c/${conversationId}?view=latest`;
  const client = new ApiClient({
    fetch: (path) =>
      path === "/api/v1/auth/logout" ? logout.response : catalogue.response,
    redirect,
    currentPath,
  });
  vi.spyOn(api, "logout").mockImplementation((signal) => client.logout(signal));
  const cache = new QueryClient();
  cache.setQueryData(["session"], {
    id: conversationId,
    email: "member@example.invalid",
    role: "member",
  });
  const onClose = vi.fn();
  render(
    <QueryClientProvider client={cache}>
      <AccountActions onClose={onClose} />
    </QueryClientProvider>,
  );
  return { client, logout, catalogue, redirect, currentPath, onClose };
}

function unauthorized() {
  return Response.json(
    {
      error: {
        code: "unauthenticated",
        message: "Sign in required",
        retryable: false,
      },
    },
    { status: 401 },
  );
}

describe("WEB-U-007 deliberate sign-out", () => {
  it.each(["pending", "succeeded", "new query"] as const)(
    "keeps late 401s from replacing sign-out navigation: %s",
    async (phase) => {
      const { client, logout, catalogue, redirect, currentPath, onClose } =
        setup();
      let request = phase === "new query" ? undefined : client.templates();
      fireEvent.click(screen.getByRole("menuitem", { name: "Sign out" }));
      expect(
        screen.getByRole("menuitem", { name: "Signing out…" }),
      ).toBeDisabled();
      if (phase !== "pending") {
        await act(async () => {
          logout.respond(new Response(null, { status: 204 }));
          await logout.response;
        });
        await waitFor(() => expect(replace).toHaveBeenCalledWith("/login"));
      }
      request ??= client.templates();
      const rejected = expect(request).rejects.toMatchObject({ status: 401 });
      await act(async () => {
        catalogue.respond(unauthorized());
        await rejected;
      });
      if (phase === "pending") {
        expect(replace).not.toHaveBeenCalled();
        await act(async () => {
          logout.respond(new Response(null, { status: 204 }));
          await logout.response;
        });
      }
      await waitFor(() =>
        expect(replace).toHaveBeenCalledExactlyOnceWith("/login"),
      );
      expect(onClose).toHaveBeenCalledOnce();
      // The router has not committed navigation yet, even after successful logout.
      expect(currentPath()).toBe(`/c/${conversationId}?view=latest`);
      expect(redirect).not.toHaveBeenCalled();
    },
  );

  it("keeps failed sign-out retryable and restores ordinary expiry redirects", async () => {
    const { client, logout, catalogue, redirect, currentPath, onClose } =
      setup();
    fireEvent.click(screen.getByRole("menuitem", { name: "Sign out" }));
    await act(async () => {
      logout.respond(
        Response.json(
          {
            error: {
              code: "unavailable",
              message: "Please try again.",
              retryable: true,
            },
          },
          { status: 503 },
        ),
      );
      await logout.response;
    });
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Please try again.",
    );
    expect(screen.getByRole("menuitem", { name: "Sign out" })).toBeEnabled();
    expect(onClose).not.toHaveBeenCalled();
    expect(replace).not.toHaveBeenCalled();
    const request = client.templates();
    catalogue.respond(unauthorized());
    await expect(request).rejects.toMatchObject({ status: 401 });
    expect(redirect).toHaveBeenCalledExactlyOnceWith(
      `/login?next=${encodeURIComponent(currentPath())}`,
    );
  });
});

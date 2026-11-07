// @vitest-environment node
import { afterEach, describe, expect, it, vi } from "vitest";

import { conversationId } from "../test/frames";
import { LIMITS } from "../test/user";

import { ApiClient } from "./client";

const user = {
  id: conversationId,
  email: "member@example.invalid",
  role: "member",
  internet: "none",
  limits: LIMITS,
  features: { agent_teams: false },
};
const credentials = { email: user.email, password: "synthetic-password" };
const path = `/c/${conversationId}?view=latest`;
const unauthorized = () =>
  Response.json(
    {
      error: {
        code: "unauthenticated",
        message: "Sign in required",
        retryable: false,
      },
    },
    { status: 401 },
  );

function setup() {
  const fetch = vi.fn<typeof globalThis.fetch>();
  const redirect = vi.fn<(path: string) => void>();
  const client = new ApiClient({ fetch, redirect, currentPath: () => path });
  return { fetch, redirect, client };
}

afterEach(() => vi.useRealTimers());

describe("WEB-U-007 session redirect policy", () => {
  it.each(["login", "bootstrap"] as const)(
    "restores expiry handling only after a successful %s",
    async (operation) => {
      const { client, fetch, redirect } = setup();
      fetch.mockResolvedValueOnce(new Response(null, { status: 204 }));
      await client.logout();
      const signIn = () =>
        operation === "login"
          ? client.login(credentials)
          : client.bootstrap({ ...credentials, token: "synthetic-bootstrap" });
      fetch.mockResolvedValueOnce(unauthorized());
      await expect(signIn()).rejects.toMatchObject({ status: 401 });
      fetch.mockResolvedValueOnce(unauthorized());
      await expect(client.me()).rejects.toMatchObject({ status: 401 });
      expect(redirect).not.toHaveBeenCalled();
      fetch.mockResolvedValueOnce(Response.json(user));
      await expect(signIn()).resolves.toEqual(user);
      fetch.mockResolvedValueOnce(unauthorized());
      await expect(client.me()).rejects.toMatchObject({ status: 401 });
      expect(redirect).toHaveBeenCalledExactlyOnceWith(
        `/login?next=${encodeURIComponent(path)}`,
      );
    },
  );

  it("ignores a previous session's late response after the next sign-in", async () => {
    const { client, fetch, redirect } = setup();
    let respond: (response: Response) => void = () => {
      throw new Error("No pending request");
    };
    fetch.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          respond = resolve;
        }),
    );
    const request = client.models();
    fetch.mockResolvedValueOnce(new Response(null, { status: 204 }));
    await client.logout();
    fetch.mockResolvedValueOnce(Response.json(user));
    await client.login(credentials);
    respond(unauthorized());
    await expect(request).rejects.toMatchObject({ status: 401 });
    expect(redirect).not.toHaveBeenCalled();
    fetch.mockResolvedValueOnce(unauthorized());
    await expect(client.models()).rejects.toMatchObject({ status: 401 });
    expect(redirect).toHaveBeenCalledExactlyOnceWith(
      `/login?next=${encodeURIComponent(path)}`,
    );
  });

  it.each(["network", "cancelled", "timeout"] as const)(
    "restores expiry handling when logout fails with %s",
    async (cause) => {
      vi.useFakeTimers();
      const { client, fetch, redirect } = setup();
      fetch.mockImplementationOnce(
        (_path, init) =>
          new Promise((_resolve, reject) => {
            if (cause === "network")
              reject(new TypeError("Network unavailable"));
            else
              init?.signal?.addEventListener(
                "abort",
                () => reject(new DOMException("Aborted", "AbortError")),
                { once: true },
              );
          }),
      );
      const controller = new AbortController();
      const rejected = expect(
        client.logout(controller.signal),
      ).rejects.toMatchObject({ code: cause });
      if (cause === "cancelled") controller.abort();
      if (cause === "timeout") await vi.advanceTimersByTimeAsync(10_000);
      await rejected;
      fetch.mockResolvedValueOnce(unauthorized());
      await expect(client.me()).rejects.toMatchObject({ status: 401 });
      expect(redirect).toHaveBeenCalledExactlyOnceWith(
        `/login?next=${encodeURIComponent(path)}`,
      );
    },
  );
});

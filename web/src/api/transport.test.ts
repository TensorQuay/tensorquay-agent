// @vitest-environment node
import { afterEach, describe, expect, it, vi } from "vitest";

import { delay, Transport } from "./transport";

function transport(answer: (init?: RequestInit) => Promise<Response>) {
  const fetch = vi.fn((_url: RequestInfo | URL, init?: RequestInit) =>
    answer(init),
  );
  return {
    fetch,
    client: new Transport({
      fetch,
      redirect: vi.fn(),
      currentPath: () => "/",
      sleep: vi.fn().mockResolvedValue(undefined),
    }),
  };
}
const json = { "content-type": "application/json" };
afterEach(() => vi.useRealTimers());

// The browser's reading of gateway answers stays bounded and fails with a plain code (GW-U-022).
describe("GW-U-022 transport bounds and failures", () => {
  it("refuses an answer that is not JSON, is empty or cannot be parsed", async () => {
    for (const response of [
      () =>
        new Response("<html></html>", {
          headers: { "content-type": "text/html" },
        }),
      () => new Response(null, { status: 200, headers: json }),
      () => new Response("{not json", { headers: json }),
    ]) {
      const { client } = transport(() => Promise.resolve(response()));
      await expect(
        client.request("me", (value) => value),
      ).rejects.toMatchObject({
        code: "invalid_response",
      });
    }
  });

  // A request the gateway accepted to do later (202, no body, as a template check) is a success with no value; a
  // 200 without a body stays unreadable.
  it("takes an accepted request without a body as done, and still refuses an empty 200", async () => {
    const { client } = transport(() =>
      Promise.resolve(new Response(null, { status: 202 })),
    );
    await expect(client.request("me", (value) => value)).resolves.toBeNull();
    const empty = transport(() =>
      Promise.resolve(new Response(null, { status: 200 })),
    );
    await expect(
      empty.client.request("me", (value) => value),
    ).rejects.toMatchObject({
      code: "invalid_response",
    });
  });

  // A contract failure says which route and which definition it was, for whoever reads the error (the page words it by
  // its code, never this message).
  it("names the route and the definition an answer failed", async () => {
    const { client } = transport(() =>
      Promise.resolve(new Response('{"id":1}', { headers: json })),
    );
    const failed = await client
      .request("me", () => {
        throw new Error("Invalid API response: User (urn:tqa:api)");
      })
      .catch((error: unknown) => error);
    expect(failed).toMatchObject({ code: "invalid_response" });
    expect(failed instanceof Error ? failed.message : "").toBe(
      "The server response could not be read. Please try again. (GET /api/v1/auth/me: Invalid API response: User (urn:tqa:api))",
    );
  });

  it("stops reading an answer larger than 2 MiB", async () => {
    const big = `"${"x".repeat(2 * 1024 * 1024)}"`;
    const { client } = transport(() =>
      Promise.resolve(new Response(big, { headers: json })),
    );
    await expect(client.request("me", (value) => value)).rejects.toMatchObject({
      code: "invalid_response",
      message: "The server response was too large.",
    });
  });

  it("does not send a request whose signal was already cancelled, and says it was cancelled", async () => {
    const { client, fetch } = transport((init) =>
      init?.signal?.aborted
        ? Promise.reject(new DOMException("aborted", "AbortError"))
        : Promise.resolve(Response.json({})),
    );
    const cancelled = new AbortController();
    cancelled.abort();
    await expect(
      client.request("me", (value) => value, { signal: cancelled.signal }),
    ).rejects.toMatchObject({ code: "cancelled" });
    expect(fetch.mock.calls[0]?.[1]?.signal?.aborted).toBe(true);
  });

  it("waits the retry delay, and stops at once when cancelled before or during it", async () => {
    vi.useFakeTimers();
    const waited = vi.fn();
    const wait = delay(1000).then(waited);
    await vi.advanceTimersByTimeAsync(999);
    expect(waited).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(1);
    await wait;
    expect(waited).toHaveBeenCalledOnce();

    const before = new AbortController();
    before.abort();
    await expect(delay(1000, before.signal)).rejects.toMatchObject({
      code: "cancelled",
    });

    const during = new AbortController();
    const pending = delay(1000, during.signal);
    during.abort();
    await expect(pending).rejects.toMatchObject({ code: "cancelled" });
    expect(vi.getTimerCount()).toBe(0);
  });
});

// WEB-U-138 (the c13c how-to check): a slow answer the gateway bounds itself, such as a route check on a local model,
// waits that bound; every other request still stops at 10 s.
describe("WEB-U-138 a request that names its own timeout", () => {
  /** An answer that arrives after `ms`, unless the request is aborted first. */
  const after = (ms: number) => (init?: RequestInit) =>
    new Promise<Response>((resolve, reject) => {
      const timer = setTimeout(
        () => resolve(Response.json({ ok: true }, { headers: json })),
        ms,
      );
      init?.signal?.addEventListener("abort", () => {
        clearTimeout(timer);
        reject(new DOMException("aborted", "AbortError"));
      });
    });

  it("stops any other request at 10 s", async () => {
    vi.useFakeTimers();
    const { client } = transport(after(12_000));
    const answer = client.request("me", (value) => value);
    const failed = expect(answer).rejects.toMatchObject({ code: "timeout" });
    await vi.advanceTimersByTimeAsync(10_000);
    await failed;
  });

  it("waits the timeout a request names", async () => {
    vi.useFakeTimers();
    const { client } = transport(after(12_000));
    const answer = client.request("me", (value) => value, {
      timeoutMs: 150_000,
    });
    await vi.advanceTimersByTimeAsync(12_000);
    await expect(answer).resolves.toEqual({ ok: true });
  });
});

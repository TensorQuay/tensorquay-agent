import {
  QueryClient,
  QueryClientProvider,
  notifyManager,
  useQuery,
} from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import { afterAll, describe, expect, it, vi } from "vitest";

import { delayNotifications } from "./notify";

// §19a, WEB-U-195: a query notification queued as a test ends ran after the file's jsdom was torn down, and React threw
// "window is not defined" outside any test (seen once in LiveConversation.test.tsx under full-suite load). React's store
// subscription reads `window` for its update timer before it looks whether the component is still mounted, so a
// notification queued while mounted still reaches it after RTL's cleanup. Here the notification is forced 200 ms past
// its test and the file's teardown is made slow on purpose (`window` gone for 300 ms, as jsdom's teardown leaves it):
// without the shared setup's wait the same error comes every time.
// Query notifications delivered since the cache update (each runs through the notify function).
let delivered = 0;

function Watched() {
  const { data } = useQuery({
    queryKey: ["watched"],
    queryFn: () => new Promise<string>(() => {}),
    initialData: "first",
  });
  return <p>{data}</p>;
}

describe("WEB-U-195 query notifications never outlive their test", () => {
  afterAll(async () => {
    const saved = Object.getOwnPropertyDescriptor(globalThis, "window");
    Reflect.deleteProperty(globalThis, "window");
    await new Promise((resolve) => setTimeout(resolve, 300));
    if (saved) Object.defineProperty(globalThis, "window", saved);
    delayNotifications(0);
    notifyManager.setNotifyFunction((callback) => callback());
  });

  it("a notification queued as a test ends is still delivered", async () => {
    delayNotifications(200);
    const client = new QueryClient();
    render(
      <QueryClientProvider client={client}>
        <Watched />
      </QueryClientProvider>,
    );
    expect(await screen.findByText("first")).toBeInTheDocument();
    notifyManager.setNotifyFunction((callback) => {
      delivered += 1;
      callback();
    });
    // Queued for the mounted observer as the test ends.
    client.setQueryData(["watched"], "second");
    expect(delivered).toBe(0);
  });

  it("runs before the next test begins, while the environment is still there", () => {
    expect(delivered).toBeGreaterThan(0);
  });

  // A test that ends with fake timers still on and a notification pending: the wait runs the
  // faked notification and its bound stays on real time, so it settles, or fails within 1 s naming it.
  it("a test ending with fake timers on and a notification pending still settles", async () => {
    const client = new QueryClient();
    render(
      <QueryClientProvider client={client}>
        <Watched />
      </QueryClientProvider>,
    );
    expect(await screen.findByText("first")).toBeInTheDocument();
    delivered = 0;
    vi.useFakeTimers();
    client.setQueryData(["watched"], "third");
    expect(delivered).toBe(0);
  });

  it("delivers it before the next test, which puts real timers back", () => {
    const settled = delivered;
    vi.useRealTimers();
    expect(settled).toBeGreaterThan(0);
  });
});

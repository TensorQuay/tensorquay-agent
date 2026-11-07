import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { fakeHostKeys } from "../../test/host-keys";

import { NewHostKeyPage } from "./NewHostKeyPage";

// The router's address, as a client navigation from a task's "Add a key for {host}" gives it: window.location still
// shows the page it came from while the new page renders (the review of f8780321, item 1), so only the router knows.
const router = vi.hoisted(() => ({
  search: "",
  replace: vi.fn(),
}));
vi.mock("next/navigation", () => ({
  useRouter: () => ({ push: vi.fn(), replace: router.replace }),
  usePathname: () => "/settings/keys/new",
  useSearchParams: () => new URLSearchParams(router.search),
}));
afterEach(() => {
  router.search = "";
  router.replace.mockReset();
  vi.restoreAllMocks();
});

function show(search: string) {
  router.search = search;
  const cache = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(
    <QueryClientProvider client={cache}>
      <NewHostKeyPage api={fakeHostKeys().api} />
    </QueryClientProvider>,
  );
}
const field = (name: string) => screen.getByLabelText(name, { exact: true });

// A task's "Add a key for {host}" link (WEB-U-148) opens Add a key with the address filled in.
describe("WEB-U-142 Add a key opened for an address", () => {
  it("fills in the address the task linked to, and suggests its name", async () => {
    expect(window.location.search).toBe("");
    show("?address=git.acme.lan%3A8443");
    expect(field("Web address")).toHaveValue("git.acme.lan:8443");
    // GIT_TOKEN is kept for the task's own use, so the suggestion steps around it.
    await waitFor(() => expect(field("Name")).toHaveValue("KEY_GIT_TOKEN"));
  });

  it("takes the address off the page's own address once read, so a reload doesn't ask again", async () => {
    show("?address=api.notion.com");
    await waitFor(() =>
      expect(router.replace).toHaveBeenCalledWith("/settings/keys/new"),
    );
    expect(field("Web address")).toHaveValue("api.notion.com");
  });

  it("starts empty, and leaves the page's address alone, without one", () => {
    show("");
    expect(field("Web address")).toHaveValue("");
    expect(router.replace).not.toHaveBeenCalled();
  });

  it("ignores an address too long to be one", () => {
    show(`?address=${"a".repeat(1100)}.com`);
    expect(field("Web address")).toHaveValue("");
  });
});

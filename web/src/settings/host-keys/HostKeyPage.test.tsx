import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { ApiError } from "../../api/error";
import { english } from "../../i18n/translate";
import { fakeHostKeys, hostKey } from "../../test/host-keys";

import type { HostKeysApi } from "./contract";
import { draftProblems } from "./host-key-form";
import { HostKeyPage } from "./HostKeyPage";

vi.mock("next/navigation", () => ({
  useRouter: () => ({ push: vi.fn(), replace: vi.fn() }),
  usePathname: () => "/settings/keys/new",
}));
afterEach(() => vi.restoreAllMocks());

function show(api: HostKeysApi, extra: { address?: string } = {}) {
  const cache = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  const onDone = vi.fn();
  render(
    <QueryClientProvider client={cache}>
      <HostKeyPage api={api} onDone={onDone} {...extra} />
    </QueryClientProvider>,
  );
  return { onDone };
}
const field = (name: string) => screen.getByLabelText(name, { exact: true });
const type = (name: string, value: string) =>
  fireEvent.change(field(name), { target: { value } });
const save = () =>
  fireEvent.click(screen.getByRole("button", { name: "Save" }));

// ADR-0037 and the reviewed Keys sketch §3: Add a key is a Page (the UI standard): where it's used, how the service
// expects it, and the key itself, written once and never shown again.
describe("WEB-U-142 the Add a key page", () => {
  it("is a page with its sections, and suggests a name from the address until it's edited", async () => {
    show(fakeHostKeys().api);
    expect(screen.getByRole("heading", { name: "Add a key" })).toBeVisible();
    for (const section of [
      "Where it's used",
      "How the service expects it",
      "The key",
    ])
      expect(screen.getByRole("region", { name: section })).toBeVisible();
    type("Web address", "https://github.com/acme/");
    await waitFor(() => expect(field("Name")).toHaveValue("GITHUB_TOKEN"));
    type("Name", "GITHUB_WORK");
    type("Web address", "api.github.com");
    expect(field("Name")).toHaveValue("GITHUB_WORK");
    // The key is a password field, never shown.
    expect(field("Key")).toHaveAttribute("type", "password");
  });

  it("takes the address a task linked to, and counts nothing as unsaved yet", () => {
    show(fakeHostKeys().api, { address: "api.notion.com" });
    expect(field("Web address")).toHaveValue("api.notion.com");
    // Nothing was typed, so leaving doesn't ask.
    expect(screen.queryByText("Unsaved changes")).toBeNull();
  });

  // The suggestion waits for the person's keys, so it never offers a taken name.
  it("suggests, for an address it starts with, a name the person's keys don't have", async () => {
    show(
      fakeHostKeys({
        keys: [
          hostKey({
            name: "GITHUB_TOKEN",
            host: "api.github.com",
            path_prefix: null,
          }),
        ],
      }).api,
      { address: "github.com/acme" },
    );
    await waitFor(() => expect(field("Name")).toHaveValue("GITHUB_TOKEN_2"));
  });

  it("offers a token or a user name, and asks for the user name only under its choice", () => {
    show(fakeHostKeys().api);
    const token = screen.getByRole("radio", { name: /^As a token/ });
    expect(token).toBeChecked();
    expect(screen.queryByLabelText("User name")).toBeNull();
    fireEvent.click(screen.getByRole("radio", { name: /^With a user name/ }));
    expect(field("User name")).toBeVisible();
    // A named header waits for the gateway's second slice.
    expect(
      screen.queryByRole("radio", { name: /^In a named header/ }),
    ).toBeNull();
  });

  it.each([
    ["an empty address", "", "Enter the service's web address."],
    ["plain http", "http://api.github.com", "Keys are sent only over https."],
    [
      "a query",
      "https://api.example.com/v1?key=1",
      "Use the service's address only, without a name, password, or anything after ? or #.",
    ],
    [
      "a wildcard",
      "*.github.com",
      "One exact address; add another key for another host.",
    ],
  ])("refuses %s before sending", async (_, address, words) => {
    const { api, calls } = fakeHostKeys();
    show(api);
    type("Web address", address);
    type("Name", "GITHUB_WORK");
    type("Key", "ghp_example");
    save();
    expect(await screen.findByText(words)).toBeVisible();
    expect(field("Web address")).toHaveAttribute("aria-invalid", "true");
    expect(calls.addKey).not.toHaveBeenCalled();
  });

  it.each([
    [
      "an invalid name",
      "github_work",
      "Use capital letters, digits and _, starting with a letter.",
    ],
    [
      "a reserved name",
      "PATH",
      "PATH is kept for the task's own use. Pick another name.",
    ],
    [
      "a name of the person's own",
      "NOTION_KEY",
      "NOTION_KEY is already your key for api.notion.com. Pick another name.",
    ],
    [
      "a shared key's name",
      "PYPI_MIRROR",
      "PYPI_MIRROR is already the shared key for pypi.acme.lan. Pick another name.",
    ],
  ])("refuses %s before sending", async (_, name, words) => {
    const { api, calls } = fakeHostKeys({
      keys: [
        hostKey({
          name: "NOTION_KEY",
          host: "api.notion.com",
          path_prefix: null,
        }),
      ],
      shared: [
        hostKey({
          name: "PYPI_MIRROR",
          host: "pypi.acme.lan",
          path_prefix: null,
          shared: true,
        }),
      ],
    });
    show(api);
    await waitFor(() => expect(calls.shared).toHaveBeenCalled());
    type("Web address", "github.com/acme");
    type("Name", name);
    type("Key", "ghp_example");
    save();
    expect(await screen.findByText(words)).toBeVisible();
    expect(calls.addKey).not.toHaveBeenCalled();
  });

  // The user name a service expects with the key is asked for before sending.
  it("asks for the user name when the key goes with one", async () => {
    const { api, calls } = fakeHostKeys();
    show(api);
    type("Web address", "github.com/acme");
    fireEvent.click(screen.getByRole("radio", { name: /^With a user name/ }));
    type("Key", "ghp_example");
    save();
    expect(
      await screen.findByText(
        "Enter the user name the service expects with this key.",
      ),
    ).toBeVisible();
    expect(field("User name")).toHaveAttribute("aria-invalid", "true");
    expect(calls.addKey).not.toHaveBeenCalled();
  });

  it.each([
    ["no key", "", "Paste the key."],
    // A field keeps one line (it drops line breaks), so a key too long is what the page can meet.
    [
      "a key over 8 KiB",
      "a".repeat(8193),
      "Paste the key on one line, up to 8 kB.",
    ],
  ])("refuses %s before sending", async (_, value, words) => {
    const { api, calls } = fakeHostKeys();
    show(api);
    type("Web address", "github.com/acme");
    fireEvent.change(field("Key"), { target: { value } });
    save();
    expect(await screen.findByText(words)).toBeVisible();
    expect(calls.addKey).not.toHaveBeenCalled();
  });

  it("sends the key once, its address as the https URL it read (WEB-U-192), and goes back saying what it saved", async () => {
    const { api, added } = fakeHostKeys();
    const { onDone } = show(api);
    type("Web address", " https://github.com/acme/ ");
    type("Name", "GITHUB_WORK");
    fireEvent.click(screen.getByRole("radio", { name: /^With a user name/ }));
    type("User name", "robot");
    type("Key", "ghp_example");
    save();
    await waitFor(() =>
      expect(onDone).toHaveBeenCalledWith("Saved GITHUB_WORK."),
    );
    expect(added).toEqual([
      {
        name: "GITHUB_WORK",
        address: "https://github.com/acme",
        scheme: "basic",
        user_name: "robot",
        value: "ghp_example",
      },
    ]);
  });

  it.each([
    [
      "an address the gateway refuses",
      new ApiError(400, "invalid_request", "Bad.", undefined, [
        { field: "address", reason: "address_not_https" },
      ]),
      "Keys are sent only over https.",
    ],
    [
      "a name another key took meanwhile",
      new ApiError(409, "name_taken", "Taken.", undefined, [
        { field: "name", reason: "name_taken" },
      ]),
      "This name is already another key's. Pick another name.",
    ],
    [
      // A refused way of sending is the page's line, not the address's.
      "a way of sending it doesn't take yet",
      new ApiError(400, "invalid_request", "Bad.", undefined, [
        { field: "scheme", reason: "scheme_not_supported" },
      ]),
      "This way of sending a key isn't available yet.",
    ],
    [
      "a sealed store",
      new ApiError(503, "store_unavailable", "Sealed."),
      "The key could not be saved. Try again in a moment.",
    ],
    [
      "too many keys",
      new ApiError(409, "host_key_limit", "Limit."),
      "You have as many keys as you can keep (64). Delete one first.",
    ],
  ])(
    "says so in words when the gateway refuses %s",
    async (_, error, words) => {
      const { api } = fakeHostKeys();
      api.addKey = vi.fn(() => Promise.reject(error));
      const { onDone } = show(api);
      type("Web address", "github.com/acme");
      type("Name", "GITHUB_WORK");
      type("Key", "ghp_example");
      save();
      expect(await screen.findByText(words)).toBeVisible();
      expect(onDone).not.toHaveBeenCalled();
      // The value stays in its field to try again, and nowhere else on the page.
      expect(field("Key")).toHaveValue("ghp_example");
      expect(document.body).not.toHaveTextContent("ghp_example");
    },
  );
});

// WEB-U-266 (the c14b first hour, F2): an http address was refused only on Save. It is now refused once the person
// leaves the field, then checked as they type; an empty field left on the way through says nothing, and nothing else
// is checked before Save.
describe("WEB-U-266 the address is checked when its field is left", () => {
  it("refuses http on leaving the field, and clears as it becomes https", async () => {
    const { api, calls } = fakeHostKeys();
    show(api);
    fireEvent.blur(field("Web address"));
    expect(screen.queryByText("Enter the service's web address.")).toBeNull();
    type("Web address", "http://api.example.com");
    expect(screen.queryByText("Keys are sent only over https.")).toBeNull();
    fireEvent.blur(field("Web address"));
    expect(
      await screen.findByText("Keys are sent only over https."),
    ).toBeVisible();
    expect(field("Web address")).toHaveAttribute("aria-invalid", "true");
    // Nothing else is judged before Save: the empty Key says nothing yet.
    expect(screen.queryByText(/Paste the key/)).toBeNull();
    type("Web address", "https://api.example.com");
    expect(screen.queryByText("Keys are sent only over https.")).toBeNull();
    expect(field("Web address")).not.toHaveAttribute("aria-invalid", "true");
    expect(calls.addKey).not.toHaveBeenCalled();
  });
});

describe("WEB-U-142 the key's own checks", () => {
  it("refuses a key with a line break, which another way of filling the field could bring", () => {
    const draft = {
      address: "github.com/acme",
      name: "GITHUB_WORK",
      scheme: "bearer" as const,
      userName: "",
      headerName: "",
      value: "abc\ndef",
      users: null,
    };
    expect(draftProblems(draft, { own: [], shared: [] }, english).value).toBe(
      "Paste the key on one line, up to 8 kB.",
    );
    expect(
      draftProblems(
        { ...draft, value: "abc" },
        { own: [], shared: [] },
        english,
      ),
    ).toEqual({});
  });
});

// A name another key took meanwhile is said at Name, however the gateway says it.
describe("WEB-U-142 a name taken meanwhile", () => {
  it("marks Name for a 409 name_taken without details too", async () => {
    const { api } = fakeHostKeys({
      refuse: { status: 409, code: "name_taken" },
    });
    show(api);
    type("Web address", "github.com/acme");
    type("Name", "GITHUB_WORK");
    type("Key", "ghp_example");
    save();
    expect(
      await screen.findByText(
        "This name is already another key's. Pick another name.",
      ),
    ).toBeVisible();
    expect(field("Name")).toHaveAttribute("aria-invalid", "true");
  });
});

// A refused way of sending is said on the page's line, never under Web address.
describe("WEB-U-142 a way of sending the gateway doesn't take yet", () => {
  it("says it on the page's line, leaving the address alone", async () => {
    const { api } = fakeHostKeys();
    api.addKey = vi.fn(() =>
      Promise.reject(
        new ApiError(400, "invalid_request", "Bad.", undefined, [
          { field: "scheme", reason: "scheme_not_supported" },
        ]),
      ),
    );
    show(api);
    type("Web address", "github.com/acme");
    type("Name", "GITHUB_WORK");
    type("Key", "ghp_example");
    save();
    const line = await screen.findByText(
      "This way of sending a key isn't available yet.",
    );
    expect(line.closest(".settings-form-footer")).not.toBeNull();
    expect(field("Web address")).not.toHaveAttribute("aria-invalid", "true");
  });
});

import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { ApiError } from "../api/error";
import { showModels } from "../test/models-app";
import { fakeRoutes, route } from "../test/routes";

import type { RoutesApi } from "./admin/routes";
import { connectorProblems } from "./connector-copy";
import { fromDraft, gatewayProblems, toDraft } from "./route-copy";
import { secretProblems } from "./secret-copy";

/** Replaced by the harness once it renders (showModels). */
const router = vi.hoisted(() => ({ push: vi.fn<(href: string) => void>() }));
vi.mock("next/navigation", () => ({ useRouter: () => router }));
vi.mock("next/link", async () => {
  const { createElement } = await import("react");
  return {
    default: ({ href, children, ...rest }: Record<string, unknown>) =>
      createElement("a", { href, ...rest }, children as never),
  };
});

// Obviously fake: never a real provider key.
const KEY = "fake-provider-key-0001";

function show(api: RoutesApi) {
  showModels(api, router);
  // The whole page: a key must never reach any of it.
  return document.body;
}
/** The route's page (WEB-U-115): its form, from the title down to Save and Cancel. */
const page = () => {
  const form = screen
    .getByRole("group", { name: "Save or cancel" })
    .closest("form");
  if (!form) throw new Error("The page is a form");
  return form;
};
/** The open dialog (a route's check, a delete), else the route's page, where the route's form now is. */
const dialog = () => screen.queryByRole("dialog") ?? page();
async function openEdit(name: string) {
  await screen.findByRole("table", { name: "Model routes" });
  fireEvent.click(
    await screen.findByRole("link", { name: `Edit route ${name}` }),
  );
  await screen.findByRole("group", { name: "Save or cancel" });
  // Editing sums the key up in one line; these tests work with the Secret field and Paste key under it.
  const another = within(dialog()).queryByRole("button", {
    name: "Use another shared key",
  });
  if (another) fireEvent.click(another);
}
function paste(name: string | null, value: string) {
  const button = within(dialog()).queryByRole("button", { name: "Paste key" });
  if (button) fireEvent.click(button);
  const panel = within(dialog()).getByRole("group", { name: "Paste a key" });
  if (name !== null)
    fireEvent.change(within(panel).getByLabelText("Secret name"), {
      target: { value: name },
    });
  fireEvent.change(within(panel).getByLabelText("Key", { selector: "input" }), {
    target: { value },
  });
  fireEvent.click(within(panel).getByRole("button", { name: "Store key" }));
  return panel;
}

// A route's Secret field holds only a reference to a stored secret, and Paste key
// stores a key as a new secret and fills the reference, never overwriting one (WEB-U-056).
describe("WEB-U-056 the route's key is always a stored secret", () => {
  it("refuses anything but secrets/<name> in the Secret field before sending", () => {
    const draft = toDraft("local", route("local", { display_name: "Local" }));
    for (const key_ref of [
      "sk-abc123",
      KEY,
      "openai",
      "secrets/",
      "secrets/a/b",
      "secrets/has space",
    ])
      expect(fromDraft({ ...draft, key_ref }).problems.key_ref).toBe(
        "models.keyRefNotReference",
      );
    expect(
      fromDraft({ ...draft, key_ref: " secrets/openai " }).fields.key_ref,
    ).toBe("secrets/openai");
    expect(
      fromDraft({ ...draft, key_ref: KEY, authKind: "none" }).problems,
    ).toEqual({});
  });

  it("reads the gateway's key reference refusals in words", () => {
    expect(
      gatewayProblems([{ field: "key_ref", reason: "key_ref_not_reference" }]),
    ).toEqual({ key_ref: "models.keyRefNotReference" });
    expect(
      gatewayProblems([{ field: "key_ref", reason: "key_ref_reserved" }]),
    ).toEqual({ key_ref: "models.keyRefReserved" });
    expect(
      connectorProblems(
        new ApiError(400, "invalid_request", "x", undefined, [
          { field: "secret_ref", reason: "secret_ref_reserved" },
        ]),
      ),
    ).toMatchObject({ secretRef: "connectors.secretReserved" });
  });

  it("reserves git- and connector- names in any case", () => {
    const reason = (name: string) =>
      secretProblems({ name, value: KEY }, true).map(
        (problem) => problem.reason,
      );
    for (const name of ["git-work", "GIT-work", "connector-x", "Connector-x"])
      expect(reason(name)).toEqual(["reserved_prefix"]);
    for (const name of ["gitlab-key", "connectors", "my-connector-key"])
      expect(reason(name)).toEqual([]);
  });

  it("WEB-U-291: stores a pasted key as a new secret, fills the reference and saves the route with it", async () => {
    const fake = fakeRoutes({ routes: [route("local")] });
    const container = show(fake.api);
    await openEdit("Local model");
    expect(
      within(dialog()).getByLabelText("Shared key", { exact: true }),
    ).toHaveAttribute("autocomplete", "off");
    fireEvent.click(
      within(dialog()).getByRole("button", { name: "Paste key" }),
    );
    // The name is suggested from the route and can be changed.
    expect(within(dialog()).getByLabelText("Secret name")).toHaveValue(
      "local-key",
    );
    expect(
      within(dialog()).getByLabelText("Key", { selector: "input" }),
    ).toHaveAttribute("type", "password");
    // The panel's buttons belong to it, not the dialog's footer, and Save stays the dialog's one primary action.
    const store = within(dialog()).getByRole("button", { name: "Store key" });
    expect(store.closest(".dialog-actions")).toBeNull();
    expect(store.closest(".paste-key-panel")).not.toBeNull();
    expect(store).toHaveAttribute("data-variant", "secondary");
    paste(null, KEY);
    expect(
      await within(dialog()).findByText(
        "Stored the key as local-key. Save the route to use it.",
      ),
    ).toBeVisible();
    expect(fake.secretWrites).toEqual([{ name: "local-key", value: KEY }]);
    // The reference is filled: the key reads as one line again, naming it.
    expect(
      within(dialog()).getByText("Uses the shared key local-key."),
    ).toBeVisible();
    expect(container.innerHTML).not.toContain(KEY);
    // Display name is required before a save.
    fireEvent.change(within(dialog()).getByLabelText("Display name"), {
      target: { value: "Local" },
    });
    fireEvent.click(within(dialog()).getByRole("button", { name: "Save" }));
    expect(await screen.findByText(/^Saved route Local\./)).toBeVisible();
    expect(fake.writes.at(-1)?.fields?.key_ref).toBe("secrets/local-key");
    expect(JSON.stringify(fake.writes)).not.toContain(KEY);
  });

  it("WEB-U-291: asks for another name when one exists, and never overwrites it", async () => {
    const fake = fakeRoutes({
      routes: [route("local")],
      secrets: ["secrets/local-key"],
    });
    show(fake.api);
    await openEdit("Local model");
    const panel = paste(null, KEY);
    expect(
      await within(panel).findByText(
        "A shared key named local-key already exists. Choose another name, or pick local-key from the list.",
      ),
    ).toBeVisible();
    expect(fake.storedValue("local-key")).toBe("fixture value");
    // Nothing was stored: the Secret field still holds the route's own key.
    expect(
      within(dialog()).getByLabelText("Shared key", { exact: true }),
    ).toHaveValue("local");
    fireEvent.change(within(panel).getByLabelText("Secret name"), {
      target: { value: "local-key-2" },
    });
    fireEvent.click(within(panel).getByRole("button", { name: "Store key" }));
    expect(
      await within(dialog()).findByText(
        "Stored the key as local-key-2. Save the route to use it.",
      ),
    ).toBeVisible();
    // The create is never a replace: no write names a version.
    expect(fake.secretWrites.map((write) => write.name)).toEqual([
      "local-key",
      "local-key-2",
    ]);
  });

  it("refuses a reserved name without sending anything", async () => {
    const fake = fakeRoutes({ routes: [route("local")] });
    show(fake.api);
    await openEdit("Local model");
    const panel = paste("Connector-mine", KEY);
    expect(
      await within(panel).findByText(
        "Names starting with git- or connector- are reserved for people's own credentials.",
      ),
    ).toBeVisible();
    expect(fake.secretWrites).toEqual([]);
  });

  it("WEB-U-291: names the stored secret when the route save fails after it", async () => {
    const fake = fakeRoutes({ routes: [route("local")] });
    show(fake.api);
    await openEdit("Local model");
    paste(null, KEY);
    await within(dialog()).findByText(/^Stored the key as local-key/);
    fake.touch("local");
    // Display name is required before a save.
    fireEvent.change(within(dialog()).getByLabelText("Display name"), {
      target: { value: "Local" },
    });
    fireEvent.click(within(dialog()).getByRole("button", { name: "Save" }));
    expect(
      await within(dialog()).findByText(
        "The key is stored as local-key. It stays until you delete it under Keys › Shared.",
      ),
    ).toBeVisible();
    expect(fake.secretWrites).toHaveLength(1);
  });

  it("shows a route whose key was scrubbed, and opens Paste key for it", async () => {
    const fake = fakeRoutes({
      routes: [route("legacy", { key_ref: null, key_ref_invalid: true })],
    });
    show(fake.api);
    await screen.findByRole("table", { name: "Model routes" });
    const row = screen.getByRole("row", { name: /legacy/ });
    expect(within(row).getByText("Key needed")).toBeVisible();
    // The fix is an action, in the row's menu with the others (UI-GUIDELINES §7).
    expect(
      within(row).queryByRole("button", { name: "Paste key for Legacy model" }),
    ).toBeNull();
    fireEvent.click(
      within(row).getByRole("button", {
        name: "More actions for route Legacy model",
      }),
    );
    fireEvent.click(
      screen.getByRole("menuitem", { name: "Paste key for Legacy model" }),
    );
    const panel = within(page()).getByRole("group", { name: "Paste a key" });
    await waitFor(() =>
      expect(
        within(panel).getByLabelText("Key", { selector: "input" }),
      ).toHaveFocus(),
    );
    expect(within(panel).getByLabelText("Secret name")).toHaveValue(
      "legacy-key",
    );
  });
});

// Adding a route takes its key inline, and stores it as a shared key when the route is saved.
describe("WEB-U-056 a new route's key, typed in the form", () => {
  const start = async (fake: ReturnType<typeof fakeRoutes>) => {
    const container = show(fake.api);
    await screen.findByRole("table", { name: "Model routes" });
    fireEvent.click(await screen.findByRole("link", { name: "Add route" }));
    await screen.findByRole("group", { name: "Save or cancel" });
    const form = page();
    const set = (label: string, value: string) =>
      // Fields only: the Key section is named "Key" too (WEB-U-115).
      fireEvent.change(
        within(form).getByLabelText(label, {
          exact: true,
          selector: "input, select, textarea",
        }),
        { target: { value } },
      );
    set("Display name", "Qwen Coder");
    set("Address", "https://models.example.invalid/v1");
    set("Model", "qwen2.5-coder");
    return { container, form, set };
  };

  it("stores the typed key as a shared key named after the route, then saves the route with it", async () => {
    const fake = fakeRoutes({ routes: [route("local")] });
    const { container, form, set } = await start(fake);
    expect(
      within(form).getByLabelText("Key", { selector: "input" }),
    ).toHaveAttribute("type", "password");
    expect(
      within(form).getByText(
        "Stored as the shared key qwen-coder-key when you save.",
      ),
    ).toBeVisible();
    set("Key", KEY);
    fireEvent.click(within(form).getByRole("button", { name: "Save" }));
    expect(await screen.findByText(/^Saved route Qwen Coder\./)).toBeVisible();
    expect(fake.secretWrites).toEqual([{ name: "qwen-coder-key", value: KEY }]);
    expect(fake.writes.at(-1)).toMatchObject({
      name: "qwen-coder",
      fields: { key_ref: "secrets/qwen-coder-key" },
    });
    expect(container.innerHTML).not.toContain(KEY);
    expect(JSON.stringify(fake.writes)).not.toContain(KEY);
  });

  it("numbers the name when it is taken, never overwriting a shared key", async () => {
    const fake = fakeRoutes({
      routes: [route("local")],
      secrets: ["secrets/qwen-coder-key"],
    });
    const { form, set } = await start(fake);
    set("Key", KEY);
    fireEvent.click(within(form).getByRole("button", { name: "Save" }));
    expect(await screen.findByText(/^Saved route Qwen Coder\./)).toBeVisible();
    expect(fake.storedValue("qwen-coder-key")).toBe("fixture value");
    expect(fake.storedValue("qwen-coder-key-2")).toBe(KEY);
    expect(fake.writes.at(-1)?.fields?.key_ref).toBe(
      "secrets/qwen-coder-key-2",
    );
  });

  it("stores nothing while the route has a problem of its own", async () => {
    const fake = fakeRoutes({ routes: [route("local")] });
    const { form, set } = await start(fake);
    set("Model", "");
    set("Key", KEY);
    fireEvent.click(within(form).getByRole("button", { name: "Save" }));
    expect(
      await within(form).findByText("Enter the model name the server expects."),
    ).toBeVisible();
    expect(fake.secretWrites).toEqual([]);
  });

  it("uses an existing shared key instead, on request", async () => {
    const fake = fakeRoutes({
      routes: [route("local")],
      secrets: ["secrets/local"],
    });
    const { form, set } = await start(fake);
    fireEvent.click(
      within(form).getByRole("button", { name: "Use an existing shared key" }),
    );
    expect(
      within(form).queryByLabelText("Key", { selector: "input" }),
    ).toBeNull();
    set("Shared key", "local");
    fireEvent.click(within(form).getByRole("button", { name: "Save" }));
    expect(await screen.findByText(/^Saved route Qwen Coder\./)).toBeVisible();
    expect(fake.secretWrites).toEqual([]);
    expect(fake.writes.at(-1)?.fields?.key_ref).toBe("secrets/local");
  });

  // WEB-U-294: only a listed key's name becomes its reference. A key pasted here by mistake, even one that looks like a
  // name (letters, digits, - and _), stays as typed and is refused: it is never saved as the route's key reference.
  /** A made-up key's shape, put together here so the source holds nothing key-shaped for the secrets scan (G-10). */
  const PASTED = ["sk", "proj", "AbC_123"].join("-");
  it("WEB-U-294: refuses a pasted key in Shared key and writes nothing", async () => {
    const fake = fakeRoutes({
      routes: [route("local")],
      secrets: ["secrets/local"],
    });
    const { form, set } = await start(fake);
    fireEvent.click(
      within(form).getByRole("button", { name: "Use an existing shared key" }),
    );
    set("Shared key", PASTED);
    fireEvent.click(within(form).getByRole("button", { name: "Save" }));
    expect(
      await within(form).findByText(
        "Enter the name of a shared key, or use Paste key to store one.",
      ),
    ).toBeVisible();
    expect(fake.writes).toEqual([]);
    expect(fake.secretWrites).toEqual([]);
    // TEST: WEB-U-295
    // WEB-U-295: written as a reference, only a listed key is one: secrets/ plus a pasted key, or a key not stored
    // yet, is refused the same way; a listed key's reference is saved.
    for (const typed of [`secrets/${PASTED}`, "secrets/later-key"]) {
      set("Shared key", typed);
      fireEvent.click(within(form).getByRole("button", { name: "Save" }));
      expect(
        await within(form).findByText(
          "Enter the name of a shared key, or use Paste key to store one.",
        ),
      ).toBeVisible();
      expect(fake.writes).toEqual([]);
    }
    set("Shared key", "secrets/local");
    fireEvent.click(within(form).getByRole("button", { name: "Save" }));
    expect(await screen.findByText(/^Saved route Qwen Coder\./)).toBeVisible();
    expect(fake.writes.at(-1)?.fields?.key_ref).toBe("secrets/local");
    // No request ever carried the pasted key.
    expect(JSON.stringify([fake.writes, fake.secretWrites])).not.toContain(
      "AbC_123",
    );
  });
});

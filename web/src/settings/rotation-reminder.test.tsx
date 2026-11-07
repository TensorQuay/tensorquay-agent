import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import type { User } from "../protocol/generated/api";
import { fakeAdmin, secretItem } from "../test/admin";
import { choose as choosePicked, chosen } from "../test/picker";
import { LIMITS } from "../test/user";

import type { AdminApi } from "./admin/contract";
import { SecretsSettings } from "./SecretsSettings";

const admin: User = {
  id: "0195d8c0-0000-7000-8000-000000000001",
  email: "admin@example.invalid",
  role: "admin",
  internet: "none",
  limits: LIMITS,
  features: { agent_teams: false },
};
const listed = [
  // Replaced with the same value after its last change: Last changed stays at rotated_at.
  secretItem("openai", {
    rotated_at: "2026-06-01T09:00:00Z",
    updated_at: "2026-09-20T09:00:00Z",
    rotate_after_days: 90,
    rotation_due: true,
  }),
  secretItem("deepseek", {
    rotated_at: "2026-09-01T09:00:00Z",
    rotate_after_days: 180,
  }),
  secretItem("spare", { rotated_at: "2026-09-10T09:00:00Z" }),
];

function show(api: AdminApi) {
  const cache = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  render(
    <QueryClientProvider client={cache}>
      <SecretsSettings me={admin} api={api} />
    </QueryClientProvider>,
  );
}
const row = (name: string) =>
  screen.getByRole("row", { name: new RegExp(`^${name}(?![\\w-])`) });
/** Opens a shared key's "…" menu and chooses one of its items (UI-GUIDELINES §7). */
function menuItem(ref: string, item: string) {
  fireEvent.click(
    screen.getByRole("button", { name: `More actions for ${ref}` }),
  );
  fireEvent.click(screen.getByRole("menuitem", { name: item }));
}
async function openReminder(name: string) {
  await screen.findByRole("table", { name: "Shared keys" });
  menuItem(`shared key ${name}`, `Set the reminder for shared key ${name}`);
  return screen.getByRole("dialog", {
    name: `Rotation reminder for shared key ${name}`,
  });
}
/** Chooses in Remind me (the standard picker, WEB-U-134) by the words it shows. */
const choose = (dialog: HTMLElement, words: string) =>
  choosePicked("Remind me", words, dialog);
const save = (dialog: HTMLElement) =>
  fireEvent.click(
    within(dialog).getByRole("button", { name: "Save reminder" }),
  );

// ADR-0033: when each shared secret last changed, and a reminder to rotate it that never blocks
// anything (WEB-U-052).
describe("WEB-U-052 the rotation reminder on Keys › Shared", () => {
  it("shows when each value last changed, its reminder and a due badge", async () => {
    show(fakeAdmin({ secrets: listed }).api);
    const table = await screen.findByRole("table", { name: "Shared keys" });
    expect(
      within(table)
        .getAllByRole("columnheader")
        .map((header) => header.textContent),
    ).toEqual(["Name", "Used by", "Last changed", "Actions"]);
    const due = row("openai");
    expect(within(due).getByRole("time")).toHaveAttribute(
      "datetime",
      "2026-06-01T09:00:00Z",
    );
    expect(within(due).getByText("Reminder every 90 days")).toBeVisible();
    const badge = within(due).getByText("Rotation due");
    expect(badge).toBeVisible();
    expect(badge).toHaveAccessibleDescription(
      /^Last changed .+\. The reminder is every 90 days\.$/,
    );
    expect(
      within(row("deepseek")).getByText("Reminder every 180 days"),
    ).toBeVisible();
    expect(within(row("deepseek")).queryByText("Rotation due")).toBeNull();
    expect(within(row("spare")).getByText("No reminder")).toBeVisible();
    // A due reminder blocks nothing.
    expect(
      screen.getByRole("button", { name: "Replace shared key openai" }),
    ).toBeEnabled();
    fireEvent.click(
      screen.getByRole("button", {
        name: "More actions for shared key openai",
      }),
    );
    expect(
      screen.getByRole("menuitem", { name: "Delete shared key openai" }),
    ).not.toHaveAttribute("aria-disabled");
  });

  it("sets a preset with the version read, and clears it with No reminder", async () => {
    const fake = fakeAdmin({ secrets: listed });
    show(fake.api);
    let dialog = await openReminder("spare");
    expect(chosen("Remind me", dialog)).toBe("No reminder");
    choose(dialog, "Reminder every 90 days");
    save(dialog);
    expect(
      await screen.findByText("Saved the reminder for shared key spare."),
    ).toBeVisible();
    expect(
      await within(row("spare")).findByText("Reminder every 90 days"),
    ).toBeVisible();
    dialog = await openReminder("spare");
    expect(chosen("Remind me", dialog)).toBe("Reminder every 90 days");
    choose(dialog, "No reminder");
    save(dialog);
    expect(await within(row("spare")).findByText("No reminder")).toBeVisible();
    expect(fake.reminders).toEqual([
      { name: "spare", rotate_after_days: 90, version: 1 },
      { name: "spare", rotate_after_days: null, version: 2 },
    ]);
  });

  it("takes other whole days from 1 to 3650, refusing the rest before sending", async () => {
    const fake = fakeAdmin({ secrets: listed });
    show(fake.api);
    const dialog = await openReminder("deepseek");
    choose(dialog, "Other");
    const days = within(dialog).getByLabelText("Days");
    for (const value of ["", "0", "3651", "1.5", "ten"]) {
      fireEvent.change(days, { target: { value } });
      save(dialog);
      expect(
        await within(dialog).findByText(
          "Enter a whole number of days from 1 to 3650.",
        ),
      ).toBeVisible();
    }
    expect(fake.reminders).toEqual([]);
    fireEvent.change(days, { target: { value: "45" } });
    save(dialog);
    expect(
      await within(row("deepseek")).findByText("Reminder every 45 days"),
    ).toBeVisible();
    expect(fake.reminders).toEqual([
      { name: "deepseek", rotate_after_days: 45, version: 1 },
    ]);
  });

  it("keeps the choice after another admin's change, and saves it with the new version", async () => {
    const fake = fakeAdmin({ secrets: listed });
    show(fake.api);
    const dialog = await openReminder("deepseek");
    choose(dialog, "Reminder every 30 days");
    fake.touch("secrets/deepseek");
    save(dialog);
    expect(
      await within(dialog).findByText(
        "Someone changed this key since you opened it. Your choice is kept: save again to replace their change.",
      ),
    ).toBeVisible();
    expect(chosen("Remind me", dialog)).toBe("Reminder every 30 days");
    save(dialog);
    expect(
      await within(row("deepseek")).findByText("Reminder every 30 days"),
    ).toBeVisible();
    expect(fake.reminders.map((write) => write.version)).toEqual([1, 2]);
  });

  it("reads the gateway's refusal of the days at the field", async () => {
    const fake = fakeAdmin({ secrets: listed, refuseReminder: true });
    show(fake.api);
    const dialog = await openReminder("spare");
    choose(dialog, "Reminder every 365 days");
    save(dialog);
    expect(
      await within(dialog).findByText(
        "Enter a whole number of days from 1 to 3650.",
      ),
    ).toBeVisible();
  });
});

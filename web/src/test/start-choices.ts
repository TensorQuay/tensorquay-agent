import { fireEvent, screen, waitFor } from "@testing-library/react";
import { expect } from "vitest";

import { settleLazy } from "./lazy";

/**
 * New task's model and environment, in its top bar (WEB-U-113: no Change panel); its workspace and profile come from
 * the composer's "+" (see addFromPlus).
 */
export async function openChoices() {
  return openTaskChoices();
}

/** Adds a workspace or an agent profile from New task's "+" and returns the chip's picker, opened. */
export async function addFromPlus(item: "Workspace…" | "Agent profile…") {
  fireEvent.click(await screen.findByRole("button", { name: "Add" }));
  // The profiles may still be arriving: the menu gains the item once they do.
  fireEvent.click(await screen.findByRole("menuitem", { name: item }));
  return screen.findByRole("button", {
    name: item === "Workspace…" ? /^Workspace:/ : /^Agent:/,
  });
}

/**
 * Finds a task's model and environment in its header (WEB-U-113: two small pickers right of the title and status; no ⚙
 * or panel) and returns the group, once both pickers are there.
 */
export async function openTaskChoices() {
  const group = await screen.findByRole("group", {
    name: "Model and environment",
  });
  await settleLazy();
  await waitFor(() =>
    expect(group.querySelectorAll(".choice-row")).toHaveLength(2),
  );
  return group;
}

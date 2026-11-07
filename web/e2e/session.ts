import { randomUUID } from "node:crypto";

import { expect, type Page } from "@playwright/test";

/** Establishes an isolated fake session through the same public API as the browser form. */
export async function signIn(page: Page) {
  const response = await page.request.post("/api/v1/auth/login", {
    data: {
      email: "member@example.invalid",
      password: "synthetic-browser-password",
    },
  });
  expect(response.status()).toBe(200);
}

/** Creates metadata through the public contract and checks its identifier before building a path. */
export async function createConversation(page: Page): Promise<string> {
  const response = await page.request.post("/api/v1/conversations", {
    headers: { "Idempotency-Key": randomUUID() },
    data: { title: "Workspace review", model_route: "local" },
  });
  expect(response.status()).toBe(201);
  const value: unknown = await response.json();
  if (
    typeof value !== "object" ||
    value === null ||
    !("id" in value) ||
    typeof value.id !== "string"
  )
    throw new Error("Missing conversation identifier");
  return `/c/${value.id}`;
}

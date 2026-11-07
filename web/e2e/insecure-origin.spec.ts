import { networkInterfaces } from "node:os";

import { signIn } from "./session";
import { expect, test } from "./test";

const address = Object.values(networkInterfaces())
  .flat()
  .find((entry) => entry?.family === "IPv4" && !entry.internal)?.address;
if (!address)
  throw new Error(
    "The insecure-origin test requires a non-loopback IPv4 interface.",
  );
test.use({ baseURL: `http://${address}:3100` });

test("WEB-E2E-004 sends and streams over HTTP outside a secure context", async ({
  page,
}) => {
  await page.goto("/login");
  expect(await page.evaluate(() => window.isSecureContext)).toBe(false);
  expect(await page.evaluate(() => typeof crypto.randomUUID)).toBe("undefined");
  expect(await page.evaluate(() => typeof crypto.getRandomValues)).toBe(
    "function",
  );
  await signIn(page);
  await page.goto("/");
  // What the task will use is in the composer: its model picker (WEB-U-102).
  await expect(
    page.getByRole("button", { name: "Model: Local model" }),
  ).toBeVisible();
  const creation = page.waitForResponse(
    (response) =>
      response.request().method() === "POST" &&
      new URL(response.url()).pathname === "/api/v1/conversations",
  );
  const input = page.getByRole("textbox", { name: "Message", exact: true });
  await input.fill("Summarize the workspace.");
  const [response] = await Promise.all([
    page.waitForResponse(
      (response) =>
        response.request().method() === "POST" &&
        new URL(response.url()).pathname.endsWith("/messages"),
      { timeout: 5000 },
    ),
    page.getByRole("button", { name: "Send" }).click(),
  ]);
  expect(response.status()).toBe(202);
  const created = await creation;
  expect(created.status()).toBe(201);
  expect(created.request().headers()["idempotency-key"]).toMatch(
    /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/,
  );
  expect(created.request().headers()["idempotency-key"]).not.toBe(
    response.request().headers()["idempotency-key"],
  );
  expect(response.request().headers()["idempotency-key"]).toMatch(
    /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/,
  );
  await expect(
    page.getByText("I’ll check the project files.", { exact: true }),
  ).toBeVisible();
  await expect(page.getByLabel("Run status")).toHaveText("Done");
  await expect(input).toHaveValue("");
});

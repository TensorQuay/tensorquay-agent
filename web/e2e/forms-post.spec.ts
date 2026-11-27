import { expect, test } from "./test";

// SEC-070 (18.2.1; the lead's check of 27 Nov 2026): a sign-in or join form submitted before the page's script has
// loaded never puts the password in the address. Without a method the browser sent `/login?email=…&password=…`.
test.use({ javaScriptEnabled: false });

test("SEC-070 a sign-in sent before the page's script keeps the password out of the address", async ({
  page,
}) => {
  await page.goto("/login");
  await page
    .getByLabel("Email", { exact: true })
    .fill("nobody@example.invalid");
  await page
    .getByLabel("Password", { exact: true })
    .fill("not-a-real-password-123");
  const sent = page.waitForRequest(
    (request) =>
      (request.url().includes("/login") && request.method() !== "GET") ||
      request.url().includes("password="),
  );
  await page.getByRole("button", { name: "Sign in", exact: true }).click();
  const request = await sent;
  expect(request.url()).not.toContain("password");
  expect(request.url()).not.toContain("not-a-real-password-123");
  expect(request.method()).toBe("POST");
});

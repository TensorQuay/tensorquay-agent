import { expect, test } from "./test";

// SEC-070 (18.2.1; the lead's check of 27 Nov 2026): a sign-in or join form submitted before the page's script has
// loaded never puts the password in the address. Without a method the browser sent `/login?email=…&password=…`.
test.use({ javaScriptEnabled: false });

test("SEC-070 a sign-in before the page's script is never sent, so the password never reaches the address", async ({
  page,
}) => {
  // 18.3.0 (WEB-U-548): until the page is live its submit button is disabled, so nothing is sent before the script:
  // neither a click nor Enter in a field. Before 18.2.1 the browser sent `/login?email=…&password=…`; in 18.2.1 a POST
  // that signed no one in (the nightly real-stack run of 27 Nov 2026).
  const sent: string[] = [];
  page.on("request", (request) => {
    if (
      request.url().includes("password=") ||
      (request.url().includes("/login") && request.method() !== "GET")
    )
      sent.push(`${request.method()} ${request.url()}`);
  });
  await page.goto("/login");
  await page
    .getByLabel("Email", { exact: true })
    .fill("nobody@example.invalid");
  const password = page.getByLabel("Password", { exact: true });
  await password.fill("not-a-real-password-123");
  const submit = page.getByRole("button", { name: "Sign in", exact: true });
  await expect(submit).toBeDisabled();
  await password.press("Enter");
  await page.waitForTimeout(1500);
  expect(sent).toEqual([]);
  expect(page.url()).not.toContain("password");
});

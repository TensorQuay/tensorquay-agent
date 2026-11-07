import { createConversation, signIn } from "./session";
import { expect, test } from "./test";

// WEB-U-008: the precompiled validators run no generated code, so the page works under a Content-Security-Policy
// without 'unsafe-eval'. Next.js still needs its inline scripts, so the policy allows those; eval is what is tested.
const POLICY =
  "default-src 'self'; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'; img-src 'self' data: blob:; font-src 'self' data:; connect-src 'self'";

test("WEB-U-008 a task loads and streams under a policy without 'unsafe-eval'", async ({
  page,
}) => {
  await page.route("**/*", async (route) => {
    if (route.request().resourceType() !== "document") return route.continue();
    const response = await route.fetch();
    await route.fulfill({
      response,
      headers: {
        ...response.headers(),
        "content-security-policy": POLICY,
      },
    });
  });
  const violations: string[] = [];
  await page.exposeFunction("recordViolation", (text: string) => {
    violations.push(text);
  });
  await page.addInitScript(() => {
    document.addEventListener("securitypolicyviolation", (event) => {
      void (
        window as unknown as { recordViolation: (text: string) => void }
      ).recordViolation(`${event.violatedDirective} ${event.blockedURI}`);
    });
  });
  await signIn(page);
  await page.goto(await createConversation(page));
  const input = page.getByRole("textbox", { name: "Message", exact: true });
  await input.fill("Summarize the workspace.");
  await input.press("Enter");
  // A whole run, not one assertion's 5 s: under load the fake's run took longer and was still working.
  await expect(page.getByLabel("Run status")).toHaveText(/^Done/, {
    timeout: 30_000,
  });
  await expect(
    page.getByText("The workspace contains", { exact: false }),
  ).toBeVisible();
  expect(violations).toEqual([]);
});

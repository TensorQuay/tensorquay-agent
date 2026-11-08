import { WRAPPED } from "./front/stream.mjs";
import { createConversation, signIn } from "./session";
import { expect, test } from "./test";

// WEB-U-135 on the page: a final answer whose closing paragraph a model wrapped in <result>…</result> shows the
// inner text; a tag quoted in a code block stays as written.
test("WEB-U-135 a wrapped final answer shows without its tag", async ({
  page,
}) => {
  await signIn(page);
  await page.goto(await createConversation(page));
  const input = page.getByLabel("Message", { exact: true });
  await input.fill(WRAPPED);
  await input.press("Enter");
  await expect(page.getByLabel("Run status")).toHaveText(/^Done/);
  const answer = page.locator(".markdown-answer").last();
  await expect(
    answer.getByText("The workspace holds a Rust project.", { exact: true }),
  ).toBeVisible();
  await expect(answer).not.toContainText("<result>");
  await expect(
    answer.locator("code").filter({ hasText: "<note>draft</note>" }),
  ).toBeVisible();
});

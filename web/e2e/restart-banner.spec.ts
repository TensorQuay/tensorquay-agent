import { THINK } from "./front/stream.mjs";
import { createConversation, signIn } from "./session";
import { expect, test } from "./test";

// WEB-U-234 on the page (the 14b page pass, E4): a gateway restart under an open task is a reconnect. The page says
// "Reconnecting to the task…" while it is away, and no red "something went wrong" shows, before or after it is back.
test("WEB-U-234 a gateway restart is a quiet reconnect, never a red banner", async ({
  page,
}) => {
  await signIn(page);
  const path = await createConversation(page);
  await page.goto(path);
  const task = page.getByRole("region", { name: "Task", exact: true });
  await expect(
    task.getByRole("textbox", { name: "Message", exact: true }),
  ).toBeVisible();
  const reopened = page.waitForRequest(
    (request) =>
      request.url().includes("/events") && request.url().includes("stream=1"),
  );
  const id = path.split("/").pop() ?? "";
  const restarted = await page.request.post(
    `/__fake/restart?conversation=${encodeURIComponent(id)}`,
  );
  expect(restarted.status()).toBe(204);
  await reopened;
  await expect(page.getByText("Reconnecting to the task…")).toHaveCount(0);
  await expect(page.getByText(/went wrong/i)).toHaveCount(0);
  await expect(task.getByRole("alert")).toHaveCount(0);
});

// WEB-U-234 and WEB-U-016, first-hour F7: a restart under a thinking step loses the model call, and the runtime asks
// again once its link is back. The step's line says the step starts over, never a bare "Reconnected", and no red banner.
test("WEB-U-234 a restart under a step says the model is asked again and the step starts over", async ({
  page,
}) => {
  await signIn(page);
  const path = await createConversation(page);
  await page.goto(path);
  const input = page.getByLabel("Message", { exact: true });
  await input.fill(THINK);
  await input.press("Enter");
  const group = page.getByRole("group", { name: "Activity" });
  await expect(
    group.getByRole("button", { name: /^Thinking… · \d+ s$/ }),
  ).toBeVisible();
  const reopened = page.waitForRequest(
    (request) =>
      request.url().includes("/events") && request.url().includes("stream=1"),
  );
  const id = path.split("/").pop() ?? "";
  const restarted = await page.request.post(
    `/__fake/restart?conversation=${encodeURIComponent(id)}&gateway_link_ms=12400`,
  );
  expect(restarted.status()).toBe(204);
  await reopened;
  await expect(
    page.getByText(
      "The connection to the server dropped for 12 s, so the model was asked again and this step started over.",
    ),
  ).toBeVisible();
  await expect(page.getByText(/^Reconnected/)).toHaveCount(0);
  await expect(page.getByText(/went wrong/i)).toHaveCount(0);
  await expect(
    page.getByRole("region", { name: "Task", exact: true }).getByRole("alert"),
  ).toHaveCount(0);
});

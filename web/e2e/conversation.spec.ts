import { mkdir } from "node:fs/promises";
import { join } from "node:path";

import AxeBuilder from "@axe-core/playwright";

import { openCard } from "./cards";
import { chooseModel } from "./model";
import { createConversation, signIn } from "./session";
import { expect, test } from "./test";

test("WEB-U-430 composer grows from two to eight lines and keeps Shift+Enter as a newline", async ({
  page,
}) => {
  await signIn(page);
  await page.goto(await createConversation(page));
  const input = page.getByRole("textbox", { name: "Message", exact: true });
  const dimensions = () =>
    input.evaluate((element) => {
      const style = getComputedStyle(element);
      return {
        height: element.getBoundingClientRect().height,
        line: Number.parseFloat(style.lineHeight),
        padding:
          Number.parseFloat(style.paddingTop) +
          Number.parseFloat(style.paddingBottom) +
          Number.parseFloat(style.borderTopWidth) +
          Number.parseFloat(style.borderBottomWidth),
        resize: style.resize,
      };
    });
  const initial = await dimensions();
  expect(initial.height).toBeCloseTo(initial.line * 2 + initial.padding, 0);
  // The card: 12 px by 16 px of padding (UI-GUIDELINES §5b's table), so empty it is two 24 px lines, the 12 px gap, the
  // 36 px bar, the padding and its border: 122 px.
  const card = await input.evaluate((element) => {
    const box = element.closest(".composer-card");
    if (!box) throw new Error("The composer has no card");
    const style = getComputedStyle(box);
    return {
      height: Math.round(box.getBoundingClientRect().height),
      padding: [
        style.paddingTop,
        style.paddingRight,
        style.paddingBottom,
        style.paddingLeft,
      ],
    };
  });
  expect(card).toEqual({
    height: 122,
    padding: ["12px", "16px", "12px", "16px"],
  });
  expect(initial.resize).toBe("none");
  await input.fill("First line");
  await input.press("Shift+Enter");
  await expect(input).toHaveValue("First line\n");
  await expect(page.getByLabel("Run status")).toHaveText("Ready");
  await input.fill(
    Array.from({ length: 24 }, (_, index) => `Line ${index}`).join("\n"),
  );
  const grown = await dimensions();
  expect(grown.height).toBeCloseTo(grown.line * 8 + grown.padding, 0);
  await input.fill("");
  expect((await dimensions()).height).toBeCloseTo(initial.height, 0);
});

// WEB-U-431: a person with no tasks yet finds the keyboard hint in New task's placeholder; their first task ends it.
test("WEB-U-431 the keyboard hint is New task's placeholder until the person has a task", async ({
  page,
}) => {
  await signIn(page);
  await page.goto("/");
  const input = page.getByRole("textbox", { name: "Message", exact: true });
  await expect(input).toHaveAttribute(
    "placeholder",
    "Describe the task…\nEnter to send · Shift+Enter for a new line",
  );
  const path = await createConversation(page);
  await page.goto("/");
  await expect(input).toHaveAttribute("placeholder", "Describe the task…");
  await page.goto(path);
  await expect(input).toHaveAttribute("placeholder", "Reply…");
});

test("WEB-E2E-004 precursor: sign in, create, stream a tool call, replay, delete and sign out", async ({
  page,
}) => {
  await page.goto("/login");
  await page
    .getByLabel("Email", { exact: true })
    .fill("member@example.invalid");
  await page
    .getByLabel("Password", { exact: true })
    .fill("synthetic-browser-password");
  await page.getByRole("button", { name: "Sign in", exact: true }).click();
  await expect(page.getByText("Your tasks will appear here.")).toBeVisible();
  await page.getByRole("link", { name: "New task" }).click();
  await expect(page.getByRole("dialog")).toHaveCount(0);
  await chooseModel(page, "local");
  await page
    .getByRole("textbox", { name: "Message", exact: true })
    .fill("Summarize the workspace, then wait.");
  await page.keyboard.press("Enter");
  await expect(page).toHaveURL(/\/c\//);
  // The fake run pauses after its narration, so the live state is checked before the run can end and fold it.
  await expect(page.getByLabel("Run status")).toHaveText(/^Working…/);
  await expect(page.getByText("I’ll check the project files.")).toBeVisible();
  await expect(page.getByRole("button", { name: /Working/ })).toHaveAttribute(
    "aria-expanded",
    "true",
  );
  const task = new URL(page.url()).pathname.split("/").at(-1) ?? "";
  expect(
    (
      await page.request.post(`/api/v1/conversations/${task}/fixture-resume`)
    ).status(),
  ).toBe(204);
  await expect(page.getByLabel("Run status")).toHaveText("Done");
  // How long the run took waits in the status's tooltip.
  await expect(page.getByLabel("Run status")).toHaveAttribute(
    "title",
    /^Took [\d\u202f]+ s$/,
  );
  await expect(
    page.getByRole("button", { name: /Worked for/ }),
  ).toHaveAttribute("aria-expanded", "false");
  const card = page.getByRole("article", { name: "shell tool call" });
  await expect(page.locator(".markdown-answer")).toContainText(
    "The workspace contains a Rust project:",
  );
  const transcript = await page.getByTestId("transcript").innerText();
  await page.reload();
  await expect(page.getByLabel("Run status")).toHaveText("Done");
  await expect
    .poll(() => page.getByTestId("transcript").innerText())
    .toBe(transcript);
  await page.getByRole("button", { name: /Worked for/ }).press("Enter");
  await openCard(page.getByRole("button", { name: "Ran a command: ls" }));
  // The step's line names the command; the card keeps the call's arguments behind Details.
  await card.getByRole("button", { name: "Details", exact: true }).click();
  await expect(card.getByLabel("All arguments")).toContainText(
    '"command": "ls"',
  );
  await expect(card.getByLabel("Result")).toContainText("Cargo.toml");
  await page
    .getByRole("link", { name: "New task", exact: true })
    .last()
    .click();
  // The sidebar row's ⋯ (the task header has one of the same name).
  await page
    .getByRole("region", { name: "Tasks" })
    .first()
    .getByRole("button", {
      name: "More actions for Summarize the workspace, then wait.",
    })
    .click();
  await page.getByRole("menuitem", { name: "Delete task" }).click();
  await page
    .getByRole("dialog", { name: "Delete task" })
    .getByRole("button", { name: "Delete task", exact: true })
    .click();
  await expect(page.getByText("Your tasks will appear here.")).toBeVisible();
  await page.getByRole("button", { name: "Account", exact: true }).click();
  await page.getByRole("menuitem", { name: "Sign out" }).click();
  await expect(page).toHaveURL(/\/login$/);
});

test("WEB-E2E-004 precursor: Stop cancels the active run and gives Send back", async ({
  page,
}) => {
  await signIn(page);
  await page.goto(await createConversation(page));
  await page
    .getByLabel("Message", { exact: true })
    .fill("Wait for cancellation");
  await page.getByRole("button", { name: "Send" }).click();
  await expect(page.getByLabel("Run status")).toHaveText(/^Working…/);
  // WEB-U-079: the box stays usable while the agent works; Stop takes Send's place.
  await expect(page.getByLabel("Message", { exact: true })).toBeEnabled();
  await expect(
    page.getByRole("button", { name: "Send", exact: true }),
  ).toHaveCount(0);
  // Stop acts at once (c9): the work so far is kept, so there is nothing to confirm.
  await page.getByRole("button", { name: "Stop", exact: true }).click();
  await expect(page.getByLabel("Run status")).toHaveText("Stopped");
  await page.getByLabel("Message", { exact: true }).fill("Try again");
  await expect(
    page.getByRole("button", { name: "Send", exact: true }),
  ).toBeEnabled();
});

for (const theme of ["light", "dark"] as const) {
  for (const path of ["/setup", "/login", "/"]) {
    test(`WEB-U-007 authenticated page accessibility ${theme} ${path}`, async ({
      page,
    }) => {
      await page.emulateMedia({ colorScheme: theme });
      await signIn(page);
      await createConversation(page);
      await page.goto(path);
      await page.evaluate(() => document.fonts.ready);
      if (path === "/")
        await expect(
          page.getByRole("link", { name: /Workspace review/ }),
        ).toBeVisible();
      expect((await new AxeBuilder({ page }).analyze()).violations).toEqual([]);
    });
  }

  test(`WEB-U-007 empty and finished transcript accessibility ${theme}`, async ({
    page,
  }) => {
    await page.emulateMedia({ colorScheme: theme });
    await signIn(page);
    await page.goto(await createConversation(page));
    await page.evaluate(() => document.fonts.ready);
    await expect(page.getByLabel("Message", { exact: true })).toBeVisible();
    expect((await new AxeBuilder({ page }).analyze()).violations).toEqual([]);
    await page
      .getByLabel("Message", { exact: true })
      .fill("Summarize the workspace.");
    await page.keyboard.press("Enter");
    await expect(page.getByLabel("Run status")).toHaveText("Done");
    await page.getByRole("button", { name: /Worked for/ }).click();
    await openCard(page.getByRole("button", { name: "Ran a command: ls" }));
    expect((await new AxeBuilder({ page }).analyze()).violations).toEqual([]);
    if (process.env.TQA_WEB_EVIDENCE_DIR) {
      await mkdir(process.env.TQA_WEB_EVIDENCE_DIR, { recursive: true });
      await page.screenshot({
        path: join(process.env.TQA_WEB_EVIDENCE_DIR, `transcript-${theme}.png`),
        fullPage: true,
      });
    }
    for (const width of [360, 768, 1024, 1440]) {
      await page.setViewportSize({ width, height: 1000 });
      expect(
        await page.evaluate(
          () => document.documentElement.scrollWidth <= window.innerWidth,
        ),
      ).toBe(true);
    }
  });
}

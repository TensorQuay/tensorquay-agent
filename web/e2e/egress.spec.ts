import { mkdir } from "node:fs/promises";
import { networkInterfaces } from "node:os";
import { join } from "node:path";

import AxeBuilder from "@axe-core/playwright";

import { createConversation, signIn } from "./session";
import { expect, test, type Page } from "./test";

const address = Object.values(networkInterfaces())
  .flat()
  .find((entry) => entry?.family === "IPv4" && !entry.internal)?.address;
if (!address)
  throw new Error("The egress contract needs a non-loopback interface.");
test.use({ baseURL: `http://${address}:3100` });

const blocked =
  "Blocked network access to example.invalid:443: the network allow-list doesn't allow it. An administrator can change the list under Settings › Administration › Internet access.";

async function capture(page: Page, name: string) {
  const directory = process.env.TQA_WEB_EVIDENCE_DIR;
  if (!directory) return;
  // Evidence shows rendered content, never a loading skeleton.
  await expect(page.locator(".skeleton")).toHaveCount(0);
  await mkdir(directory, { recursive: true });
  await page.screenshot({ path: join(directory, `egress-${name}.png`) });
}

// SEC-001 (web side): a refusal is said once - by the denied fetch line inside the work group, live and on replay.
for (const width of [1024, 1440])
  for (const colorScheme of ["light", "dark"] as const)
    test(`SEC-001 blocked network access ${width} ${colorScheme}`, async ({
      page,
    }) => {
      const logged: string[] = [];
      page.on("console", (message) => {
        if (["error", "warning"].includes(message.type()))
          logged.push(message.text());
      });
      await page.setViewportSize({ width, height: 900 });
      await page.emulateMedia({ colorScheme });
      await signIn(page);
      await page.goto(await createConversation(page));
      const input = page.getByRole("textbox", { name: "Message" });
      await input.fill("Fetch a blocked page");
      await input.press("Enter");
      await expect(page.getByLabel("Run status")).toHaveText("Done");
      const main = page.getByRole("main");
      await expect(
        main.getByText(blocked, { exact: true }).filter({ visible: true }),
      ).toHaveCount(0);
      await expect(main.getByText(/something went wrong/i)).toHaveCount(0);
      await expect(main.getByText("Internal proxy diagnostic.")).toHaveCount(0);
      await page.getByRole("button", { name: /Worked for/ }).click();
      await expect(
        main.getByText(blocked, { exact: true }).filter({ visible: true }),
      ).toHaveCount(1);
      await capture(page, `${width}-${colorScheme}-live`);
      expect((await new AxeBuilder({ page }).analyze()).violations).toEqual([]);

      await page.reload();
      await expect(page.getByLabel("Run status")).toHaveText("Done");
      await page.getByRole("button", { name: /Worked for/ }).click();
      await expect(
        main.getByText(blocked, { exact: true }).filter({ visible: true }),
      ).toHaveCount(1);
      await capture(page, `${width}-${colorScheme}-replay`);
      expect(logged).toEqual([]);
    });

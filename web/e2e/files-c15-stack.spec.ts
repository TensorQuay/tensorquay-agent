import { randomUUID } from "node:crypto";

import { expect, test } from "@playwright/test";

import { openFilesPanel } from "./files-panel";
import { afterStart, budget } from "./stack-start";
import { bootstrap } from "./ux/flow";

// The feature matrix's gap in the Files panel before 17.0 (F7): what had a unit test and a fake-front spec
// but no real-stack test. On the files suite's stack, beside its own spec, whose result it leaves alone.
test.skip(
  process.env.TQA_STACK_SUITE !== "files",
  "Requires the files stack suite",
);

const RUN = randomUUID().slice(0, 8);
/**
 * 11 MiB that a file reader takes for a PNG, as a real one is binary from its first byte: the PNG signature, then NULs.
 * Not a run of one printable or control byte, which is UTF-8 without NUL: the gateway previews that as text, and its
 * escaped JSON (3 MiB for 0x07) passed the web's 2 MiB read limit before the image view was reached (the 15b rerun).
 */
const BIG_PNG = Buffer.concat([
  Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
  Buffer.alloc(11 * 1024 * 1024 - 8),
]);

// F7 (WEB-U-255): an image over the preview's limit (10 MiB), uploaded through the real gateway, says it is too large
// with its size and how to open it, and offers no Expand, which would have nothing to show.
test("WEB-E2E-056 a file too large to preview says so, with no Expand", async ({
  page,
  baseURL,
}) => {
  if (!baseURL) throw new Error("The driver must supply the stack origin");
  test.setTimeout(budget(2, 180_000));
  await bootstrap(page);
  const created = await page.request.post("/api/v1/conversations", {
    headers: { "Idempotency-Key": randomUUID(), Origin: baseURL },
    data: { title: `Too large ${RUN}`, model_route: "local" },
  });
  expect(created.status()).toBe(201);
  const { id } = (await created.json()) as { id: string };
  // 11 MiB: under the upload limit (25 MB), over the image preview's 10 MiB.
  const uploaded = await page.request.post(
    `/api/v1/conversations/${id}/uploads?name=big.png`,
    {
      headers: {
        "Idempotency-Key": randomUUID(),
        Origin: baseURL,
        "content-type": "application/octet-stream",
      },
      data: BIG_PNG,
      timeout: afterStart(60_000),
    },
  );
  expect(uploaded.ok(), `upload: ${uploaded.status()}`).toBe(true);
  await page.goto(`/c/${id}`);
  const panel = await openFilesPanel(page);
  await panel.getByRole("button", { name: /^uploads/ }).click({
    timeout: afterStart(30_000),
  });
  await panel.getByRole("button", { name: /^big\.png/ }).click();
  await expect(
    panel.getByText(/^Too large to preview \(.+\)\. Download it to open it\.$/),
  ).toBeVisible();
  await expect(panel.getByRole("button", { name: "Expand" })).toHaveCount(0);
});

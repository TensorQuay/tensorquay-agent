import { readFileSync } from "node:fs";

import { openFilesPanel } from "./files-panel";
import { createConversation, signIn } from "./session";
import { expect, test, type Page } from "./test";

// WEB-E2E-030 (the app's Content-Security-Policy, the CSP design): every page carries the app's own policy, with a
// fresh nonce per load that every script on the page carries; and the app runs under it with no violation, across
// the pages the fake front serves, a task that runs, the agent's web page in its preview frame, and a PDF drawn by
// pdf.js with its worker. Unlike WEB-U-008 and WEB-U-091, no policy is injected: this is the one the app sends.

/** The pages a person reaches from the sidebar and Settings; each loads its own data from the fake front. */
const PAGES = [
  "/",
  "/workspaces",
  "/channels",
  "/settings",
  "/settings/account",
  "/settings/tokens",
  "/settings/keys",
  "/settings/keys/shared",
  "/settings/git",
  "/settings/connector-credentials",
  "/settings/models",
  "/settings/people",
  "/settings/profiles",
  "/settings/agent-profiles",
  "/settings/environments",
  "/settings/connectors",
  "/settings/decision-engines",
  "/settings/internet-access",
  "/settings/limits",
  "/settings/usage",
  "/settings/status",
];

/** Every violation in the page or any frame: the event, and the console's report of it. */
async function watch(page: Page) {
  const violations: string[] = [];
  await page.exposeFunction("recordViolation", (text: string) => {
    violations.push(text);
  });
  await page.addInitScript(() => {
    document.addEventListener("securitypolicyviolation", (event) => {
      void (
        window as unknown as { recordViolation?: (text: string) => void }
      ).recordViolation?.(
        `${event.violatedDirective} ${event.blockedURI} ${event.sourceFile}:${event.lineNumber}`,
      );
    });
  });
  page.on("console", (message) => {
    if (/Content[- ]Security[- ]Policy/i.test(message.text()))
      violations.push(`console: ${message.text()}`);
  });
  return violations;
}

function directives(policy: string) {
  return new Map(
    policy
      .split(";")
      .map((part) => part.trim().split(/\s+/))
      .filter((words) => words[0])
      .map(([name, ...values]) => [name ?? "", values]),
  );
}

test("WEB-E2E-030 every page sends the app's policy with a fresh nonce its scripts carry", async ({
  page,
}) => {
  const nonces = new Set<string>();
  for (const path of ["/login", "/login", "/settings/models"]) {
    const response = await page.request.get(path);
    expect(response.status()).toBe(200);
    const policy = response.headers()["content-security-policy"] ?? "";
    const parts = directives(policy);
    const script = parts.get("script-src") ?? [];
    const nonce = script.find((value) => value.startsWith("'nonce-"));
    expect(nonce, policy).toBeTruthy();
    expect(script).toEqual(["'self'", nonce, "'strict-dynamic'"]);
    expect(parts.get("default-src")).toEqual(["'self'"]);
    expect(parts.get("connect-src")).toEqual(["'self'"]);
    expect(parts.get("object-src")).toEqual(["'none'"]);
    expect(parts.get("base-uri")).toEqual(["'none'"]);
    expect(parts.get("frame-ancestors")).toEqual(["'none'"]);
    expect(parts.get("font-src")).toEqual(["'self'"]);
    expect(policy).not.toContain("unsafe-eval");
    // A page holds its load's nonce, so no shared cache may replay it (a condition of WEB-E2E-010).
    const cache = response.headers()["cache-control"] ?? "";
    expect(cache, path).toContain("private");
    expect(cache, path).toContain("no-store");
    nonces.add(nonce ?? "");
    // Every script on the page, the layout's own inline ones among them, carries this load's nonce.
    const html = await response.text();
    const scripts = html.match(/<script\b[^>]*>/g) ?? [];
    expect(scripts.length).toBeGreaterThan(2);
    const value = (nonce ?? "").slice("'nonce-".length, -1);
    for (const tag of scripts) expect(tag).toContain(`nonce="${value}"`);
  }
  expect(nonces.size).toBe(3);
});

test("WEB-E2E-030 the app's pages run under its policy with no violation", async ({
  page,
}) => {
  test.setTimeout(180_000);
  const violations = await watch(page);
  await page.goto("/login");
  await expect(page.getByLabel("Email", { exact: true })).toBeVisible();
  await signIn(page);
  for (const path of PAGES) {
    const response = await page.goto(path);
    expect(response?.status(), path).toBe(200);
    await expect(page.locator("main").first(), path).toBeVisible();
    await page.waitForLoadState("load");
  }
  expect(violations).toEqual([]);
});

test("WEB-E2E-030 a task, its web page preview and a PDF drawn by pdf.js raise no violation", async ({
  page,
}) => {
  test.setTimeout(180_000);
  const violations = await watch(page);
  await page.setViewportSize({ width: 1280, height: 900 });
  await signIn(page);
  await page.goto(await createConversation(page));
  const input = page.getByRole("textbox", { name: "Message", exact: true });
  await input.fill("Make a chart and a brief");
  await input.press("Enter");
  await expect(page.getByLabel("Run status")).toHaveText("Done");
  const panel = await openFilesPanel(page);
  await panel.getByRole("button", { name: "reports" }).click();
  // The agent's page runs in its sandboxed frame from /files/view, under that route's own policy.
  await panel.getByRole("button", { name: /^report\.html/ }).click();
  await expect(
    panel.frameLocator("iframe").getByText("Scripts ran."),
  ).toBeVisible();
  // pdf.js and its worker, from our own origin; a preview replaces the folder list, so from a fresh load.
  const workers: string[] = [];
  page.on("worker", (worker) => workers.push(worker.url()));
  await page.reload();
  await panel.getByRole("button", { name: "reports" }).click();
  await panel.getByRole("button", { name: /^brief\.pdf/ }).click();
  await expect(panel.getByRole("group", { name: "Page 1" })).toBeVisible();
  await expect.poll(() => workers.length).toBeGreaterThan(0);
  expect(violations).toEqual([]);
});

/** How many of a PDF page's pixels are dark: the page's text, drawn. */
async function drawn(panel: ReturnType<Page["getByRole"]>) {
  const first = panel.getByRole("group", { name: "Page 1" }).locator("canvas");
  await expect(first).toBeVisible();
  await expect
    .poll(() =>
      first.evaluate((canvas: HTMLCanvasElement) => {
        const context = canvas.getContext("2d");
        if (!context) return 0;
        const { data } = context.getImageData(
          0,
          0,
          canvas.width,
          canvas.height,
        );
        let count = 0;
        for (let index = 0; index < data.length; index += 4)
          if ((data[index] ?? 255) < 128) count++;
        return count;
      }),
    )
    .toBeGreaterThan(50);
}

/** A font's name-table strings (Windows, US English) by name ID: 1 family, 5 version, 13 licence. */
function fontNames(bytes: Buffer): Record<number, string> {
  const tables = bytes.readUInt16BE(4);
  for (let index = 0; index < tables; index++) {
    const record = 12 + 16 * index;
    if (bytes.toString("latin1", record, record + 4) !== "name") continue;
    const table = bytes.readUInt32BE(record + 8);
    const count = bytes.readUInt16BE(table + 2);
    const strings = table + bytes.readUInt16BE(table + 4);
    const names: Record<number, string> = {};
    for (let entry = 0; entry < count; entry++) {
      const at = table + 6 + 12 * entry;
      if (bytes.readUInt16BE(at) !== 3 || bytes.readUInt16BE(at + 4) !== 0x409)
        continue;
      const start = strings + bytes.readUInt16BE(at + 10);
      const text = bytes.subarray(start, start + bytes.readUInt16BE(at + 8));
      names[bytes.readUInt16BE(at + 6)] = Buffer.from(text)
        .swap16()
        .toString("utf16le");
    }
    return names;
  }
  return {};
}

const PDFJS = (
  JSON.parse(readFileSync("node_modules/pdfjs-dist/package.json", "utf8")) as {
    version: string;
  }
).version;

test("WEB-E2E-030 PDFs in Symbol, Chinese and Helvetica draw, with fonts and CMaps from our own origin", async ({
  page,
}) => {
  test.setTimeout(120_000);
  const violations = await watch(page);
  // pdf.js's own requests for font data and CMaps, wherever they go, and its warnings when it lacks them.
  const fetched: string[] = [];
  page.context().on("requestfinished", (request) => {
    void request.response().then((response) => {
      if (/\/(standard_fonts|cmaps)\//.test(request.url()))
        fetched.push(`${response?.status()} ${request.url()}`);
    });
  });
  const warnings: string[] = [];
  page.on("console", (message) => {
    if (/standardFontDataUrl|cMapUrl|CMap|font data/i.test(message.text()))
      warnings.push(message.text());
  });
  await page.setViewportSize({ width: 1280, height: 900 });
  await signIn(page);
  const task = await createConversation(page);
  let origin = "";
  const open = async (file: RegExp) => {
    await page.goto(task);
    origin = new URL(page.url()).origin;
    const panel = await openFilesPanel(page);
    await panel.getByRole("button", { name: "reports" }).click();
    await panel.getByRole("button", { name: file }).click();
    await drawn(panel);
  };
  // Symbol has no system font, so pdf.js draws it from our standard font data; Chinese needs our CMaps to read it.
  for (const [file, kind] of [
    [/^symbol\.pdf/, "standard_fonts"],
    [/^chinese\.pdf/, "cmaps"],
  ] as const) {
    await open(file);
    await expect
      .poll(
        () =>
          fetched.filter((line) =>
            line.startsWith(`200 ${origin}/pdfjs/${PDFJS}/${kind}/`),
          ).length,
        {
          message: `${String(file)}: ${kind} from our own origin (${fetched.join(", ")})`,
        },
      )
      .toBeGreaterThan(0);
  }
  // Helvetica: a local sans-serif, else Arimo (served for pdf.js's Liberation Sans names).
  await open(/^brief\.pdf/);
  expect(fetched.filter((line) => !line.startsWith(`200 ${origin}/`))).toEqual(
    [],
  );
  expect(warnings).toEqual([]);
  expect(violations).toEqual([]);
});

test("WEB-E2E-030 pdf.js's Liberation Sans URLs serve Arimo 1.31, under its Apache-2.0 grant", async ({
  page,
}) => {
  for (const style of ["Regular", "Bold", "Italic", "BoldItalic"]) {
    const response = await page.request.get(
      `/pdfjs/${PDFJS}/standard_fonts/LiberationSans-${style}.ttf`,
    );
    expect(response.status(), style).toBe(200);
    const names = fontNames(await response.body());
    expect(names[1], style).toBe("Arimo");
    expect(names[5], style).toBe("Version 1.31");
    expect(names[13], style).toBe(
      "Licensed under the Apache License, Version 2.0",
    );
  }
  // Under its own name too, and nothing of pdfjs-dist's wasm/ (its scripting engine among it) is served.
  expect(
    (
      await page.request.get(`/pdfjs/${PDFJS}/standard_fonts/Arimo-Regular.ttf`)
    ).status(),
  ).toBe(200);
  expect(
    (await page.request.get(`/pdfjs/${PDFJS}/wasm/quickjs-eval.wasm`)).status(),
  ).toBe(404);
});

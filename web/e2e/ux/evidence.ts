import { mkdir, writeFile } from "node:fs/promises";
import { join } from "node:path";

import {
  test as base,
  type Page,
  type Request,
  type TestInfo,
} from "@playwright/test";

import type { observations } from "./observer";

type Evidence = {
  conversations: string[];
  samples: Awaited<ReturnType<typeof observations>>[];
  consoleFailures: string[];
  requests: {
    method: string;
    path: string;
    status: number | null;
    failure: string | null;
    elapsedMs: number;
  }[];
};

function observeRequests(page: Page, log: Evidence) {
  const started = new WeakMap<Request, number>();
  page.on("request", (request) => started.set(request, performance.now()));
  const record = (
    request: Request,
    status: number | null,
    failure: string | null,
  ) => {
    const path = new URL(request.url()).pathname;
    if (!path.startsWith("/api/") || log.requests.length >= 80) return;
    log.requests.push({
      method: request.method(),
      path,
      status,
      failure,
      elapsedMs: Math.round(
        performance.now() - (started.get(request) ?? performance.now()),
      ),
    });
  };
  page.on("response", (response) =>
    record(response.request(), response.status(), null),
  );
  page.on("requestfailed", (request) =>
    record(
      request,
      null,
      request.failure()?.errorText.slice(0, 300) ?? "Request failed",
    ),
  );
}

async function save(page: Page, info: TestInfo, log: Evidence) {
  const body = JSON.stringify(
    {
      title: info.title,
      ...log,
      finalSample: await page.evaluate(() => window.__ux ?? null),
      failures: info.errors.map((error) => error.message),
    },
    null,
    2,
  );
  await info.attach("UX observations", {
    body,
    contentType: "application/json",
  });
  const folder = process.env.TQA_WEB_EVIDENCE_DIR;
  if (!folder) return;
  await mkdir(folder, { recursive: true });
  const suffix = /UX (\d+) (light|dark)/.exec(info.title)?.slice(1).join("-");
  if (!suffix) throw new Error("Unexpected UX evidence name");
  await writeFile(join(folder, `ux-${suffix}.json`), body, { mode: 0o600 });
  if (info.errors.length || process.env.TQA_UX_REAL_MODEL === "1")
    await page.screenshot({ path: join(folder, `ux-${suffix}-result.png`) });
}

/** Always retain observations and failures, including a test that stops at a hard assertion. */
export const test = base.extend<{ log: Evidence }>({
  log: async ({ page }, run, info) => {
    const log: Evidence = {
      conversations: [],
      samples: [],
      consoleFailures: [],
      requests: [],
    };
    observeRequests(page, log);
    page.on("console", (message) => {
      if (
        ["error", "warning"].includes(message.type()) &&
        log.consoleFailures.length < 20
      )
        log.consoleFailures.push(
          `${message.type()}: ${message.text().slice(0, 1000)}`,
        );
    });
    page.on("pageerror", (error) => {
      if (log.consoleFailures.length < 20)
        log.consoleFailures.push(error.message.slice(0, 1000));
    });
    await run(log);
    await save(page, info, log);
  },
});

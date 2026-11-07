import { createServer, type Server } from "node:http";
import type { AddressInfo } from "node:net";

import { expect, test } from "@playwright/test";

import { answered } from "./answered";

// WEB-E2E-089 (the 16n integration, 17 Oct 2026): the models-people suite read an answer's body after the page had
// moved on, couldn't, and recorded `{}`: an empty answer the gateway never sent ("unreachable" became no failure). A
// step's answer is read as it arrives, and one that can't be read, or isn't JSON, fails the step, never reads as empty.
let server: Server;
let origin = "";

test.beforeAll(async () => {
  server = createServer((request, response) => {
    if (request.url === "/answer") {
      response.setHeader("content-type", "application/json");
      response.end(
        JSON.stringify({ models: [], failure: { code: "unreachable" } }),
      );
    } else if (request.url === "/stalled") {
      // Headers and the start of a body, and then nothing: the rest never comes.
      response.writeHead(200, { "content-type": "application/json" });
      response.write('{"models": [');
    } else if (request.url === "/garbled") {
      response.setHeader("content-type", "application/json");
      response.end("<html>not the gateway</html>");
    } else {
      response.setHeader("content-type", "text/html");
      response.end("<!doctype html><title>Answers</title><body></body>");
    }
  });
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  origin = `http://127.0.0.1:${(server.address() as AddressInfo).port}`;
});

test.afterAll(async () => {
  server.closeAllConnections();
  await new Promise((resolve) => server.close(resolve));
});

test("WEB-E2E-089 an answer is read as the gateway sent it", async ({
  page,
}) => {
  await page.goto(`${origin}/`);
  const answer = await answered(page, "/answer", () =>
    page.evaluate(() => fetch("/answer").then(() => undefined)),
  );
  expect(answer.status).toBe(200);
  expect(answer.body).toEqual({ models: [], failure: { code: "unreachable" } });
});

test("WEB-E2E-089 an answer whose body never comes fails the step, never reads as empty", async ({
  page,
}) => {
  await page.goto(`${origin}/`);
  await expect(
    answered(
      page,
      "/stalled",
      async () => {
        // The step goes on once the answer's headers are in: the page leaves before its body comes.
        await Promise.all([
          page.waitForResponse(
            (response) => new URL(response.url()).pathname === "/stalled",
          ),
          page.evaluate(() => void fetch("/stalled")),
        ]);
        await page.goto("about:blank");
      },
      5_000,
    ),
  ).rejects.toThrow("The answer to /stalled couldn't be read");
});

test("WEB-E2E-089 an answer that isn't JSON fails the step, never reads as empty", async ({
  page,
}) => {
  await page.goto(`${origin}/`);
  await expect(
    answered(page, "/garbled", () =>
      page.evaluate(() => fetch("/garbled").then(() => undefined)),
    ),
  ).rejects.toThrow("The answer to /garbled isn't JSON");
});

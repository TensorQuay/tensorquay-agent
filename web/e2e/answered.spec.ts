import { createServer, type Server } from "node:http";
import type { AddressInfo } from "node:net";

import { expect, test } from "@playwright/test";

import { answered, everyAnswer } from "./answered";

// WEB-E2E-089 (an integration before 17.0, 17 Oct 2026): the models-people suite read an answer's body after the
// page had moved on, couldn't, and recorded `{}`: an empty answer the gateway never sent ("unreachable" became no
// failure). A step's answer is read as it arrives, and one that can't be read, or isn't JSON, fails the step, never
// reads as empty.
let server: Server;
let origin = "";

// Requests for /api/seq, counted per run: the first is answered after 400 ms, the second after 800 ms.
const seq = new Map<string, number>();

test.beforeAll(async () => {
  server = createServer((request, response) => {
    const url = new URL(request.url ?? "/", "http://localhost");
    if (url.pathname === "/api/seq") {
      const run = url.searchParams.get("run") ?? "";
      const count = (seq.get(run) ?? 0) + 1;
      seq.set(run, count);
      setTimeout(
        () => {
          response.setHeader("content-type", "application/json");
          response.end(JSON.stringify({ request: count }));
        },
        count === 1 ? 400 : 800,
      );
    } else if (request.url === "/answer") {
      response.setHeader("content-type", "application/json");
      response.end(
        JSON.stringify({ models: [], failure: { code: "unreachable" } }),
      );
    } else if (request.url === "/stalled") {
      // Headers and the start of a body, and then nothing: the rest never comes.
      response.writeHead(200, { "content-type": "application/json" });
      response.write('{"models": [');
    } else if (request.url === "/large") {
      // Over the 20 MB Chromium keeps of a response for DevTools: the page reads all of it, DevTools keeps none.
      response.setHeader("content-type", "application/json");
      response.end(JSON.stringify({ filler: "x".repeat(21_000_000) }));
    } else if (request.url === "/api/slow") {
      setTimeout(() => {
        response.setHeader("content-type", "application/json");
        response.end('{"slow":"secret"}');
      }, 600);
    } else if (request.url === "/slow") {
      // Headers at once, the body a moment later.
      response.writeHead(200, { "content-type": "application/json" });
      setTimeout(() => response.end('{"models": []}'), 500);
    } else if (request.url === "/api/one") {
      response.setHeader("content-type", "application/json");
      response.end('{"one":1}');
    } else if (request.url === "/api/broken") {
      // The start of a body, and then the connection goes.
      response.writeHead(200, { "content-type": "application/json" });
      response.write('{"partial":"secret-part');
      setTimeout(() => response.destroy(), 100);
    } else if (request.url === "/api/events") {
      response.writeHead(200, { "content-type": "text/event-stream" });
      response.write("data: hello\n\n");
    } else if (request.url === "/api/empty") {
      response.writeHead(204).end();
    } else if (request.url === "/api/moved") {
      response.writeHead(302, { location: "/api/one" }).end();
    } else if (request.url === "/api/dropped") {
      // The connection goes before any answer.
      request.socket.destroy();
    } else if (request.url === "/api/stalled") {
      response.writeHead(200, { "content-type": "application/json" });
      response.write("{");
    } else if (request.url === "/garbled") {
      response.setHeader("content-type", "application/json");
      response.end("<html>not the gateway</html>");
    } else if (url.pathname.startsWith("/api/")) {
      response.setHeader("content-type", "application/json");
      response.end(JSON.stringify({ path: url.pathname }));
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
  // The page stays; the answer's headers come, and its body never does.
  await expect(
    answered(
      page,
      "/stalled",
      () => page.evaluate(() => void fetch("/stalled")),
      1_000,
    ),
  ).rejects.toThrow(
    "The answer to /stalled couldn't be read: its body didn't come within 1000 ms",
  );
});

// The answer read is the page's own copy of what its request received, never Chromium's DevTools copy, which it may not
// keep (17.4's release candidate, 10 Nov 2026: DevTools had no copy of a step's answer, and the step failed on a
// healthy gateway).
test("WEB-E2E-089 an answer the page read is read whole, though the browser keeps no copy of it", async ({
  page,
}) => {
  await page.goto(`${origin}/`);
  const answer = await answered(page, "/large", () =>
    page.evaluate(() =>
      fetch("/large")
        .then((response) => response.json())
        .then(() => undefined),
    ),
  );
  expect(answer.status).toBe(200);
  expect((answer.body as { filler: string }).filler).toHaveLength(21_000_000);
});

test("WEB-E2E-089 an answer the page cuts off after its headers fails the step, saying the page did", async ({
  page,
}) => {
  await page.goto(`${origin}/`);
  await expect(
    answered(
      page,
      "/slow",
      () =>
        page.evaluate(async () => {
          const control = new AbortController();
          const response = await fetch("/slow", { signal: control.signal });
          control.abort();
          await response.text().catch(() => undefined);
        }),
      5_000,
    ),
  ).rejects.toThrow(
    "The answer to /slow couldn't be read: the page cut its own request off before the body came",
  );
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

// The models-people suite searches every API answer for a pasted key: each is the page's own copy, one whose body broke
// off as far as it came, and one that never ends is named, never left out.
test("WEB-E2E-089 every answer under a path is the page's own copy, a broken one as far as it came", async ({
  page,
}) => {
  await page.goto(`${origin}/`);
  const answers = await everyAnswer(page, "/api/");
  await page.evaluate(async () => {
    await fetch("/api/one").then((response) => response.json());
    await fetch("/api/broken")
      .then((response) => response.text())
      .catch(() => undefined);
    await fetch("/answer").then((response) => response.json());
  });
  expect((await answers.bodies()).sort()).toEqual([
    '{"one":1}',
    '{"partial":"secret-part',
  ]);
});

test("WEB-E2E-089 an answer under the path that never ends is named, never left out", async ({
  page,
}) => {
  await page.goto(`${origin}/`);
  const answers = await everyAnswer(page, "/api/");
  await page.evaluate(() => void fetch("/api/stalled"));
  await expect(answers.bodies(1_000)).rejects.toThrow(
    "Answers that never ended within 1000 ms: /api/stalled",
  );
});

// The search must not grow narrower without saying so: the page asks under the path in other ways than fetch too (an
// upload is an XMLHttpRequest). An answer the page received there holds the searched words, or it is named.
test("WEB-E2E-089 an answer under the path the page asked for some other way is searched or named", async ({
  page,
}) => {
  await page.goto(`${origin}/`);
  const answers = await everyAnswer(page, "/api/");
  await page.evaluate(
    () =>
      new Promise<void>((resolve) => {
        const request = new XMLHttpRequest();
        request.open("GET", "/api/one");
        request.onloadend = () => resolve();
        request.send();
      }),
  );
  await answers.bodies(2_000).then(
    (bodies) => expect(bodies.join("\n")).toContain('"one":1'),
    (error: unknown) => expect(String(error)).toContain("/api/one"),
  );
});

// What carries nothing to search is never named: a redirect, an answer with no body, an event stream.
test("WEB-E2E-089 a redirect, an answer with no body and an event stream under the path are not named", async ({
  page,
}) => {
  await page.goto(`${origin}/`);
  const answers = await everyAnswer(page, "/api/");
  await page.evaluate(async () => {
    await new Promise<void>((resolve) => {
      const events = new EventSource("/api/events");
      events.onmessage = () => {
        events.close();
        resolve();
      };
    });
    await fetch("/api/empty");
    await fetch("/api/moved").then((response) => response.json());
  });
  expect((await answers.bodies(2_000)).sort()).toEqual(["", '{"one":1}']);
});

test("WEB-E2E-089 a request that fails before any answer fails the step at once, saying so", async ({
  page,
}) => {
  await page.goto(`${origin}/`);
  const started = Date.now();
  await expect(
    answered(
      page,
      "/api/dropped",
      () =>
        page.evaluate(() =>
          fetch("/api/dropped").then(
            () => undefined,
            () => undefined,
          ),
        ),
      30_000,
    ),
  ).rejects.toThrow(
    "No answer to /api/dropped: the request failed before its answer came",
  );
  expect(Date.now() - started).toBeLessThan(10_000);
});

test("WEB-E2E-089 a page that leaves after the head fails the step at once, saying so", async ({
  page,
}) => {
  await page.goto(`${origin}/`);
  const started = Date.now();
  await expect(
    answered(
      page,
      "/stalled",
      async () => {
        await Promise.all([
          page.waitForResponse(
            (response) => new URL(response.url()).pathname === "/stalled",
          ),
          page.evaluate(() => void fetch("/stalled")),
        ]);
        await page.goto(`${origin}/`);
      },
      30_000,
    ),
  ).rejects.toThrow(
    "The answer to /stalled couldn't be read: the page left before its body came",
  );
  expect(Date.now() - started).toBeLessThan(10_000);
});

// A request already on its way when the search begins is outside it, and the copy of its answer must not stand in for
// an answer on the same path that the page asked for some other way.
test("WEB-E2E-089 an answer asked for other than by fetch is named, though an earlier fetch left a copy", async ({
  page,
}) => {
  await page.goto(`${origin}/`);
  await everyAnswer(page, "/zzz/");
  await Promise.all([
    page.waitForRequest((request) => request.url().endsWith("/api/slow")),
    page.evaluate(() => {
      (window as unknown as { early: Promise<string> }).early = fetch(
        "/api/slow",
      ).then((response) => response.text());
    }),
  ]);
  const answers = await everyAnswer(page, "/api/");
  await page.evaluate(async () => {
    await (window as unknown as { early: Promise<string> }).early;
    await new Promise<void>((resolve) => {
      const request = new XMLHttpRequest();
      request.open("GET", "/api/slow");
      request.onloadend = () => resolve();
      request.send();
    });
  });
  await expect(answers.bodies(2_000)).rejects.toThrow("/api/slow");
});

// A worker has its own fetch, which the page's wrap doesn't reach: its answer is named by counting what the browser
// answered against the page's copies.
test("WEB-E2E-089 a worker's own fetch under the path is named", async ({
  page,
}) => {
  await page.goto(`${origin}/`);
  const answers = await everyAnswer(page, "/api/");
  await page.evaluate(
    () =>
      new Promise<void>((resolve) => {
        const source = `fetch(${JSON.stringify(`${location.origin}/api/one`)}).then((response) => response.text()).then(() => postMessage("done"));`;
        const worker = new Worker(
          URL.createObjectURL(new Blob([source], { type: "text/javascript" })),
        );
        worker.onmessage = () => resolve();
      }),
  );
  await expect(answers.bodies(2_000)).rejects.toThrow("/api/one");
});

// In the app, a fetch can be cut off (its AbortController) a moment after its answer reached the browser: Chromium
// reports the answer, while the page's fetch rejects with an AbortError before any head. The page received nothing,
// so there is nothing to search and nothing to name. That moment can't be made on demand, so the page stands in for
// it: under the wrap, its own fetch takes the real answer and then rejects as a cut-off does.
test("WEB-E2E-089 a request the page cut off before its head is not named, though the browser got its answer", async ({
  page,
}) => {
  await page.goto(`${origin}/`);
  await page.evaluate(() => {
    const real = window.fetch.bind(window);
    window.fetch = async (...args: Parameters<typeof window.fetch>) => {
      const response = await real(...args);
      const [input] = args;
      if (
        (input instanceof Request ? input.url : input.toString()).includes(
          "/api/raced",
        )
      ) {
        await response.text();
        throw new DOMException("The user aborted a request.", "AbortError");
      }
      return response;
    };
  });
  const answers = await everyAnswer(page, "/api/");
  await page.evaluate(async () => {
    await fetch("/api/raced").catch(() => undefined);
    await fetch("/api/one").then((response) => response.text());
  });
  expect((await answers.bodies(2_000)).sort()).toEqual(["", '{"one":1}']);
});

// The same at a navigation: the browser can have the answer as the page leaves, before the page's fetch has its head.
// The page stands in for that moment: its fetch takes the real answer and never settles, and then the page goes.
test("WEB-E2E-089 a request whose page left before its head is not named, though the browser got its answer", async ({
  page,
}) => {
  await page.goto(`${origin}/`);
  await page.evaluate(() => {
    const real = window.fetch.bind(window);
    window.fetch = (...args: Parameters<typeof window.fetch>) => {
      const asked = real(...args);
      const [input] = args;
      if (
        !(input instanceof Request ? input.url : input.toString()).includes(
          "/api/unseen",
        )
      )
        return asked;
      void asked
        .then((response) => response.text())
        .then(() => {
          (window as unknown as { done: boolean }).done = true;
        });
      return new Promise<Response>(() => undefined);
    };
  });
  const answers = await everyAnswer(page, "/api/");
  await page.evaluate(() => void fetch("/api/unseen"));
  await page.waitForFunction(
    () => (window as unknown as { done?: boolean }).done === true,
  );
  await page.goto(`${origin}/other`);
  await page.evaluate(() =>
    fetch("/api/one").then((response) => response.text()),
  );
  expect(await answers.bodies(2_000)).toEqual(['{"one":1}']);
});

// The guard beside it: with a request cut off on one path, an answer with no copy on another (a fetch that goes round
// the wrap, as a worker's does) is still named, and the cut-off path is not.
test("WEB-E2E-089 beside a request cut off before its head, an answer with no copy is still named", async ({
  page,
}) => {
  await page.goto(`${origin}/`);
  await page.evaluate(() => {
    const real = window.fetch.bind(window);
    (window as unknown as { real: typeof window.fetch }).real = real;
    window.fetch = async (...args: Parameters<typeof window.fetch>) => {
      const response = await real(...args);
      const [input] = args;
      if (
        (input instanceof Request ? input.url : input.toString()).includes(
          "/api/raced",
        )
      )
        throw new DOMException("The user aborted a request.", "AbortError");
      return response;
    };
  });
  const answers = await everyAnswer(page, "/api/");
  await page.evaluate(async () => {
    await fetch("/api/raced").catch(() => undefined);
    await (window as unknown as { real: typeof window.fetch })
      .real("/api/other")
      .then((response) => response.text());
  });
  const named = await answers.bodies(2_000).then(
    () => "",
    (error: unknown) => String(error),
  );
  expect(named).toContain("with no copy to search: /api/other");
  expect(named).not.toContain("/api/raced");
});

// A request for the path already on its way when the step begins is never taken for the step's, though its answer
// comes first.
test("WEB-E2E-089 a step's answer is its own request's, not one already on its way", async ({
  page,
}) => {
  await page.goto(`${origin}/`);
  await everyAnswer(page, "/zzz/");
  await page.evaluate(() => {
    void fetch("/api/seq?run=step").then((response) => response.text());
  });
  const answer = await answered(
    page,
    "/api/seq",
    () =>
      page.evaluate(() => {
        void fetch("/api/seq?run=step").then((response) => response.text());
      }),
    5_000,
  );
  expect(answer.body).toEqual({ request: 2 });
});

test("WEB-E2E-089 a step that asks nothing gets no answer, though an earlier request answers during it", async ({
  page,
}) => {
  await page.goto(`${origin}/`);
  await everyAnswer(page, "/zzz/");
  await page.evaluate(() => {
    void fetch("/api/seq?run=nothing").then((response) => response.text());
  });
  await expect(
    answered(page, "/api/seq", () => Promise.resolve(), 1_500),
  ).rejects.toThrow("No answer to /api/seq within 1500 ms");
});

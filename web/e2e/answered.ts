// The answers to the requests a step makes, read for a check: their status, their headers and their bodies, as the
// page itself received them.

import type { Page, Request } from "@playwright/test";

/** An answer as the test read it. */
export type Answered = {
  status: number;
  headers: Record<string, string>;
  body: unknown;
};

/**
 * What the page's own fetch did for one request: its start, its head once it came, then its body, whole or as far as it
 * came (a fetch that failed before its head ends with a body that isn't whole); or a skip, for an answer not copied. A
 * request's id names its document, so ids never meet across a navigation; each new document of the main frame says so.
 */
type Heard =
  | { kind: "document"; doc: string }
  | { kind: "start"; id: string; path: string }
  | { kind: "skip"; id: string }
  | {
      kind: "head";
      id: string;
      path: string;
      status: number;
      headers: Record<string, string>;
    }
  | {
      kind: "body";
      id: string;
      text: string;
      whole: boolean;
      cutByPage: boolean;
      failure: string | null;
    };

type Listener = (heard: Heard) => void;

const BINDING = "__tqaAnswerHeard";

/**
 * Runs in each document of the page: wraps its fetch so each answer the page receives is copied as it arrives, by a
 * second reader of the page's own response (`clone()`). The copy is the gateway's answer to the browser's own request,
 * read in the page, never Chromium's DevTools copy, which it may not keep: a body over its 20 MB limit, or one it
 * had no data for. Event streams never end and server-component payloads are the build's own, so neither is copied.
 * The app calls `fetch` by name at each request, so a document already loaded uses the wrap from then on.
 */
function tee(binding: string) {
  const scope = window as unknown as Record<string, unknown>;
  if (scope.__tqaAnswerTee) return;
  scope.__tqaAnswerTee = true;
  const tell = (heard: unknown) => {
    const report = scope[binding] as
      ((heard: unknown) => Promise<void>) | undefined;
    void report?.(heard).catch(() => undefined);
  };
  const own = window.fetch.bind(window);
  const aborted = (error: unknown) =>
    error instanceof DOMException && error.name === "AbortError";
  const doc = Math.random().toString(36).slice(2);
  let next = 0;
  tell({ kind: "document", doc });
  window.fetch = async (...args: Parameters<typeof window.fetch>) => {
    const id = `${doc}-${next}`;
    next += 1;
    const [input] = args;
    tell({
      kind: "start",
      id,
      path: new URL(
        input instanceof Request ? input.url : String(input),
        location.href,
      ).pathname,
    });
    let response: Response;
    try {
      response = await own(...args);
    } catch (error) {
      tell({
        kind: "body",
        id,
        text: "",
        whole: false,
        cutByPage: aborted(error),
        failure: String(error),
      });
      throw error;
    }
    const type = response.headers.get("content-type") ?? "";
    if (
      type.startsWith("text/event-stream") ||
      type.startsWith("text/x-component")
    ) {
      tell({ kind: "skip", id });
      return response;
    }
    const headers: Record<string, string> = {};
    response.headers.forEach((value, key) => {
      headers[key] = value;
    });
    tell({
      kind: "head",
      id,
      path: new URL(response.url || location.href, location.href).pathname,
      status: response.status,
      headers,
    });
    const reader = response.clone().body?.getReader();
    void (async () => {
      const decoder = new TextDecoder();
      let text = "";
      try {
        while (reader) {
          const part = await reader.read();
          if (part.done) break;
          text += decoder.decode(part.value, { stream: true });
        }
        text += decoder.decode();
        tell({
          kind: "body",
          id,
          text,
          whole: true,
          cutByPage: false,
          failure: null,
        });
      } catch (error) {
        tell({
          kind: "body",
          id,
          text,
          whole: false,
          cutByPage: aborted(error),
          failure: String(error),
        });
      }
    })();
    return response;
  };
}

const setups = new WeakMap<Page, Promise<Set<Listener>>>();

/** The page's copies of its answers: the binding and the wrap, set up once per page, in this and every later
 * document. */
function heard(page: Page): Promise<Set<Listener>> {
  let setup = setups.get(page);
  if (!setup) {
    const listeners = new Set<Listener>();
    setup = (async () => {
      await page.exposeBinding(BINDING, (source, value: Heard) => {
        // A frame's own document is not the page leaving.
        if (value.kind === "document" && source.frame !== page.mainFrame())
          return;
        for (const listener of listeners) listener(value);
      });
      await page.addInitScript(tee, BINDING);
      await page.evaluate(tee, BINDING);
      return listeners;
    })();
    setups.set(page, setup);
  }
  return setup;
}

/**
 * The answer to the first request for `path` that `act` makes, as the page received it (WEB-E2E-089, an integration
 * before 17.0: the models-people suite read a body after the step had moved on, got nothing, and recorded an empty
 * answer the gateway never sent). A request already on its way when the step begins is never taken for the step's.
 * A body that the page's request didn't receive whole, never comes within `timeout`, or isn't JSON fails the step,
 * saying which: never an empty answer. The first request for `path` failing before any answer, or the page leaving
 * after an answer's head, fails it at once.
 */
export async function answered(
  page: Page,
  path: string,
  act: () => Promise<unknown>,
  timeout = 90_000,
): Promise<Answered> {
  const listeners = await heard(page);
  let listener: Listener | null = null;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let head: Extract<Heard, { kind: "head" }> | null = null;
  let first: string | null = null;
  // The requests begun during the step (any path: a redirect to `path` still counts).
  const mine = new Set<string>();
  const arrived = new Promise<Answered>((resolve, reject) => {
    listener = (value) => {
      if (value.kind === "start") {
        mine.add(value.id);
        if (first === null && value.path === path) first = value.id;
        return;
      }
      if (value.kind === "head") {
        if (!head && mine.has(value.id) && value.path === path) head = value;
        return;
      }
      if (value.kind === "document") {
        if (head)
          reject(
            new Error(
              `The answer to ${path} couldn't be read: the page left before its body came`,
            ),
          );
        return;
      }
      if (value.kind !== "body") return;
      if (!head) {
        if (value.id === first && !value.whole)
          reject(
            new Error(
              value.cutByPage
                ? `No answer to ${path}: the page cut its own request off before its answer came (${value.failure})`
                : `No answer to ${path}: the request failed before its answer came (${value.failure})`,
            ),
          );
        return;
      }
      if (value.id !== head.id) return;
      if (!value.whole)
        reject(
          new Error(
            value.cutByPage
              ? `The answer to ${path} couldn't be read: the page cut its own request off before the body came (${value.failure})`
              : `The answer to ${path} couldn't be read: its body broke off (${value.failure})`,
          ),
        );
      else
        try {
          resolve({
            status: head.status,
            headers: head.headers,
            body: JSON.parse(value.text) as unknown,
          });
        } catch {
          reject(
            new Error(
              `The answer to ${path} isn't JSON: ${value.text.slice(0, 200)}`,
            ),
          );
        }
    };
    listeners.add(listener);
    timer = setTimeout(
      () =>
        reject(
          new Error(
            head
              ? `The answer to ${path} couldn't be read: its body didn't come within ${timeout} ms`
              : `No answer to ${path} within ${timeout} ms`,
          ),
        ),
      timeout,
    );
  });
  try {
    const [answer] = await Promise.all([arrived, act()]);
    return answer;
  } finally {
    clearTimeout(timer);
    if (listener) listeners.delete(listener);
  }
}

/**
 * The answers to the requests the page makes under `prefix` from now on (one already on its way is outside them),
 * searched as the page received them: the page's own copy of each answer to its fetches, whole or as far as it came
 * before its request was cut off. A request the page cut off, or left, before its answer's head reached it received
 * nothing, whatever the browser got a moment earlier: nothing to search and nothing named. `bodies()` waits for the
 * requests already started, then fails, naming the path, for any answer the page received with no copy: one asked for
 * other than by fetch (an XMLHttpRequest, a link), named by how it was asked; one whose body left with its page; or
 * one that never ends. A worker's own fetch is found by counting the fetches the browser answered against the page's
 * copies, so it goes unnamed only on a path where the page's own fetches left a spare (one already on its way, a 204,
 * a HEAD, or a request cut off or left before its head). Redirects, answers with no body (204, 304, HEAD) and event
 * streams carry nothing to search.
 */
export async function everyAnswer(page: Page, prefix: string) {
  const listeners = await heard(page);
  const open = new Map<string, string>();
  const headed = new Set<string>();
  const left: string[] = [];
  const otherwise: string[] = [];
  const texts: string[] = [];
  const copied = new Map<string, number>();
  const received = new Map<string, number>();
  // Requests the page cut off, or left, before their head: the browser may have had an answer the page never saw.
  const unseen = new Map<string, number>();
  const add = (counts: Map<string, number>, path: string) =>
    counts.set(path, (counts.get(path) ?? 0) + 1);
  let settled: (() => void) | null = null;
  const close = (id: string) => {
    open.delete(id);
    headed.delete(id);
    if (open.size === 0) settled?.();
  };
  listeners.add((value) => {
    if (value.kind === "document") {
      // The page left: what its earlier documents still had open never comes.
      for (const [id, path] of open)
        if (!id.startsWith(`${value.doc}-`)) {
          if (headed.has(id)) left.push(path);
          else add(unseen, path);
          close(id);
        }
    } else if (value.kind === "start") {
      if (value.path.startsWith(prefix)) open.set(value.id, value.path);
    } else if (value.kind === "head") {
      if (value.path.startsWith(prefix)) add(copied, value.path);
      if (open.has(value.id)) headed.add(value.id);
    } else if (open.has(value.id)) {
      if (value.kind === "body") {
        texts.push(value.text);
        if (!headed.has(value.id) && value.cutByPage)
          add(unseen, open.get(value.id) ?? "");
      }
      close(value.id);
    }
  });
  // What the browser received under the prefix, from Playwright: paths and counts only, no body read from DevTools.
  const asked = new WeakSet<Request>();
  page.on("request", (request) => {
    if (new URL(request.url()).pathname.startsWith(prefix)) asked.add(request);
  });
  page.on("response", (response) => {
    const request = response.request();
    const status = response.status();
    const type = response.headers()["content-type"] ?? "";
    if (
      !asked.has(request) ||
      (status >= 300 && status < 400) ||
      status === 204 ||
      status === 304 ||
      request.method() === "HEAD" ||
      type.startsWith("text/event-stream") ||
      type.startsWith("text/x-component")
    )
      return;
    const path = new URL(response.url()).pathname;
    // Asked for other than by fetch, it has no copy: named by how it was asked, never left to a count.
    if (request.resourceType() === "fetch") add(received, path);
    else otherwise.push(path);
  });
  const missing = () =>
    [...received]
      .filter(
        ([path, count]) =>
          (copied.get(path) ?? 0) + (unseen.get(path) ?? 0) < count,
      )
      .map(([path]) => path);
  return {
    async bodies(timeout = 30_000): Promise<string[]> {
      if (open.size > 0) {
        let timer: ReturnType<typeof setTimeout> | undefined;
        await new Promise<void>((resolve) => {
          settled = resolve;
          timer = setTimeout(resolve, timeout);
        });
        clearTimeout(timer);
        settled = null;
      }
      if (open.size > 0)
        throw new Error(
          `Answers that never ended within ${timeout} ms: ${[...open.values()].join(", ")}`,
        );
      // A copy's head comes by the page's binding, a moment after Playwright hears the answer.
      for (let waited = 0; missing().length > 0 && waited < 2_000; waited += 50)
        await new Promise((resolve) => setTimeout(resolve, 50));
      const unread = [...otherwise, ...missing(), ...left];
      if (unread.length > 0)
        throw new Error(
          `Answers under ${prefix} the page received with no copy to search: ${unread.join(", ")}`,
        );
      return [...texts];
    },
  };
}

// The answer to a request a step makes, read for a check: its status, its headers and its JSON body.

import type { Page, Response } from "@playwright/test";

/** An answer as the test read it. */
export type Answered = {
  status: number;
  headers: Record<string, string>;
  body: unknown;
};

/**
 * The answer to the first request for `path` that `act` makes, its body read in the response handler, as the answer
 * arrives (WEB-E2E-089, the 16n integration: the models-people suite read a body after the step had moved on, got
 * nothing, and recorded an empty answer the gateway never sent). A body that can't be read, never comes within
 * `timeout`, or isn't JSON fails the step: never an empty answer.
 */
export async function answered(
  page: Page,
  path: string,
  act: () => Promise<unknown>,
  timeout = 90_000,
): Promise<Answered> {
  let listener: ((response: Response) => void) | null = null;
  let heard = false;
  let timer: ReturnType<typeof setTimeout> | undefined;
  const arrived = new Promise<Answered>((resolve, reject) => {
    const answer = (response: Response) => {
      if (new URL(response.url()).pathname !== path) return;
      page.off("response", answer);
      listener = null;
      heard = true;
      response.text().then(
        (text) => {
          try {
            resolve({
              status: response.status(),
              headers: response.headers(),
              body: JSON.parse(text) as unknown,
            });
          } catch {
            reject(
              new Error(
                `The answer to ${path} isn't JSON: ${text.slice(0, 200)}`,
              ),
            );
          }
        },
        (error: unknown) =>
          reject(
            new Error(
              `The answer to ${path} couldn't be read: ${String(error)}`,
            ),
          ),
      );
    };
    listener = answer;
    page.on("response", answer);
    timer = setTimeout(
      () =>
        reject(
          new Error(
            heard
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
    if (listener) page.off("response", listener);
  }
}

import { expect, type Page, type Request } from "@playwright/test";

// Shared by WEB-E2E-010's connector and profile halves on the real stack: the driver's environment, and the search of
// every response body and the page for the credential's forms (never printed).
export const env = (name: string) => {
  const value = process.env[name];
  if (!value) throw new Error(`The driver must supply ${name}`);
  return value;
};
export const EMAIL = "connector@example.invalid";

/**
 * Every text response body the page receives, searched for the credential's forms; never printed. The API's own
 * responses and the build's scripts are fetched for the page and searched in full before it gets them (profile run 4:
 * the page cancels a query's fetch after its headers when the query refetches or unmounts, so its body could never be
 * read afterwards).
 * Anything else is searched once its request has finished. A body that never finishes or is dropped on navigation is
 * not waited for: its path and type are recorded instead. The driver accepts only event streams (their frames are
 * the events it searches through the API and in the database dump; the API's event stream is left to the page, as
 * a fetch for it would never end) and server-component payloads there (run 9). Those payloads are build output only
 * while the web server never sees request data: scripts/server-boundary.mjs checks that in `pnpm check` (only the
 * listed dynamic routes, each a client component; no next/headers, middleware, route handlers or server actions), so
 * the exemption cannot silently hide a future leak.
 */
export async function watchResponses(page: Page, forms: string[]) {
  const seen = {
    responses: 0,
    found: new Set<string>(),
    pending: [] as Promise<void>[],
    open: new Map<Request, string>(),
    cut: [] as string[],
    /** The page's own requests from their start until their response or failure: a flush waits for them too. */
    started: new Set<Request>(),
    /** Requests whose bodies were searched before the page got them: a later cancel by the page hides nothing. */
    searched: new Set<Request>(),
  };
  const search = (body: string) => {
    seen.responses += 1;
    for (const form of forms) if (body.includes(form)) seen.found.add(form);
  };
  await page.route(
    // The build's own scripts too (connector run 5x-2: a chunk's body was evicted before it was read).
    (url) =>
      (url.pathname.startsWith("/api/") && !url.pathname.endsWith("/events")) ||
      url.pathname.startsWith("/_next/static/"),
    async (route) => {
      const response = await route.fetch().catch(() => null);
      // No answer at all (the page closed, or the server did not reply): the request fails and delivers no body.
      if (response === null) return route.abort().catch(() => {});
      const type = response.headers()["content-type"] ?? "";
      if (/json|text|javascript/.test(type)) {
        search(await response.text());
        seen.searched.add(route.request());
      }
      // The page may have cancelled the request meanwhile; its body was searched all the same.
      await route.fulfill({ response }).catch(() => {});
    },
  );
  const label = (request: Request, type: string) =>
    `${request.method()} ${new URL(request.url()).pathname} ${type}`;
  page.on("request", (request) => {
    // A server-component request is not waited for (profile run 5x-1: a prefetch the page dropped while the route
    // held it never answered or failed); its payload may stay unsearched anyway, and any answer is still recorded.
    if (
      ["fetch", "xhr", "document"].includes(request.resourceType()) &&
      request.headers()["rsc"] !== "1"
    )
      seen.started.add(request);
  });
  page.on("response", (response) => {
    seen.started.delete(response.request());
    if (seen.searched.has(response.request())) return;
    const type = response.headers()["content-type"] ?? "";
    if (/json|text|javascript/.test(type))
      seen.open.set(response.request(), label(response.request(), type));
  });
  page.on("requestfinished", (request) => {
    const entry = seen.open.get(request);
    if (entry === undefined) return;
    seen.open.delete(request);
    seen.pending.push(
      request
        .response()
        .then((response) => response?.text())
        .then(
          (body = "") => search(body),
          // The browser dropped the body before it was read (a navigation, run 8): recorded, never skipped silently.
          () => {
            seen.cut.push(`${entry} (evicted)`);
          },
        ),
    );
  });
  page.on("requestfailed", (request) => {
    seen.started.delete(request);
    const entry = seen.open.get(request);
    if (entry === undefined) return;
    seen.open.delete(request);
    seen.cut.push(entry);
  });
  return seen;
}

type Watch = Awaited<ReturnType<typeof watchResponses>>;

/** Reads every body before the page moves on or the test ends, so a navigation cannot drop one unread. */
export async function flushed(seen: Watch) {
  // Bodies still loading are waited for too (runs 10-11: API responses in flight at a navigation or at the end);
  // only the kinds the driver accepts may stay open.
  // Requests not yet answered count too (profile run 1): the page's first fetches had not answered when the flush
  // ran, and the next navigation cut their bodies off after the headers, so neither finish nor failure ever came.
  await expect
    .poll(() => [
      ...[...seen.started].map(
        (request) => `${request.url()} (no response yet)`,
      ),
      ...[...seen.open.values()].filter(
        (entry) => !/event-stream|x-component/.test(entry),
      ),
    ])
    .toEqual([]);
  await Promise.all(seen.pending);
}

/**
 * The last search: the page may start no new request, then every body already received is read (profile run 3: a Files
 * refresh answered after the final flush's last check). An aborted request delivers no body, so nothing is left unread.
 */
export async function finalFlush(page: Page, seen: Watch) {
  await page.route("**/*", (route) => route.abort());
  await flushed(seen);
}

export async function absentFromPage(page: Page, forms: string[]) {
  const content = await page.content();
  expect(
    forms.filter((form) => content.includes(form)),
    "The credential reached the page",
  ).toEqual([]);
}

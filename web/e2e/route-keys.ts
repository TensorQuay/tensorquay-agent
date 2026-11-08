import { randomUUID } from "node:crypto";

import { expect, type Page } from "@playwright/test";

/**
 * Allows the shared key `keyRef` for route `name`, as an administrator does on the Keys page: a route naming a shared
 * key that isn't bound to it can't be saved (`409 secret_not_allowed`). Removing the route unbinds it.
 * `origin` defaults to the page's own, for a page that has navigated; a spec that writes before it navigates names it.
 */
export async function allowKey(
  page: Page,
  keyRef: string,
  name: string,
  origin = new URL(page.url()).origin,
) {
  const listed = await page.request.get("/api/v1/admin/secrets");
  expect(listed.ok(), `shared keys: ${listed.status()}`).toBe(true);
  const { items } = (await listed.json()) as {
    items: {
      ref: string;
      version: number;
      consumers?: { kind: string; name: string }[];
    }[];
  };
  const key = items.find((item) => item.ref === keyRef);
  expect(key, `the shared key ${keyRef}`).toBeDefined();
  const bound = await page.request.put(
    `/api/v1/admin/secrets/${encodeURIComponent(keyRef.replace(/^secrets\//, ""))}/consumers`,
    {
      headers: { Origin: origin, "Idempotency-Key": randomUUID() },
      data: {
        consumers: [...(key?.consumers ?? []), { kind: "route", name }],
        version: key?.version,
      },
    },
  );
  expect(
    bound.ok(),
    `allow ${keyRef} for ${name}: ${bound.status()} ${bound.ok() ? "" : await bound.text()}`,
  ).toBe(true);
}

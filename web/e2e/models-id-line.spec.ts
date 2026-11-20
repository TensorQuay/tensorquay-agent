import { signIn } from "./session";
import { expect, test } from "./test";

// WEB-E2E-144 (18.1.1, the first hour's finding 5): on Settings > Models, a model's id has its own line before its
// Verified pill, so a long language's wider menu never cuts it (Spanish, where the first hour saw "qwen3.8-…").
for (const width of [1280, 1440] as const)
  test(`WEB-E2E-144 a model's id is never cut beside its pill, Spanish at ${width}`, async ({
    page,
  }) => {
    await page.setViewportSize({ width, height: 900 });
    await page.addInitScript(() => {
      try {
        localStorage.setItem("tensorquay-language", "es");
      } catch {
        // The browser's own language stays; the test then reads English.
      }
    });
    // A real install's ids are long (an organisation, a size and a quantisation): each route's model is answered so.
    await page.route(
      (url) => url.pathname.startsWith("/api/v1/"),
      async (route) => {
        if (route.request().method() !== "GET") return route.fallback();
        const response = await route.fetch();
        if (!(response.headers()["content-type"] ?? "").includes("json"))
          return route.fulfill({ response });
        const long = (value: unknown): unknown => {
          if (Array.isArray(value)) return value.map(long);
          if (!value || typeof value !== "object") return value;
          const entries = Object.entries(value).map(([key, inner]) => [
            key,
            long(inner),
          ]);
          const object = Object.fromEntries(entries) as Record<string, unknown>;
          if (
            typeof object.protocol === "string" &&
            typeof object.model === "string"
          )
            object.model = "Qwen/Qwen3.8-27B-Instruct-AWQ";
          return object;
        };
        await route.fulfill({ response, json: long(await response.json()) });
      },
    );
    await signIn(page);
    await page.goto("/settings/models");
    const ids = page
      .locator("tbody tr td:nth-child(2)")
      .getByText("Qwen/Qwen3.8-27B-Instruct-AWQ", { exact: true });
    await expect(ids.first()).toBeVisible();
    const layout = await ids.evaluateAll((elements) =>
      elements.map((element) => {
        const id = element.getBoundingClientRect();
        const pill = element
          .closest("td")
          ?.querySelector(".status-pill")
          ?.getBoundingClientRect();
        return {
          cut: element.scrollWidth > element.clientWidth + 1,
          pillUnder: pill ? pill.top >= id.bottom - 1 : true,
        };
      }),
    );
    // Whole (it wraps if it must, never "…"), and its pill under it, not beside it.
    expect(layout.filter((each) => each.cut)).toEqual([]);
    expect(layout.filter((each) => !each.pillUnder)).toEqual([]);
  });

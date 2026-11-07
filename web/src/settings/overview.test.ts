import { describe, expect, it, vi } from "vitest";

vi.mock("./requests", () => ({
  settingsApi: {
    readiness: vi.fn(() =>
      Promise.resolve({ ready: true, checks: { database: true } }),
    ),
    invitations: vi.fn(() =>
      Promise.resolve({
        items: [
          { status: "pending" },
          { status: "accepted" },
          { status: "pending" },
        ],
      }),
    ),
  },
}));
vi.mock("../api/client", () => ({
  api: {
    users: vi.fn(() =>
      Promise.resolve({
        items: [{ disabled: false }, { disabled: true }, { disabled: false }],
      }),
    ),
  },
}));
vi.mock("./admin/gateway", () => ({
  gatewayRoutes: {
    routes: vi.fn(() =>
      Promise.resolve({ items: [{ name: "local" }], generation: 1 }),
    ),
  },
  gatewayEgress: {
    rules: vi.fn(() =>
      Promise.resolve({ disabled: false, items: [{}, {}], limit: 100 }),
    ),
  },
  gatewayAdmin: {
    secrets: vi.fn(() =>
      Promise.resolve({
        items: [{ rotation_due: true }, { rotation_due: false }],
        limit: 50,
      }),
    ),
  },
}));

import { gatewayOverview } from "./overview";

// WEB-U-077: the overview reads the endpoints that already exist and sums them up.
describe("WEB-U-077 the overview's reads", () => {
  it("counts active people and waiting invitations, allowed sites and keys due", async () => {
    expect(await gatewayOverview.readiness()).toEqual({
      ready: true,
      checks: { database: true },
    });
    expect(await gatewayOverview.routes()).toEqual([{ name: "local" }]);
    expect(await gatewayOverview.people()).toEqual({ active: 2, invited: 2 });
    expect(await gatewayOverview.internetAccess()).toEqual({
      disabled: false,
      rules: 2,
    });
    expect(await gatewayOverview.rotationDue()).toBe(1);
  });
});

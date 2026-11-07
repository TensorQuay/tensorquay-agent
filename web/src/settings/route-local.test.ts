import { describe, expect, it } from "vitest";

import { route } from "../test/routes";

import { containerHostAddress, localRoute, routePlace } from "./route-local";
import cases from "./route-place-cases.json";

// WEB-U-095: the gateway gives a local route's check longer (GW-U-092), so the page says so; both use the same rule.
// The addresses are the gateway's own cases (GW-U-094 reads this table too), so the two copies can't drift apart.
describe("WEB-U-095 which routes are local", () => {
  it("has the shared cases", () => {
    expect(cases.length).toBeGreaterThanOrEqual(30);
  });

  it.each(cases)("places $url as $place", ({ url, place }) => {
    expect(routePlace(url)).toBe(place);
  });

  it.each(cases)(
    "counts a keyed route at $url as local unless hosted",
    ({ url, place }) => {
      expect(localRoute(route("r", { base_url: url }))).toBe(
        place !== "hosted",
      );
    },
  );

  it("counts a keyed route with no address as elsewhere", () => {
    expect(localRoute(route("r", { base_url: "" }))).toBe(false);
  });

  it("counts a route without a key as local wherever it is", () => {
    expect(
      localRoute(
        route("r", {
          base_url: "https://models.example/v1",
          auth: { kind: "none" },
        }),
      ),
    ).toBe(true);
  });
});

// WEB-U-281: a loopback address with only its host changed to the container host's name; anything else, none.
describe("WEB-U-281 the container host's address", () => {
  it.each([
    ["http://localhost:11434/v1", "http://host.docker.internal:11434/v1"],
    ["http://127.0.0.1:8000/v1/", "http://host.docker.internal:8000/v1/"],
    ["http://[::1]:1234/v1", "http://host.docker.internal:1234/v1"],
    ["HTTP://LocalHost/v1", "HTTP://host.docker.internal/v1"],
    ["http://app.localhost:8080", "http://host.docker.internal:8080"],
  ])("replaces %s's host", (address, replaced) => {
    expect(containerHostAddress(address)).toBe(replaced);
  });

  it.each([
    "http://host.docker.internal:11434/v1",
    "http://host.containers.internal:11434/v1",
    "http://ollama:11434/v1",
    "http://gpu-box.local:11434/v1",
    "https://api.example.invalid/v1",
    "not an address",
  ])("leaves %s alone", (address) => {
    expect(containerHostAddress(address)).toBeNull();
  });
});

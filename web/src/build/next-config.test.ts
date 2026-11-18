// @vitest-environment node
// WEB-U-491 (18.0.1): no build asks the registry for upgrades or stops itself to recommend one (Next.js's agent
// upgrade prompt, which stopped the 18.0.1 check's build): dependency upgrades are the team's own decision.
import { describe, expect, it } from "vitest";

import nextConfig from "../../next.config";

describe("WEB-U-491 Next's upgrade prompt is off", () => {
  it("sets experimental.agentUpgrade to false", () => {
    expect(nextConfig.experimental?.agentUpgrade).toBe(false);
  });
});

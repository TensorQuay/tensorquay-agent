import { describe, expect, it, vi } from "vitest";

import { areaLoader } from "./area-catalogue";
import { english, hasReference } from "./translate";

const catalogue = (messages: Record<string, string>) => ({
  default: { messages },
});

// An area's English downloads once and serves every later view; a failed download is tried again (WEB-U-014).
describe("WEB-U-014 area catalogue loader", () => {
  it("downloads an area's copy once and makes it available by key", async () => {
    const download = vi
      .fn()
      .mockResolvedValue(catalogue({ "probe.once": "Loaded once" }));
    const load = areaLoader("probe-once", download);
    expect(hasReference("probe-once")).toBe(false);
    await Promise.all([load(), load()]);
    await load();
    expect(download).toHaveBeenCalledOnce();
    expect(hasReference("probe-once")).toBe(true);
    expect(english("probe.once" as never)).toBe("Loaded once");
  });

  it("tries the download again after a failure, and keeps the result once it succeeds", async () => {
    const download = vi
      .fn()
      .mockRejectedValueOnce(new Error("offline"))
      .mockResolvedValue(catalogue({ "probe.retry": "Loaded on retry" }));
    const load = areaLoader("probe-retry", download);
    await expect(load()).rejects.toThrow("offline");
    expect(hasReference("probe-retry")).toBe(false);
    await load();
    await load();
    expect(download).toHaveBeenCalledTimes(2);
    expect(english("probe.retry" as never)).toBe("Loaded on retry");
  });
});

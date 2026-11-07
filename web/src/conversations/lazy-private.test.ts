// @vitest-environment node
import { readFileSync } from "node:fs";

import { describe, expect, it } from "vitest";

// The conversation route's budget (WEB-U-177): an administrator's view of another person's task is rare on the task
// page, so its code loads when it is shown, not with the page.
describe("WEB-U-177 the private task view loads when it is shown", () => {
  it("isn't imported statically by the task page", () => {
    const page = readFileSync(
      new URL("./ConversationPage.tsx", import.meta.url),
      "utf8",
    );
    expect(page).not.toMatch(/from "\.\/PrivateTask"/);
    expect(page).toMatch(/import\("\.\/PrivateTask"\)/);
  });
});

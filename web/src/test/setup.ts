import "@testing-library/jest-dom/vitest";
import { cleanup, configure } from "@testing-library/react";
import { afterEach, beforeAll } from "vitest";

import { parseScale, unitCaps } from "../../deadline-scale";
import context from "../i18n/generated/en.context.json";
import failures from "../i18n/generated/en.failures.json";
import settings from "../i18n/generated/en.settings.json";
import { addReference } from "../i18n/translate";

import { countNotifications, notificationsSettled } from "./notify";

// Component tests render settings pages and the context view without their loaders; add their copy once, as those do. Only the
// translator is imported here, so a test file's own mocks of UI modules still apply.
addReference("settings", settings.messages);
addReference("context", context.messages);
// The failure copy loads on its first use in the app (WEB-U-392); failure-catalogue.test.tsx checks that load itself.
addReference("failures", failures.messages);

// A findBy… or waitFor's cap scales with TQA_TEST_DEADLINE_SCALE (1 s, or 4 s in the check), so a host stalled by other
// jobs (7 Oct 2026, WEB-U-074) doesn't fail a wait that was on its way, and a plain run still catches one that is late.
configure({
  asyncUtilTimeout: unitCaps(parseScale(process.env.TQA_TEST_DEADLINE_SCALE))
    .asyncUtilMs,
});

// A menu's open state loads when it is wanted (WEB-U-014's budget), and component tests read a menu the moment it opens:
// its code is fetched before each file's tests, after the file's own mocks, as the person's pointer would have
// (lazy-menus.test.tsx checks the fetch itself).
beforeAll(async () => {
  await (await import("../ui/lazy-menus")).loadMenus();
});

// After each test's unmount, its queued query notifications run while its environment is still there (§19a, WEB-U-195).
countNotifications();
afterEach(async () => {
  cleanup();
  await notificationsSettled();
});

// jsdom has no native dialog methods; browser tests verify native focus trapping.
if (typeof HTMLDialogElement !== "undefined")
  Object.defineProperties(HTMLDialogElement.prototype, {
    showModal: {
      configurable: true,
      value: function (this: HTMLDialogElement) {
        this.setAttribute("open", "");
      },
    },
    close: {
      configurable: true,
      value: function (this: HTMLDialogElement) {
        this.removeAttribute("open");
      },
    },
  });

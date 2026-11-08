import { randomUUID } from "node:crypto";

import { signIn } from "./session";
import { axe, capture, type } from "./settings-page";
import { expect, test } from "./test";

// Channels on the fake front (ADR-0014; WEB-U-062 and WEB-U-063 browser part), keyboard first, at the widths
// UI-GUIDELINES §17.2 asks for in both themes: the sidebar's Channels, the list, a channel's log and a reply, and a new
// channel. Each run posts to the seeded channel and creates its own.
for (const width of [1024, 1280] as const)
  for (const colorScheme of ["light", "dark"] as const)
    test(`WEB-U-062/063 channels ${width} ${colorScheme}`, async ({ page }) => {
      await page.setViewportSize({ width, height: 900 });
      await page.emulateMedia({ colorScheme });
      await signIn(page);
      await page.goto("/");
      await page.getByRole("link", { name: "Channels" }).click();
      await expect(page).toHaveTitle("Channels · TensorQuay Agent");
      const list = page.getByRole("list", { name: "Channels" });
      const release = list.getByRole("listitem").filter({
        has: page.getByRole("link", { name: "release", exact: true }),
      });
      await expect(release).toContainText("Decided by majority");
      await axe(page);
      await capture(page, `channels-list-${width}-${colorScheme}`);

      await release.getByRole("link", { name: "release", exact: true }).click();
      await expect(
        page.getByRole("heading", { level: 1, name: "release" }),
      ).toBeVisible();
      const log = page.getByRole("list", { name: "Channel log" });
      await expect(
        log.getByRole("article", { name: "#4 Decision" }),
      ).toContainText("Accepted by majority");
      await expect(
        page
          .getByRole("list", { name: "Replies to #7" })
          .getByRole("article", { name: /^#8 / }),
      ).toBeVisible();
      // A line names the proposal or task it is about, linked to its post, never "#2" (WEB-U-165).
      await expect(log.getByRole("article", { name: /^#3 / })).toContainText(
        "voted Accept on Ship on Friday",
      );
      await expect(
        log
          .getByRole("article", { name: "#4 Decision" })
          .getByRole("link", { name: "Ship on Friday" }),
      ).toHaveAttribute("href", "#post-2");
      await expect(log.getByRole("article", { name: /^#6 / })).toContainText(
        "took on Write the changelog",
      );
      await axe(page);
      await capture(page, `channel-log-${width}-${colorScheme}`);

      // A post from elsewhere arrives on the channel's stream, without a reload (G8).
      const channelId = new URL(page.url()).pathname.split("/").at(-1) ?? "";
      const elsewhere = "The 2.4 build is green on main.";
      const posted = await page.request.post(
        `/api/v1/channels/${channelId}/messages`,
        {
          headers: { "Idempotency-Key": randomUUID() },
          data: { body: { type: "text", text: elsewhere } },
        },
      );
      expect(posted.status()).toBe(201);
      await expect(log).toContainText(elsewhere);

      await page
        .getByRole("button", { name: "Reply to #1", exact: true })
        .focus();
      await page.keyboard.press("Enter");
      const form = page.getByRole("form", { name: "Post to the channel" });
      await expect(form.getByText("Replying to #1")).toBeVisible();
      const note = "Noted. I'll check the changelog next.";
      await form.getByLabel("Your message").fill(note);
      await form.getByRole("button", { name: "Post" }).click();
      const replies = page.getByRole("list", {
        name: "Replies to #1",
        exact: true,
      });
      await expect(replies).toContainText(note);
      await expect(form.getByText("Replying to #1")).toHaveCount(0);

      // The seeded proposal is decided; one this test makes takes a vote (WEB-U-064).
      await expect(
        page.getByRole("group", { name: "Votes on #2", exact: true }),
      ).toContainText("Accepted by majority");
      const ask =
        "Ship 2.4 today, once the changelog, the release notes and the migration guide are merged?";
      await form.getByRole("button", { name: "Propose…" }).click();
      await form.getByLabel("Title").fill(ask);
      await form.getByLabel("What and why").fill("All checks are green.");
      await form.getByRole("button", { name: "Post" }).click();
      const asked = page.getByRole("article").filter({ hasText: ask });
      const votes = asked.getByRole("group", { name: /^Votes on #/ });
      await expect(votes).toContainText("Accept 0 · Reject 0");
      await votes.getByRole("button", { name: /^Accept #/ }).click();
      await expect(votes).toContainText("Accept 1 · Reject 0");
      await expect(votes).toContainText("You voted Accept.");
      // The chosen vote is marked (WEB-U-166); the vote's line cuts the long title, whole in its tooltip (WEB-U-165).
      await expect(
        votes.getByRole("button", { name: /^Accept #/ }),
      ).toHaveAttribute("aria-pressed", "true");
      await expect(
        votes.getByRole("button", { name: /^Reject #/ }),
      ).toHaveAttribute("aria-pressed", "false");
      const named = log
        .getByRole("article")
        .filter({ hasText: "voted Accept on" })
        .last()
        .getByRole("link", { name: ask });
      await expect(named).toHaveAttribute("title", ask);
      expect(
        await named.evaluate(
          (link) =>
            link.scrollWidth > link.clientWidth &&
            link.getBoundingClientRect().height <
              2 * parseFloat(getComputedStyle(link).fontSize),
        ),
      ).toBe(true);
      await axe(page);
      await votes.scrollIntoViewIfNeeded();
      await capture(page, `channel-voted-${width}-${colorScheme}`);

      // A task this test adds, claimed from the board (WEB-U-065).
      const title = "Tag the release";
      await form.getByRole("button", { name: "Add a to-do…" }).click();
      await form.getByLabel("Title").fill(title);
      await form
        .getByLabel("What is to be done")
        .fill("Tag v2.4.0 once the changelog is merged.");
      await form.getByRole("button", { name: "Post" }).click();
      const board = page.getByRole("list", { name: "To-dos" });
      const task = board.getByRole("listitem", { name: title });
      await expect(task).toContainText("Open · 1 place");
      await task.getByRole("button", { name: /^Take on #/ }).click();
      await expect(task).toContainText("1 of 1 places taken");
      await expect(task).toContainText("(you): Working");
      await axe(page);
      await capture(page, `channel-board-${width}-${colorScheme}`);
      await page.getByRole("tab", { name: "Members" }).click();
      await expect(page.getByRole("list", { name: "Members" })).toContainText(
        "Coder · ana",
      );
      // Each agent's run status (WEB-U-065, GW-I-113), checked in both themes.
      await expect(
        page
          .getByRole("list", { name: "Members" })
          .getByRole("listitem", { name: "Reviewer · ben" }),
      ).toContainText("Done");
      // WEB-U-426: above the box, the agents that aren't running. Reviewer has finished and isn't the viewer's to open,
      // so it is named without a link; Coder is working and isn't listed.
      const idle = page.getByRole("region", { name: "1 agent isn’t running" });
      await expect(idle).toBeVisible();
      await expect(idle).toContainText(
        "An agent reads this channel only while its task runs: open its task and ask it to work here, or ask its owner when the agent isn’t yours.",
      );
      await expect(idle).toContainText("Reviewer · ben");
      await expect(idle.getByRole("link")).toHaveCount(0);
      await expect(idle).not.toContainText("Coder");
      await axe(page);
      // The owner's request: an agent's name opens its task, when the viewer may open it (the gateway's can_open).
      const members = page.getByRole("list", { name: "Members" });
      await expect(
        members
          .getByRole("listitem", { name: "Reviewer · ben" })
          .getByRole("link"),
      ).toHaveCount(0);
      const coder = members.getByRole("link", { name: "Coder · ana" });
      await expect(coder).toHaveAttribute(
        "href",
        "/c/0195d8c0-0000-7000-8000-00000000c0de",
      );
      const back = page.url();
      await coder.click();
      await expect(page).toHaveURL(
        /\/c\/0195d8c0-0000-7000-8000-00000000c0de$/,
      );
      await page.goto(back);
      await page.getByRole("tab", { name: "Members" }).click();

      await page.getByRole("link", { name: "Channels" }).click();
      await page.getByRole("button", { name: "New channel" }).first().click();
      const dialog = page.getByRole("dialog", { name: "New channel" });
      // One question first; the title follows it (UI-GUIDELINES §0).
      const channelTitle = "Release review";
      await type(
        page,
        "What's this channel for?",
        `${channelTitle}. Check the release.`,
      );
      await expect(
        dialog.getByText(channelTitle, { exact: true }),
      ).toBeVisible();
      // A two-line box, not a one-line field (a textarea's rows are kept).
      const purposeBox = await dialog
        .getByLabel("What's this channel for?")
        .boundingBox();
      expect(purposeBox?.height ?? 0).toBeGreaterThan(50);
      // Its first member, a person by exact email in the one search box (WEB-U-062).
      const search = dialog.getByRole("combobox", { name: "Who's in it" });
      await search.focus();
      await page.keyboard.type("colleague@example.invalid");
      await expect(
        page.getByRole("option", { name: "Add colleague@example.invalid" }),
      ).toBeVisible();
      await page.keyboard.press("Enter");
      await expect(
        dialog
          .getByRole("list", { name: "Who's in it" })
          .getByRole("listitem", { name: "colleague@example.invalid" }),
      ).toBeVisible();
      await dialog.getByRole("button", { name: "More options" }).click();
      await dialog.getByRole("radio", { name: "A person decides" }).check();
      await axe(page);
      await capture(page, `channel-new-${width}-${colorScheme}`);
      await dialog.getByRole("button", { name: "Create channel" }).click();
      await expect(
        page.getByRole("heading", { level: 1, name: channelTitle }),
      ).toBeVisible();
      await page.getByRole("tab", { name: "Members" }).click();
      await expect(
        page
          .getByRole("list", { name: "Members" })
          .getByRole("listitem", { name: "colleague" }),
      ).toBeVisible();
      // A plain member carries no role badge.
      await expect(
        page
          .getByRole("list", { name: "Members" })
          .getByRole("listitem", { name: "colleague" }),
      ).not.toContainText(/Lead|Reads only|Member/);
      await expect(page.getByText("Decided by a person")).toBeVisible();
      await expect(
        page.getByRole("form", { name: "Post to the channel" }),
      ).toBeVisible();
    });

// The owner's controls on a channel of this run's own (ADR-0014 points 4 and 13; WEB-U-066 browser part): the
// Members tab offers adding a task's agent, End leaves the channel read-only, and Delete returns to the list.
for (const [width, colorScheme] of [
  [1024, "dark"],
  [1280, "light"],
] as const)
  test(`WEB-U-066 channel membership, end and delete ${width} ${colorScheme}`, async ({
    page,
  }) => {
    await page.setViewportSize({ width, height: 900 });
    await page.emulateMedia({ colorScheme });
    await signIn(page);
    await page.goto("/channels");
    const name = "Release checklist";
    await page.getByRole("button", { name: "New channel" }).first().click();
    const dialog = page.getByRole("dialog", { name: "New channel" });
    await type(
      page,
      "What's this channel for?",
      `${name}. Try the owner's controls.`,
    );
    await dialog.getByRole("button", { name: "Create channel" }).click();
    await expect(page.getByRole("heading", { level: 1, name })).toBeVisible();

    await page.getByRole("tab", { name: "Members" }).click();
    await expect(
      page
        .getByRole("list", { name: "Members" })
        .getByRole("listitem", { name: /\(you\)$/ }),
    ).toContainText("Lead");
    // "+ Add" opens one search box: an unknown address first, then the colleague, who joins as a member.
    await page.getByRole("button", { name: "Add", exact: true }).click();
    const box = page.getByRole("combobox", { name: "Add to the channel" });
    await expect(box).toBeFocused();
    await box.fill("nobody@example.invalid");
    await box.press("Enter");
    await expect(page.getByRole("tabpanel").getByRole("alert")).toContainText(
      "No account uses that email.",
    );
    await box.fill("colleague@example.invalid");
    await box.press("Enter");
    await expect(page.getByRole("status")).toContainText(
      "Added colleague@example.invalid.",
    );
    const colleague = page
      .getByRole("list", { name: "Members" })
      .getByRole("listitem", { name: "colleague" });
    // A role and Remove are in the row's "…" menu.
    await colleague
      .getByRole("button", { name: "More actions for colleague" })
      .click();
    await page.getByRole("menuitem", { name: "Make read-only" }).click();
    await expect(colleague).toContainText("Reads only");
    await axe(page);
    await capture(page, `channel-members-${width}-${colorScheme}`);
    await colleague
      .getByRole("button", { name: "More actions for colleague" })
      .click();
    await page.getByRole("menuitem", { name: "Remove" }).click();
    await page
      .getByRole("dialog", { name: "Remove colleague from the channel?" })
      .getByRole("button", { name: "Remove" })
      .click();
    await expect(colleague).toHaveCount(0);

    const bar = page.getByRole("region", { name: "Channel header" });
    // End and Delete sit in the header's "…" menu; the keyboard opens it and lands on its first item.
    await bar.getByRole("button", { name: `More actions for ${name}` }).focus();
    await page.keyboard.press("Enter");
    await expect(
      page.getByRole("menuitem", { name: "End channel" }),
    ).toBeFocused();
    await page.keyboard.press("Enter");
    const end = page.getByRole("dialog", { name: `End ${name}?` });
    await axe(page);
    await end.getByRole("button", { name: "End channel" }).click();
    await expect(
      page.getByText("This channel has ended; it can be read, not added to."),
    ).toBeVisible();
    await expect(bar.getByText("Ended")).toBeVisible();
    await expect(
      page.getByRole("button", { name: "Add", exact: true }),
    ).toHaveCount(0);

    await bar.getByRole("button", { name: `More actions for ${name}` }).click();
    await page.getByRole("menuitem", { name: "Delete channel" }).click();
    await page
      .getByRole("dialog", { name: `Delete ${name}?` })
      .getByRole("button", { name: "Delete channel" })
      .click();
    await expect(page).toHaveTitle("Channels · TensorQuay Agent");
    await expect(
      page
        .getByRole("list", { name: "Channels" })
        .getByRole("link", { name, exact: true }),
    ).toHaveCount(0);
  });

// A post's quiet line (seq, time, Reply) shares the author's line at a phone's width instead of leaving a blank row
// under it, and a proposal's tally bar runs the width of its votes (WEB-U-063/064).
test("WEB-U-063/064 post rows at 390 px, and the tally bar at every width", async ({
  page,
}) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await signIn(page);
  await page.goto("/channels/0195d8c0-0000-7000-8000-00000000c4a1");
  const first = page.getByRole("article", { name: /^#1 / });
  const who = await first
    .locator(":scope > .channel-post-head .type-body-strong")
    .boundingBox();
  const meta = await first
    .locator(":scope > .channel-post-head > .channel-post-meta")
    .boundingBox();
  expect(who && meta).toBeTruthy();
  expect(Math.abs((meta?.y ?? 0) - (who?.y ?? 0))).toBeLessThan(12);
  const votes = page.getByRole("group", { name: "Votes on #2", exact: true });
  const bar = await votes.locator(".proposal-bar").boundingBox();
  const group = await votes.boundingBox();
  expect(bar?.width ?? 0).toBeGreaterThan((group?.width ?? 0) * 0.9);
  // And on a wide screen, where a capped bar stopped short of the counts.
  await page.setViewportSize({ width: 1280, height: 900 });
  const wide = await votes.locator(".proposal-bar").boundingBox();
  const wideGroup = await votes.boundingBox();
  expect(wide?.width ?? 0).toBeGreaterThan((wideGroup?.width ?? 0) * 0.9);
});

// The shared "…" tooltip stays inside the window and the side panel, isn't shown over an open menu, and a row's menu
// doesn't cover the "+ Add" box (UI-GUIDELINES §5b; WEB-U-066).
for (const width of [1280, 390] as const)
  test(`WEB-U-066 member menu and tooltip stay inside the panel at ${width}`, async ({
    page,
  }) => {
    await page.setViewportSize({ width, height: 900 });
    await signIn(page);
    await page.goto("/channels");
    await page.getByRole("button", { name: "New channel" }).first().click();
    const dialog = page.getByRole("dialog", { name: "New channel" });
    const name = "Design review";
    await dialog.getByLabel("What's this channel for?").fill(name);
    await dialog.getByRole("button", { name: "Create channel" }).click();
    await expect(page.getByRole("heading", { level: 1, name })).toBeVisible();
    await page.getByRole("tab", { name: "Members" }).click();
    await page.getByRole("button", { name: "Add", exact: true }).click();
    const box = page.getByRole("combobox", { name: "Add to the channel" });
    await box.fill("colleague@example.invalid");
    await box.press("Enter");
    const row = page
      .getByRole("list", { name: "Members" })
      .getByRole("listitem", { name: "colleague" });
    const more = row.getByRole("button", {
      name: "More actions for colleague",
    });
    await more.hover();
    // The tip is a layer of its own on the page (WEB-U-429), so the panel is found by itself, not as its ancestor.
    const tip = page.locator(".action-tooltip", { hasText: "More actions" });
    await expect(tip).toBeVisible();
    const inside = await tip.evaluate((element) => {
      const r = element.getBoundingClientRect();
      const side = document
        .querySelector(".channel-side")
        ?.getBoundingClientRect();
      return {
        window: r.left >= 0 && r.right <= innerWidth,
        panel: !side || (r.left >= side.left && r.right <= side.right),
      };
    });
    expect(inside).toEqual({ window: true, panel: true });
    await more.click();
    const menu = page.getByRole("menu");
    await expect(menu).toBeVisible();
    await expect(tip).toBeHidden();
    // The "+ Add" box closes when a row's menu opens, so the menu covers nothing being typed in.
    await expect(box).toHaveCount(0);
    const [m, panel] = await Promise.all([
      menu.boundingBox(),
      page.locator(".channel-side").boundingBox(),
    ]);
    expect((m?.x ?? 0) >= 0 && (m?.x ?? 0) + (m?.width ?? 0) <= width).toBe(
      true,
    );
    expect(panel).toBeTruthy();
  });

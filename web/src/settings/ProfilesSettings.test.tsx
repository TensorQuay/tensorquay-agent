import { fireEvent, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import type { User } from "../protocol/generated/api";
import { choose, picker } from "../test/picker";
import { showProfiles } from "../test/profile-app";
import { fakeProfiles, profile } from "../test/profiles";
import { LIMITS } from "../test/user";

import type { ProfilesApi } from "./profiles";
import type { ChoicesApi } from "./ProfilesSettings";

/** Replaced by the harness once it renders (showProfiles). */
const router = vi.hoisted(() => ({ push: vi.fn<(href: string) => void>() }));
vi.mock("next/navigation", () => ({ useRouter: () => router }));
vi.mock("next/link", async () => {
  const { createElement } = await import("react");
  return {
    default: ({ href, children, ...rest }: Record<string, unknown>) =>
      createElement("a", { href, ...rest }, children as never),
  };
});

const member: User = {
  id: "0195d8c0-0000-7000-8000-000000000002",
  email: "member@example.invalid",
  role: "member",
  internet: "none",
  limits: LIMITS,
  features: { agent_teams: false },
};
const admin: User = { ...member, role: "admin" };
const choices: ChoicesApi = {
  models: () =>
    Promise.resolve({
      items: [
        {
          name: "local",
          model: "local-model",
          protocol: "openai_chat",
          display_name: "Local model",
          description: null,
          verified: null,
          context_window: 32768,
          vision: false,
          native_tools: true,
          runs_on: { where: "hosted", provider: null },
          reasoning: null,
        },
      ],
    }),
  templates: () =>
    Promise.resolve({
      items: [
        {
          name: "workstation",
          display_name: "Workstation",
          description: null,
          capabilities: ["shell"],
          is_default: true,
          cpus: 1,
          memory_mib: 512,
        },
      ],
    }),
  connectors: () =>
    Promise.resolve({
      items: [
        {
          name: "tracker",
          display_name: "Issue tracker",
          scheme: "header",
          required: true,
          has_credential: true,
          credential_updated_at: null,
        },
      ],
    }),
};
const LISTED = [
  profile("review", { display_name: "Code review", description: "Reads only" }),
  profile("team", { display_name: "Team default", shared: true }),
  profile("legacy", {
    display_name: "Legacy",
    shared: true,
    usable: false,
    unusable_field: "template",
  }),
];
function show(api: ProfilesApi, me: User = member) {
  return showProfiles(api, choices, me, router);
}
/** The profile's page (WEB-U-104): its form, from the title down to Save and Cancel. */
const page = () => {
  const form = screen
    .getByRole("group", { name: "Save or cancel" })
    .closest("form");
  if (!form) throw new Error("The page is a form");
  return form;
};
/** The open confirmation (Delete). */
const dialog = () => screen.getByRole("dialog");
/** Types into the field labelled `label`, or, for a standard picker (WEB-U-134), chooses the choice so worded. */
const set = (label: string, value: string) => {
  const box = within(page()).queryByLabelText(label);
  if (box) fireEvent.change(box, { target: { value } });
  else choose(label, value, page());
};
const save = () =>
  fireEvent.click(within(page()).getByRole("button", { name: "Save" }));
/** Follows a link on the list to a profile's page, and waits for it. */
async function go(link: string) {
  fireEvent.click(await screen.findByRole("link", { name: link }));
  await screen.findByRole("group", { name: "Save or cancel" });
}
/** Types the ID by hand: Change opens it, unless it is already open. */
function setId(id: string) {
  if (!within(page()).queryByLabelText("ID"))
    fireEvent.click(
      within(page()).getByRole("button", { name: "Change the ID" }),
    );
  set("ID", id);
}

// Settings → Profiles (ADR-0012, WEB-U-049): a person's own profiles and the shared ones, narrowing only.
describe("WEB-U-049 the profiles list", () => {
  // Say in one line what a profile is for, with an example that isn't only coding, and how it
  // differs from a model or an environment.
  it("says what an agent profile is for, with a general example", async () => {
    show(fakeProfiles({ profiles: LISTED }).api);
    await screen.findByRole("table", { name: "Shared agent profiles" });
    expect(
      screen.getByText(
        "An agent profile sets a task up for one kind of work, such as a Researcher with web access: its instructions, and the model, environment and tools it uses. Models and environments are what an administrator sets up; a profile chooses among them. Your own agent profiles are only yours; shared ones are for everyone.",
      ),
    ).toBeVisible();
  });

  // With nothing on either list, one empty state with the way to add one, not two empty headings.
  it("shows one empty state with Add agent profile when there are none at all", async () => {
    show(fakeProfiles({ profiles: [] }).api);
    const empty = await screen.findByRole("region", {
      name: "No agent profiles yet",
    });
    expect(empty).toHaveTextContent(
      "No agent profiles yet. Add one to start tasks set up for a kind of work.",
    );
    expect(
      within(empty).getByRole("link", { name: "Add agent profile" }),
    ).toBeVisible();
    expect(
      screen.getAllByRole("link", { name: "Add agent profile" }),
    ).toHaveLength(1);
    expect(
      screen.queryByRole("heading", { name: "Your agent profiles" }),
    ).toBeNull();
    expect(
      screen.queryByRole("heading", { name: "Shared agent profiles" }),
    ).toBeNull();
  });

  it("lists your profiles, then the shared ones, each saying whether it can be used and why not", async () => {
    show(fakeProfiles({ profiles: LISTED }).api);
    const yours = await screen.findByRole("table", {
      name: "Your agent profiles",
    });
    expect(within(yours).getByText("Code review")).toBeVisible();
    expect(within(yours).getByText("Reads only")).toBeVisible();
    const shared = screen.getByRole("table", { name: "Shared agent profiles" });
    const legacy = within(shared).getByRole("row", { name: /Legacy/ });
    expect(legacy).toHaveTextContent("Can't be used");
    expect(legacy).toHaveTextContent("Its environment can't be picked now.");
    expect(
      within(shared).getByRole("row", { name: /Team default/ }),
    ).toHaveTextContent("Ready");
    // The display name and description only: the internal name is not the reader's (as for connectors, S6).
    for (const name of ["review", "team", "legacy"])
      expect(screen.queryByText(name, { exact: true })).toBeNull();
  });

  it("shows members the shared ones read-only, with Duplicate; administrators may change them", async () => {
    show(fakeProfiles({ profiles: LISTED }).api);
    await screen.findByRole("table", { name: "Shared agent profiles" });
    expect(
      screen.queryByRole("link", { name: "Edit agent profile Team default" }),
    ).toBeNull();
    expect(
      screen.queryByRole("button", {
        name: "Delete agent profile Team default",
      }),
    ).toBeNull();
    expect(
      screen.getByRole("link", {
        name: "Duplicate agent profile Team default as your own",
      }),
    ).toBeVisible();
    expect(
      screen.getByRole("link", { name: "Edit agent profile Code review" }),
    ).toBeVisible();
    // A read-only row's one action is Duplicate; nothing else to put in a menu.
    expect(
      screen.queryByRole("button", {
        name: "More actions for agent profile Team default",
      }),
    ).toBeNull();
  });

  // One actions column (UI-GUIDELINES §7, as on Models and Keys): Edit, and the rest in a menu named after the row.
  it("gives a profile you can change Edit and a menu with Duplicate and Delete", async () => {
    show(fakeProfiles({ profiles: LISTED, admin: true }).api, admin);
    const shared = await screen.findByRole("table", {
      name: "Shared agent profiles",
    });
    const team = within(shared).getByRole("row", { name: /Team default/ });
    // Edit opens the profile's page (WEB-U-104), so it is a link; the rest are in the menu.
    expect(
      [...team.querySelectorAll("a, button")].map((control) =>
        control.getAttribute("aria-label"),
      ),
    ).toEqual([
      "Edit agent profile Team default",
      "More actions for agent profile Team default",
    ]);
    fireEvent.click(
      within(team).getByRole("button", {
        name: "More actions for agent profile Team default",
      }),
    );
    expect(
      screen
        .getAllByRole("menuitem")
        .map((item) => item.getAttribute("aria-label")),
    ).toEqual([
      "Duplicate agent profile Team default as your own",
      "Delete agent profile Team default",
    ]);
    fireEvent.keyDown(document.activeElement ?? document.body, {
      key: "Escape",
    });
    // Your own profile has no Duplicate: its menu holds Delete.
    const own = screen.getByRole("table", { name: "Your agent profiles" });
    fireEvent.click(
      within(own).getByRole("button", {
        name: "More actions for agent profile Code review",
      }),
    );
    expect(
      screen
        .getAllByRole("menuitem")
        .map((item) => item.getAttribute("aria-label")),
    ).toEqual(["Delete agent profile Code review"]);
  });

  it("lets administrators edit shared ones and offers them the shared switch", async () => {
    show(fakeProfiles({ profiles: LISTED, admin: true }).api, admin);
    await go("Edit agent profile Team default");
    expect(within(page()).queryByLabelText("Shared with everyone")).toBeNull();
    fireEvent.click(within(page()).getByRole("button", { name: "Cancel" }));
    await go("Add agent profile");
    expect(within(page()).getByLabelText("Shared with everyone")).toBeVisible();
  });
});

// PRD §12 item 22: nothing in the app is specific to git, so no profile offers to work from a project link (WEB-U-215).
// The gateway removed the field in this release and refuses it by name, so the web never sends it.
describe("WEB-U-049 no project links on a profile", () => {
  const team = profile("team", { display_name: "Team default", shared: true });

  it("offers no switch for them and never sends the field on save", async () => {
    const fake = fakeProfiles({ profiles: [team], admin: true });
    show(fake.api, admin);
    await go("Edit agent profile Team default");
    expect(page().textContent).not.toMatch(/\bgit\b|repositor|project link/i);
    save();
    expect(
      await screen.findByText("Saved agent profile Team default."),
    ).toBeVisible();
    expect(fake.writes.at(-1)?.fields).not.toHaveProperty("repositories");
  });

  it("creates a shared profile without them", async () => {
    const fake = fakeProfiles({ profiles: LISTED, admin: true });
    show(fake.api, admin);
    await go("Add agent profile");
    fireEvent.click(within(page()).getByLabelText("Shared with everyone"));
    expect(page().textContent).not.toMatch(/\bgit\b|repositor|project link/i);
    set("Display name", "Fresh team");
    setId("fresh");
    set("Model", "Local model");
    save();
    expect(await screen.findByText(/^Saved agent profile/)).toBeVisible();
    expect(fake.writes.at(-1)?.fields).not.toHaveProperty("repositories");
  });

  it("never carries them into a duplicate", async () => {
    const fake = fakeProfiles({ profiles: [team] });
    show(fake.api);
    await go("Duplicate agent profile Team default as your own");
    setId("mine");
    save();
    expect(await screen.findByText(/^Saved agent profile/)).toBeVisible();
    expect(fake.writes.at(-1)?.fields).not.toHaveProperty("repositories");
    expect(document.body.textContent).not.toMatch(/\bgit\b|repositor/i);
  });
});

describe("WEB-U-049 the profile form", () => {
  it("states the narrowing rule, creates a personal profile with no servers, and lists it", async () => {
    const fake = fakeProfiles({ profiles: LISTED });
    show(fake.api);
    await go("Add agent profile");
    expect(page()).toHaveTextContent(
      "An agent profile can only take away from what this installation allows. It can lower a limit and choose fewer tools, skills or connectors, never more.",
    );
    // PRD FR-10, superseded: no approval policy or permission wording.
    expect(page()).not.toHaveTextContent(/polic|permission/i);
    expect(within(page()).queryByLabelText("Shared with everyone")).toBeNull();
    set("Display name", "Docs writer");
    expect(page()).toHaveTextContent("ID: docs-writer · Change");
    setId("docs");
    set("Model", "Local model");
    fireEvent.click(within(page()).getByLabelText("Issue tracker"));
    save();
    expect(
      await screen.findByText("Saved agent profile Docs writer."),
    ).toBeVisible();
    expect(fake.writes.at(-1)?.fields).toMatchObject({
      name: "docs",
      model_route: "local",
      connectors: ["tracker"],
      mcp_servers: [],
      tools: null,
    });
    expect(
      within(
        screen.getByRole("table", { name: "Your agent profiles" }),
      ).getByText("Docs writer"),
    ).toBeVisible();
  });

  it("shows the gateway's reason at its field and keeps the edits through a conflict", async () => {
    const fake = fakeProfiles({ profiles: LISTED, routes: [] });
    show(fake.api);
    await go("Edit agent profile Code review");
    set("Model", "Local model");
    save();
    expect(
      await within(page()).findByText(
        "This isn't available on this installation now.",
      ),
    ).toBeVisible();
    set("Model", "The task chooses");
    fake.touch(LISTED[0]?.id ?? "");
    save();
    expect(
      await within(page()).findByText(/^Someone changed this agent profile/),
    ).toBeVisible();
    expect(within(page()).getByLabelText("Display name")).toHaveValue(
      "Code review",
    );
    save();
    expect(
      await screen.findByText("Saved agent profile Code review."),
    ).toBeVisible();
  });

  it("duplicates a shared profile into a new personal one", async () => {
    const fake = fakeProfiles({ profiles: LISTED });
    show(fake.api);
    await go("Duplicate agent profile Team default as your own");
    expect(within(page()).getByLabelText("Display name")).toHaveValue(
      "Team default",
    );
    // Its ID is made from its display name, like any new profile's.
    expect(page()).toHaveTextContent("ID: team-default · Change");
    setId("mine");
    save();
    expect(
      await screen.findByText("Saved agent profile Team default."),
    ).toBeVisible();
    expect(fake.writes.at(-1)?.fields).not.toHaveProperty("shared");
  });

  it("deletes after confirming that existing tasks keep their copy", async () => {
    show(fakeProfiles({ profiles: LISTED }).api);
    fireEvent.click(
      await screen.findByRole("button", {
        name: "More actions for agent profile Code review",
      }),
    );
    fireEvent.click(
      screen.getByRole("menuitem", {
        name: "Delete agent profile Code review",
      }),
    );
    expect(dialog()).toHaveTextContent(
      "Tasks already made from this agent profile keep what it set",
    );
    fireEvent.click(
      within(dialog()).getByRole("button", { name: "Delete agent profile" }),
    );
    expect(
      await screen.findByText("Deleted agent profile Code review."),
    ).toBeVisible();
  });

  it("sends the narrowing it is given: chosen tools, named skills, lower limits, and no approval policy", async () => {
    const fake = fakeProfiles({ profiles: LISTED });
    show(fake.api);
    await go("Add agent profile");
    set("Display name", "Narrow");
    // Every tool is listed, ticked (WEB-U-105); the person unticks what the profile should not have.
    for (const tool of [
      "Write files",
      "Edit files",
      "Find files",
      "Read notes",
      "Write notes",
      "Run commands",
      "Use skills",
      "Fetch web pages",
      "Post to channels",
      "Read channels",
      "Wait on channels",
      "Claim channel tasks",
    ])
      fireEvent.click(within(page()).getByLabelText(tool));
    set("Skills offered", "Only these skills");
    set("Skill names", "lint docs");
    set("Environment", "Workstation");
    // PRD FR-10, superseded: there is no approval policy to choose.
    expect(within(page()).queryByLabelText("Approval policy")).toBeNull();
    expect(
      within(page()).getByRole("region", { name: "Limits" }),
    ).toBeVisible();
    set("Steps per run", "20");
    set("When the workspace is busy", "Don't start");
    set("Instructions for the agent", "Review only.");
    // The limit in words, and no running count until the text nears it.
    expect(within(page()).getByText("Up to 16 KB.")).toBeVisible();
    expect(within(page()).queryByText(/of 16/)).toBeNull();
    save();
    expect(
      await screen.findByText("Saved agent profile Narrow."),
    ).toBeVisible();
    expect(fake.writes.at(-1)?.fields).toMatchObject({
      tools: ["read_file", "grep"],
      skills: ["lint", "docs"],
      template: "workstation",
      limits: { max_steps: 20 },
      busy_policy: "refuse",
      instructions: "Review only.",
    });
    expect(fake.writes.at(-1)?.fields).not.toHaveProperty("policy");
  });

  it("keeps a shared profile's tool servers, with no rules shown or sent back", async () => {
    const sharedWithServers = profile("ops", {
      display_name: "Ops",
      shared: true,
      mcp_servers: ["search"],
    });
    const fake = fakeProfiles({ profiles: [sharedWithServers], admin: true });
    show(fake.api, admin);
    await go("Edit agent profile Ops");
    expect(page()).toHaveTextContent(
      "Tool servers, set by an administrator: search.",
    );
    // PRD FR-10, superseded: no rules section; the profile's tools and the network allow-list decide.
    expect(within(page()).queryByRole("region", { name: "Rules" })).toBeNull();
    save();
    expect(await screen.findByText("Saved agent profile Ops.")).toBeVisible();
    expect(fake.writes.at(-1)?.fields).not.toHaveProperty("rules");
    expect(fake.writes.at(-1)?.fields).not.toHaveProperty("policy");
  });

  // The form speaks as people do: sizes in KB near the limit only, models by name, plain section headings.
  it("names the model and the size limit in words, and counts only near the limit", async () => {
    const fake = fakeProfiles({ profiles: LISTED });
    show(fake.api);
    await go("Add agent profile");
    fireEvent.click(picker("Model", page()));
    expect(
      screen.getByRole("menuitemradio", { name: /^Local model/ }),
    ).toBeInTheDocument();
    expect(screen.queryByRole("menuitemradio", { name: /^local/ })).toBeNull();
    fireEvent.keyDown(screen.getByRole("menu"), { key: "Escape" });
    for (const legend of page().querySelectorAll("legend"))
      expect(legend).not.toHaveClass("type-label");
    fireEvent.change(
      within(page()).getByLabelText("Instructions for the agent"),
      { target: { value: "x".repeat(15_000) } },
    );
    expect(within(page()).getByText("15 KB of 16 KB")).toBeVisible();
  });
});

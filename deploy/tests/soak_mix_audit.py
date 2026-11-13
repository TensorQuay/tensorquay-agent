"""SOAK-001's audit of the resources agent teams and Desktop tasks hold (ADR-0034; ADR-0039 §10). Every count has a
floor of 0: no slot conversation forms a team or opens a desktop, so between the lanes' conversations, and at the end,
each count is back to 0. The listings come from the stack (MixProbe); the rest only judges them.

- helper_sandboxes: sandbox containers, in any state, of the helper tasks the team lane saw;
- workspace_copies: `workspaces` rows of kind `helper_copy`, in any state (a deleted one waits for the purge);
- copy_folders: folders under the storage root's `workspaces/` with no `standard` workspace row: a copy's, its row
  purged or not;
- team_log_rows: `channel_messages`, the teams' message log; teams: `agent_teams` rows;
- pending_team_turns: queued team turns, the wakes and the helpers' first turns (`team-wake:` and `team-start:`);
- reservations: `runtime_token_reservations` (GW-I-252), held while a sandbox is created;
- desktops_in_use: running sandboxes on the pinned Desktop image, never above `desktop.max_sessions`;
- desktop_processes: the processes in those sandboxes (`docker top`);
- profile_folders: Desktop sandboxes, in any state, with the browser's profile tmpfs `/run/tqa-browser` (the profile
  lives only there, ADR-0039 §10);
- screenshot_files: PNGs under the storage root's `screens/<task>/`."""
import collections
import subprocess

from soak_audit import require

COUNTS = ("helper_sandboxes", "workspace_copies", "copy_folders", "team_log_rows", "teams", "pending_team_turns",
          "reservations", "desktops_in_use", "desktop_processes", "profile_folders", "screenshot_files")
DATABASE_COUNTS = ("teams", "team_log_rows", "workspace_copies", "pending_team_turns", "reservations")
TEAM_COUNTS = ("helper_sandboxes", "workspace_copies", "copy_folders", "team_log_rows", "teams", "pending_team_turns")
DESKTOP_COUNTS = ("desktops_in_use", "desktop_processes", "profile_folders")
PROFILE = "/run/tqa-browser"

DATABASE_QUERY = """
SELECT 'teams', count(*)::text FROM agent_teams
UNION ALL SELECT 'team_log_rows', count(*)::text FROM channel_messages
UNION ALL SELECT 'workspace_copies', count(*)::text FROM workspaces WHERE kind = 'helper_copy'
UNION ALL SELECT 'pending_team_turns', count(*)::text FROM idempotency
  WHERE execution_status = 'queued' AND (key LIKE 'team-wake:%' OR key LIKE 'team-start:%')
UNION ALL SELECT 'reservations', count(*)::text FROM runtime_token_reservations
UNION ALL SELECT 'copy', id::text FROM workspaces WHERE kind = 'helper_copy'
UNION ALL SELECT 'workspace', id::text FROM workspaces WHERE kind = 'standard';
"""

# The storage root's screenshots and workspace folders, as paths relative to it.
STORAGE_LISTING = ("cd /storage && { if [ -d screens ]; then find screens -mindepth 2 -maxdepth 2 -type f; fi; "
                   "if [ -d workspaces ]; then find workspaces -mindepth 1 -maxdepth 1; fi; }")


def parse_database(text):
    """`name|value` lines from psql -At: the counts, the helper copies' ids and the standard workspaces' ids."""
    counts, copies, workspaces = {}, set(), set()
    for line in text.splitlines():
        if not line.strip():
            continue
        name, value = line.strip().split("|", 1)
        if name == "copy":
            copies.add(value)
        elif name == "workspace":
            workspaces.add(value)
        else:
            require(name in DATABASE_COUNTS and value.isdigit(), "unexpected mix audit row: " + line.strip())
            counts[name] = int(value)
    require(set(counts) == set(DATABASE_COUNTS), "the mix audit's query missed a count")
    return {"counts": counts, "copies": copies, "workspaces": workspaces}


def parse_storage(text):
    """The storage listing: PNGs per task under `screens/`, and the folder names under `workspaces/`."""
    screens, folders = collections.Counter(), set()
    for line in text.splitlines():
        parts = line.strip().split("/")
        if len(parts) == 3 and parts[0] == "screens" and parts[2].endswith(".png"):
            screens[parts[1]] += 1
        elif len(parts) == 2 and parts[0] == "workspaces" and parts[1]:
            folders.add(parts[1])
    return {"screenshot_files": sum(screens.values()), "screens": dict(screens), "workspace_folders": folders}


def copy_folders(folders, standard):
    """Workspace folders with no standard workspace row (a folder may be `<id>` or a microVM's `<id>.ext4`)."""
    return sum(1 for name in folders if name.split(".", 1)[0] not in standard)


def is_desktop(record, desktop_image):
    return desktop_image is not None and record["Image"] == desktop_image


def desktop_counts(records, desktop_image, tops):
    """Desktops in use (running), their processes, and the Desktop sandboxes that hold a profile tmpfs."""
    desktops = [record for record in records if is_desktop(record, desktop_image)]
    running = [record for record in desktops if record["State"]["Status"] == "running"]
    return {"desktops_in_use": len(running),
            "desktop_processes": sum(tops.get(record["Id"], 0) for record in running),
            "profile_folders": sum(1 for record in desktops
                                   if PROFILE in ((record.get("HostConfig") or {}).get("Tmpfs") or {}))}


def conversation_of(record):
    return record["Config"]["Labels"].get("tqa.conversation", "")


def helper_sandboxes(records, helpers):
    return sum(1 for record in records if conversation_of(record) in helpers)


def sample_problems(counts, max_sessions, helpers=2):
    """A sample's own limits: desktops in use within `desktop.max_sessions`, a team's helpers' sandboxes within its
    two helpers (one team at a time)."""
    problems = []
    if counts["desktops_in_use"] > max_sessions:
        problems.append("{} desktops in use, above desktop.max_sessions {}".format(counts["desktops_in_use"],
                                                                                   max_sessions))
    if counts["helper_sandboxes"] > helpers:
        problems.append("{} helper sandboxes, above the team's {} helpers".format(counts["helper_sandboxes"], helpers))
    return problems


def above_floor(counts, names, when):
    """Each named count above its floor of 0; a missing count is a problem, never a pass."""
    problems = []
    for name in names:
        value = counts.get(name)
        if value is None:
            problems.append("{}: not counted {}".format(name, when))
        elif value > 0:
            problems.append("{}: {} {}, its floor 0".format(name, value, when))
    return problems


def end_problems(counts):
    """The end of the run: every count back to its floor."""
    return above_floor(counts, COUNTS, "at the end")


def own_sandboxes(sandboxes, conversations):
    return ["conversation {} still has {} sandbox{}".format(conversation, sandboxes[conversation],
                                                            "es" if sandboxes[conversation] > 1 else "")
            for conversation in sorted(conversations) if sandboxes.get(conversation)]


def team_leftovers(counts, sandboxes, conversations):
    """What a deleted team (its lead and helpers, `conversations`) still holds."""
    return own_sandboxes(sandboxes, conversations) + above_floor(counts, TEAM_COUNTS, "after the delete")


def desktop_leftovers(counts, sandboxes, screens, conversation):
    """What a deleted Desktop task still holds."""
    problems = own_sandboxes(sandboxes, {conversation})
    if screens.get(conversation):
        problems.append("conversation {} still has {} screenshot files".format(conversation, screens[conversation]))
    return problems + above_floor(counts, DESKTOP_COUNTS, "after the delete")


def storage_arguments(stack, toybox):
    """A throwaway container that lists the storage root, read-only and with no network (as the Desktop suite's picture
    copy reads it: on a rootless engine the root is the gateway's, which this user can't read)."""
    return ["docker", "run", "--rm", "--network", "none", "--mount",
            "type=bind,source={},target=/storage,readonly".format(stack.env["TQA_STORAGE_ROOT"]), toybox,
            "sh", "-c", STORAGE_LISTING]


def stopped(error):
    """The engine's answer for a container that stopped after it was listed running."""
    return isinstance(error, subprocess.CalledProcessError) and "is not running" in (error.stderr or "")


class MixProbe:
    """The raw listings, through the stack's own bounded commands: its sandbox containers (inspected), the processes of
    its running desktops, the database's counts and the storage root's listing."""

    def __init__(self, stack, desktop_image, toybox):
        self.stack, self.desktop_image, self.toybox = stack, desktop_image, toybox

    def processes(self, identity):
        """The processes of a running desktop; one that stopped since it was listed has none."""
        try:
            return max(0, len(self.stack.command(["docker", "top", identity]).splitlines()) - 1)
        except subprocess.CalledProcessError as error:
            if stopped(error):
                return 0
            raise

    def __call__(self, helpers):
        stack = self.stack
        label = "label=tqa.instance=" + stack.env["TQA_SANDBOX_NETWORK"]
        records = [stack.inspect(identity) for identity in stack.command(["docker", "ps", "-aq", "--filter",
                                                                          label]).split()]
        tops = {record["Id"]: self.processes(record["Id"]) for record in records
                if is_desktop(record, self.desktop_image) and record["State"]["Status"] == "running"}
        database = parse_database(stack.command(["docker", "exec", stack.database, "psql", "-U", "tqa", "-d", "gateway",
                                                 "-At", "-F", "|", "-c", DATABASE_QUERY], timeout=60))
        storage = parse_storage(stack.helper(storage_arguments(stack, self.toybox)))
        counts = dict(database["counts"], helper_sandboxes=helper_sandboxes(records, helpers),
                      copy_folders=copy_folders(storage["workspace_folders"], database["workspaces"]),
                      screenshot_files=storage["screenshot_files"],
                      **desktop_counts(records, self.desktop_image, tops))
        sandboxes = collections.Counter(conversation_of(record) for record in records)
        return {"counts": {name: counts[name] for name in COUNTS}, "sandboxes": dict(sandboxes),
                "screens": storage["screens"]}

"""SEC-010 on the microVM (`contract.py pids`): the storm the stormed guest runs, and the judgement over its report.

The guest's limit is its sandbox cgroup's `pids.max`, which the guest init sets from the per-sandbox configuration
device, that is from the gateway's `[sandbox] pids`. The storm reads it in the guest and starts that many background
sleeps plus PAST_THE_LIMIT: spawns past the limit fail and are not retried, so nothing spins (no artificial CPU or
memory load on the machine that runs it). It then reads the cgroup's count and events, prints one report line, and
reaps every sleep.
"""
import re

PAST_THE_LIMIT = 100
# The shell tool's hard limit (tqa-tools shell_timeout); a call past it returns no output, so the storm's sleeps, with
# spawning and reaping, end well inside it while still leaving a neighbour VM time to start and answer.
SHELL_TIMEOUT_SECONDS = 30
SLEEP_SECONDS = 20
CGROUP = "/sys/fs/cgroup/sandbox"

# Python, not a shell loop: a shell that can't fork a background job aborts the whole script (dash), so it could not
# report on its own storm. posix_spawn of sleep keeps each child small; a refused spawn (EAGAIN) is counted, not retried.
STORM = """import errno, os
g = "{cgroup}"
limit = int(open(g + "/pids.max").read())
storm = limit + {past}
children, refused = [], 0
for _ in range(storm):
    try:
        children.append(os.posix_spawn("/bin/sleep", ["sleep", "{sleep}"], {{}}))
    except OSError as error:
        if error.errno != errno.EAGAIN:
            raise
        refused += 1
current = int(open(g + "/pids.current").read())
events = dict(line.split() for line in open(g + "/pids.events"))
print("pids max={{}} storm={{}} current={{}} limited={{}} refused={{}}".format(
    limit, storm, current, events.get("max", 0), refused), flush=True)
for child in children:
    os.waitpid(child, 0)
print("pids done", flush=True)
""".format(cgroup=CGROUP, past=PAST_THE_LIMIT, sleep=SLEEP_SECONDS)

REPORT = re.compile(r"^pids max=(\d+) storm=(\d+) current=(\d+) limited=(\d+) refused=(\d+)$", re.MULTILINE)


def problems(output, *, configured):
    """What is wrong with the stormed guest's report; empty when the storm stopped at the configured limit."""
    match = REPORT.search(output)
    if not match:
        return ["the stormed guest printed no pids report"]
    limit, storm, current, limited, refused = (int(value) for value in match.groups())
    found = []
    if limit != configured:
        found.append("the guest's limit is {}, the gateway configured {}".format(limit, configured))
    if storm != limit + PAST_THE_LIMIT:
        found.append("a storm of {} is not the limit plus {}".format(storm, PAST_THE_LIMIT))
    if current > limit:
        found.append("{} processes, past the limit of {}".format(current, limit))
    if limited == 0 or refused < PAST_THE_LIMIT:
        # The storm is the limit plus PAST_THE_LIMIT, and the guest's own processes count too: at least that many
        # spawns must have been refused.
        found.append("the limit did not refuse the storm's excess (pids.events max {}, {} spawns refused, at least {} "
                     "expected)".format(limited, refused, PAST_THE_LIMIT))
    if not re.search(r"^pids done$", output, re.MULTILINE):
        found.append("the storm's processes were not all reaped")
    return found

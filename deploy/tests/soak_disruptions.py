"""SOAK-001…004: the seeded plan of gateway kills, database faults, sandbox kills and link cuts.

The plan has its own random stream, so the reconnect plan of a seed (soak_chaos.schedule) stays what it was. Global
faults (gateway kill, database restart, outage, crash) never overlap each other or a restart; a slot's faults (sandbox
kill, link cut) never overlap a global fault or each other in that slot. Each fault keeps a recovery window after it,
in which nothing else lands, so a failure can be attributed to one fault. The pauses of the reconnect plan then keep
clear of all of them."""
import random

HOUR = 3600
GLOBAL = ("gateway_kill", "db_restart", "db_outage", "db_crash", "engine_stall", "disk_full")
SLOT = ("sandbox_kill", "link_cut")
KINDS = GLOBAL + SLOT
# On a microVM a link cut is not the same fault (the guest's network runs through a tap inside the VM host), and each
# workspace is a sparse block image far larger than a small tmpfs; the microVM's own out-of-space path is SBX-C-015.
MICROVM_EXCLUDED = ("link_cut", "disk_full")


def backend_kinds(backend):
    return tuple(kind for kind in KINDS if backend != "firecracker" or kind not in MICROVM_EXCLUDED)
# (period, first time) in a full run; a kind whose period exceeds the run still happens once.
CADENCE = {"gateway_kill": (2 * HOUR, HOUR), "db_restart": (3 * HOUR, 1800), "db_outage": (6 * HOUR, 2 * HOUR + 1500),
           "db_crash": (12 * HOUR, 5 * HOUR + 1200), "engine_stall": (4050, 2400),
           "disk_full": (4 * HOUR, 3 * HOUR + 600), "sandbox_kill": (900, 450),
           "link_cut": (1800, 900)}
JITTER = {"sandbox_kill": 200, "link_cut": 600, "engine_stall": 900}
STALL_ACTIONS = ("hold", "error", "drop")
# Seconds after a fault (after its outage or cut) in which nothing else lands.
RECOVERY = {"gateway_kill": 180, "db_restart": 180, "db_outage": 180, "db_crash": 180, "engine_stall": 180,
            "disk_full": 180, "sandbox_kill": 150, "link_cut": 120}
# A rehearsal compresses one of each into its five minutes.
REHEARSAL = {"gateway_kill": 15, "db_restart": 45, "db_crash": 75, "db_outage": 105, "engine_stall": 140,
             "disk_full": 170, "sandbox_kill": 200, "link_cut": 230}
RESTART_BEFORE, RESTART_AFTER = 120, 180
KILL_LIMIT = 12 * HOUR  # The 24 h run's last 12 h are one gateway lifetime (M5).


def kill_limit(seconds):
    """Gateway kills stay in the restarted half of a long run: hours 0–12 of 24, 0–4 of 8."""
    return KILL_LIMIT if seconds >= 24 * HOUR else 4 * HOUR if seconds >= 8 * HOUR else None
STEP = 60


def _nominal(kind, seconds, rehearsal):
    if rehearsal:
        return [REHEARSAL[kind]]
    period, first = CADENCE[kind]
    times = list(range(first, int(seconds) - 600, period))
    if kind == "gateway_kill" and kill_limit(seconds):
        times = [at for at in times if at < kill_limit(seconds) - 600]
    return times or [first % max(1, int(seconds) // 2) + int(seconds) // 4]


def _window(at, duration, kind, rehearsal):
    recovery = 10 if rehearsal else RECOVERY[kind]
    return at, at + duration + recovery


def _free(window, busy):
    return not any(window[0] < end and start < window[1] for start, end in busy)


def disruptions(seed, seconds, rehearsal, slots, restarts, kinds=KINDS):
    randomizer = random.Random("disruptions-{}".format(seed))
    busy = [] if rehearsal else [(at - RESTART_BEFORE, at + RESTART_AFTER) for at in restarts]
    slot_busy = {slot: [] for slot in range(slots)}
    limit = int(seconds) - (30 if rehearsal else 600)
    plan = []
    for kind in KINDS:
        if kind not in kinds:
            continue
        for nominal in _nominal(kind, seconds, rehearsal):
            jitter = 0 if rehearsal else JITTER.get(kind, 300)
            at = max(1, nominal + randomizer.randint(-jitter, jitter))
            fault = {"kind": kind}
            if kind == "db_outage":
                fault["seconds"] = 20 if rehearsal else randomizer.randint(60, 180)
            elif kind == "link_cut":
                fault["seconds"] = 10 if rehearsal else randomizer.randint(5, 60)
            elif kind == "disk_full":
                fault["seconds"] = 15 if rehearsal else randomizer.randint(60, 120)
            elif kind == "engine_stall":
                # A window of matching engine calls held (up to `hold` seconds each), failed or dropped.
                fault["seconds"] = 15 if rehearsal else randomizer.randint(60, 120)
                fault["action"] = STALL_ACTIONS[randomizer.randrange(3)]
                fault["hold"] = 10 if rehearsal else randomizer.randint(20, min(90, fault["seconds"]))
            if kind in SLOT:
                fault["target"] = randomizer.randrange(slots)
            ceiling = min(limit, kill_limit(seconds) - RECOVERY[kind]) if kind == "gateway_kill" and kill_limit(seconds) \
                else limit
            while at <= ceiling:
                window = _window(at, fault.get("seconds", 0), kind, rehearsal)
                own = slot_busy[fault["target"]] if kind in SLOT else [span for spans in slot_busy.values()
                                                                        for span in spans]
                if window[1] <= ceiling + RECOVERY[kind] and _free(window, busy) and _free(window, own):
                    break
                at += 10 if rehearsal else STEP
            else:
                continue
            fault["at"] = at
            (slot_busy[fault["target"]] if kind in SLOT else busy).append(
                _window(at, fault.get("seconds", 0), kind, rehearsal))
            plan.append(fault)
    return sorted(plan, key=lambda fault: (fault["at"], fault["kind"]))

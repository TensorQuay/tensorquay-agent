"""SOAK-001 (PRD M5): resource growth judged per gateway lifetime. A restart resets the gateway's memory, so growth is
only measured within one process's life, and only in lifetimes of at least 4 h. The 24 h run keeps its last 12 h free
of restarts, so M5 is judged on that one 12 h lifetime:
- a 1 h warm-up is left out;
- the median of the hour after it is compared with the median of the last hour (growth under 5 %);
- a least-squares slope over the lifetime after the warm-up, projected to 24 h, must also stay under 5 %, so a slow
  leak can't hide between two close hours.

That 5 % rule judges the gateway's RSS. Open descriptors and database connections are judged by what they are
(the c15 soaks of 6 and 8 Oct 2026 failed both on noise; TEST-PLAN SOAK-001; the owner's decision of 9 Oct 2026,
PRD §12 item 25):
- open_fds without the database connections (each is a socket): the fds other than DB sockets stayed at 50 every hour
  while the pool's 4-16 swings moved open_fds, so those fds take the 5 % rule (judge_fds);
- database_connections are bounded by the pool, so they never grow without end: a leak shows as connections outside
  the pool, a pool stuck at its maximum, or a floor that creeps up (judge_connections).
"""
import statistics

HOUR = 3600
WARM_UP = HOUR
MIN_LIFETIME = 4 * HOUR
LIMIT = 0.05
# The gateway's database pool (gateway.toml limits.database_connections, 16 by default; the soak's stacks set none)
# and its connections outside the pool: a PgListener (teams/changed.rs) only with agent teams on, which the soak's
# stacks never turn on, so none.
POOL = 16
LISTENERS = 0
# The measurement's own share of `database_connections`: soak_chaos's probe counts every backend of the database, its
# own psql session included (the c15 soaks' 17 was the pool at 16 plus it). A sample that has `gateway_connections`
# (every client backend but the probe's own: today, the gateway's; from 9 Oct 2026) is read as it is.
PROBE = 1
# A pool at its maximum in bursts is healthy; through most of the last hour it's exhausted.
PINNED_LIMIT = 0.90
# The floor: the 10th percentile of the first three hours after warm-up against the last three's, judged only on a
# lifetime of 11 h or more (the 24 h run's, from its last restart at hour 12; a shorter one can't tell a slow leak
# from noise). Sized on the c15 soaks' settled hours, tiled to 12 h: healthy rises at most 1 (the 8 Oct run's own 7 h,
# 2); 4 connections held over 12 h fail in 8 of 10 tilings and 5 in all; 3 or fewer can pass.
FLOOR_QUANTILE = 0.10
FLOOR_WINDOW = 3 * HOUR
FLOOR_MIN_LIFETIME = 11 * HOUR
FLOOR_RISE = 2
# A gateway connection idle in Postgres for longer than this has been taken out of the pool (the probe counts
# them). The pool's own bound with no traffic at all: sqlx's idle_timeout (600 s) plus its reaper's period (600 s;
# a re-release resets sqlx's idle clock, not Postgres's state_change), plus 5 min. It rests on sqlx 0.9's defaults
# for idle_timeout and max_lifetime, which db.rs doesn't set: if it ever sets them, this moves with them.
STALE_SECONDS = 1500


def lifetimes(samples, windows):
    """Samples split at each restart window; samples inside a window belong to neither side."""
    parts, current, bounds = [], [], sorted(windows)
    for sample in sorted(samples, key=lambda item: item["at"]):
        while bounds and sample["at"] >= bounds[0][1]:
            if current:
                parts.append(current)
            current, bounds = [], bounds[1:]
        if bounds and sample["at"] >= bounds[0][0]:
            continue
        current.append(sample)
    if current:
        parts.append(current)
    return parts


def _slope(points):
    count = len(points)
    mean_x = sum(x for x, _ in points) / count
    mean_y = sum(y for _, y in points) / count
    spread = sum((x - mean_x) ** 2 for x, _ in points)
    return sum((x - mean_x) * (y - mean_y) for x, y in points) / spread if spread else 0.0


def judge(lifetime, metric, warm_up=WARM_UP, window=HOUR, min_lifetime=MIN_LIFETIME, horizon=24 * HOUR):
    """Growth of `metric` over one lifetime, or None when the lifetime is under `min_lifetime`. The defaults are M5's;
    a shorter measurement (PERF-021's hour) scales the warm-up, the compared windows and the slope's horizon."""
    start, end = lifetime[0]["at"], lifetime[-1]["at"]
    if end - start < min_lifetime:
        return None
    settled = [sample for sample in lifetime if sample["at"] >= start + warm_up]
    baseline = statistics.median(s[metric] for s in settled if s["at"] < start + warm_up + window)
    final = statistics.median(s[metric] for s in settled if s["at"] >= end - window)
    slope = _slope([(s["at"], s[metric]) for s in settled])
    growth = (final - baseline) / baseline if baseline else float("inf") if final > baseline else 0.0
    projected = slope * horizon / baseline if baseline else float("inf") if slope > 0 else 0.0
    return {"metric": metric, "baseline": baseline, "final": final, "growth": growth, "projected_24h": projected,
            "passed": growth < LIMIT and projected < LIMIT}


def gateway_connections(sample):
    """The gateway's own database connections in a sample: the probe's count less its own session, unless the sample
    already has them apart."""
    if "gateway_connections" in sample:
        return sample["gateway_connections"]
    return sample["database_connections"] - PROBE


def judge_fds(lifetime, **judging):
    """Open descriptors other than the gateway's database connections (each a socket), by the 5 % rule (`judge`'s,
    with its keywords)."""
    others = [dict(sample, nondb_fds=sample["open_fds"] - gateway_connections(sample)) for sample in lifetime]
    result = judge(others, "nondb_fds", **judging)
    return None if result is None else dict(result, metric="open_fds", judged_as="open_fds - gateway connections")


def _quantile(values, fraction):
    ordered = sorted(values)
    return ordered[min(len(ordered) - 1, int(fraction * (len(ordered) - 1) + 0.5))]


def judge_connections(lifetime, pool=POOL, listeners=LISTENERS, warm_up=WARM_UP, window=HOUR,
                      min_lifetime=MIN_LIFETIME, **_horizon):
    """The gateway's database connections over one lifetime, or None under `min_lifetime`: never above the pool's
    maximum plus its listeners (D1); at the pool's maximum in under 90 % of the last `window` (D2); on a lifetime of
    11 h or more, the floor up by at most 2 (D3; shorter, D3 is not judged and says so); and after the warm-up, no
    more of them idle over STALE_SECONDS than its listeners (D4: a held connection idles for good, while the pool
    reuses its own within seconds). Samples from before the probe read idle ages don't judge D4, and say so."""
    start, end = lifetime[0]["at"], lifetime[-1]["at"]
    if end - start < min_lifetime:
        return None
    counts = [(sample["at"], gateway_connections(sample)) for sample in lifetime]
    settled = [(at, count) for at, count in counts if at >= start + warm_up]
    last = [count for at, count in settled if at >= end - window]
    highest = max(count for _, count in counts)
    pinned = sum(count >= pool for count in last) / len(last)
    result = {"metric": "database_connections", "judged_as": "gateway connections (the probe's own less)",
              "highest": highest, "ceiling": pool + listeners, "pinned_last_window": pinned,
              "floor_judged": end - start >= FLOOR_MIN_LIFETIME, "floor_first": None, "floor_last": None}
    floor_ok = True
    if result["floor_judged"]:
        first = [count for at, count in settled if at < start + warm_up + FLOOR_WINDOW]
        final = [count for at, count in settled if at >= end - FLOOR_WINDOW]
        result["floor_first"], result["floor_last"] = _quantile(first, FLOOR_QUANTILE), _quantile(final, FLOOR_QUANTILE)
        floor_ok = result["floor_last"] - result["floor_first"] <= FLOOR_RISE
    stale = [sample.get("gateway_stale_connections") for sample in lifetime if sample["at"] >= start + warm_up]
    result["idle_judged"] = bool(stale) and all(count is not None for count in stale)
    result["stale_highest"] = max(stale) if result["idle_judged"] else None
    result["idle_age_highest"] = (max(sample["gateway_idle_age_seconds"] for sample in lifetime
                                      if sample["at"] >= start + warm_up) if result["idle_judged"] else None)
    idle_ok = not result["idle_judged"] or result["stale_highest"] <= listeners
    result["passed"] = highest <= pool + listeners and pinned < PINNED_LIMIT and floor_ok and idle_ok
    return result


def judge_metric(lifetime, metric, pool=POOL, listeners=LISTENERS, **judging):
    """Each metric by its own rule: open_fds and database_connections as above, everything else (RSS) by 5 %. The
    keywords scale the warm-up, the windows and the minimum lifetime, as PERF-021's hour does."""
    if metric == "open_fds":
        return judge_fds(lifetime, **judging)
    if metric == "database_connections":
        return judge_connections(lifetime, pool, listeners, **judging)
    return judge(lifetime, metric, **judging)


def verdict(samples, windows, metrics, require_judged, pool=POOL, listeners=LISTENERS, require_floor=False):
    """Every metric of every lifetime of at least 4 h must pass; a 24 h run must have one such lifetime and, with
    `require_floor`, one of 11 h or more, so the connection floor (D3) is judged: an unplanned restart in its last
    12 h can't let it pass with D3 never run. Wherever D3 wasn't judged, the reason says so."""
    judged = []
    for lifetime in lifetimes(samples, windows):
        results = {metric: judge_metric(lifetime, metric, pool, listeners) for metric in metrics}
        if all(result is not None for result in results.values()):
            judged.append({"from": lifetime[0]["at"], "to": lifetime[-1]["at"],
                           "hours": (lifetime[-1]["at"] - lifetime[0]["at"]) / HOUR, "metrics": results})
    if not judged:
        reason = "no gateway lifetime of 4 h or more" + ("" if require_judged else ": growth not judged")
        return {"passed": not require_judged, "judged": [], "reason": reason}
    failed = [(part["from"], metric) for part in judged for metric, result in part["metrics"].items()
              if not result["passed"]]
    floors = [part["metrics"]["database_connections"]["floor_judged"] for part in judged
              if "database_connections" in part["metrics"]]
    unjudged_floor = bool(floors) and not all(floors)
    floor_missing = require_floor and bool(floors) and not any(floors)
    reason = ("every metric within its limit in every judged lifetime" if not failed else
              "over its limit: {}".format(", ".join(metric for _, metric in failed)))
    if floor_missing:
        reason += "; no gateway lifetime of 11 h or more, so the connection floor (D3) not judged"
    elif unjudged_floor:
        reason += "; the connection floor (D3) not judged in a lifetime under 11 h"
    if any(not part["metrics"]["database_connections"]["idle_judged"] for part in judged
           if "database_connections" in part["metrics"]):
        reason += "; the connections' idle age (D4) not recorded in its samples (from before 9 Oct 2026)"
    return {"passed": not failed and not floor_missing, "judged": judged, "reason": reason}

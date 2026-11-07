"""The agent-teams evaluation's numbers (docs/evals/agent-teams.md section 2): pass rates, judged scores, paired
differences per task, and their 95 % intervals by a paired bootstrap over tasks, as E2E-CMP-003 computes them (the
tasks are resampled with replacement, each keeping its two sides together). Pure: it reads records, never a run.

A record is one attempt after its rerun chain: task, kind, side ("single" or "team"), its automatic check (passed, or
None where the task has none), its judged score (0 to 10, or None until judged), tokens, wall time, and its failure
classes.
"""

import math
import random
from dataclasses import dataclass, field

SIDES = ("single", "team")
KINDS = ("research", "report", "code", "mixed", "dependent", "long")
# The bars' "split work": the research and report tasks (the design's proposal for the lead's decision).
SPLIT = ("research", "report")
# The results' groups (section 3): the short tasks of the first set, the dependent ones and the long ones.
SHORT = ("research", "report", "code", "mixed")
GROUPS = (("short", SHORT), ("dependent", ("dependent",)), ("long", ("long",)))
# Failure classes only a team can have; the bars allow none in more than 10 % of the team's attempts.
TEAM_ONLY = ("helper_idle_or_stuck", "helper_never_reported", "report_not_taken", "lead_finished_unread",
             "end_note_missing")
RESAMPLES = 10_000
SEED = 20261005


@dataclass(frozen=True)
class Attempt:
    task: str
    kind: str
    side: str
    passed: bool | None
    score: float | None
    tokens: int
    wall_secs: float
    failures: tuple[str, ...] = field(default_factory=tuple)
    helpers: int = 0
    # A team's hand-backs that didn't go in as handed (take_files refused, or a file placed under another name than
    # its own because that one was taken), the lead's reads that waited on its helpers and found nothing, and its
    # reads told to yield (for a slot, or to a helper waiting on its files) (report.team_measures).
    take_conflicts: int = 0
    waits: int = 0
    yields: int = 0


def mean(values):
    values = list(values)
    return sum(values) / len(values) if values else None


def median(values):
    values = sorted(values)
    if not values:
        return None
    middle = len(values) // 2
    return values[middle] if len(values) % 2 else (values[middle - 1] + values[middle]) / 2


def wilson(passed, total, z=1.96):
    """A 95 % Wilson interval of a pass rate; (None, None) without attempts."""
    if total == 0:
        return (None, None)
    p = passed / total
    centre = (p + z * z / (2 * total)) / (1 + z * z / total)
    half = z * math.sqrt(p * (1 - p) / total + z * z / (4 * total * total)) / (1 + z * z / total)
    return (max(0.0, centre - half), min(1.0, centre + half))


def paired_bootstrap(differences, resamples=RESAMPLES, seed=SEED):
    """A percentile 95 % interval of the mean of per-task differences, resampling the tasks with replacement; seeded so
    a report is reproducible. (None, None) without tasks."""
    values = [value for value in differences if value is not None]
    if not values:
        return (None, None)
    rng = random.Random(seed)
    n = len(values)
    means = sorted(sum(rng.choice(values) for _ in range(n)) / n for _ in range(resamples))
    tail = int(0.025 * resamples)
    return (means[tail - 1], means[resamples - tail])


def per_task(attempts, metric):
    """{task: {side: mean of the metric over its attempts}}, skipping attempts without the metric."""
    found = {}
    for attempt in attempts:
        value = metric(attempt)
        if value is None:
            continue
        found.setdefault(attempt.task, {}).setdefault(attempt.side, []).append(float(value))
    return {task: {side: mean(values) for side, values in sides.items()} for task, sides in found.items()}


def differences(attempts, metric, kinds=None):
    """Per task, the team's mean minus the single agent's, for tasks both sides have; optionally only some kinds."""
    kind_of = {attempt.task: attempt.kind for attempt in attempts}
    table = per_task(attempts, metric)
    return {task: sides["team"] - sides["single"] for task, sides in sorted(table.items())
            if "team" in sides and "single" in sides and (kinds is None or kind_of[task] in kinds)}


def passed_metric(attempt):
    return None if attempt.passed is None else (1.0 if attempt.passed else 0.0)


def score_metric(attempt):
    return attempt.score


def side_summary(attempts, side, kinds=None):
    """One side's attempts, optionally only some kinds: counts and sums as before, medians per attempt, and the team's
    own measures."""
    mine = [attempt for attempt in attempts if attempt.side == side and (kinds is None or attempt.kind in kinds)]
    checked = [attempt for attempt in mine if attempt.passed is not None]
    passed = sum(1 for attempt in checked if attempt.passed)
    return {
        "attempts": len(mine),
        "passed": passed,
        "checked": len(checked),
        "pass_rate": passed / len(checked) if checked else None,
        "pass_interval": wilson(passed, len(checked)),
        "mean_score": mean(attempt.score for attempt in mine if attempt.score is not None),
        "tokens": sum(attempt.tokens for attempt in mine),
        "wall_secs": sum(attempt.wall_secs for attempt in mine),
        "median_tokens": median(attempt.tokens for attempt in mine),
        "median_wall_secs": median(attempt.wall_secs for attempt in mine),
        "median_wall_secs_passed": median(attempt.wall_secs for attempt in checked if attempt.passed),
        "mean_helpers": mean(attempt.helpers for attempt in mine),
        "take_conflicts": sum(attempt.take_conflicts for attempt in mine),
        "waits": sum(attempt.waits for attempt in mine),
        "yields": sum(attempt.yields for attempt in mine),
        "failures": failure_counts(mine),
    }


def failure_counts(attempts):
    counts = {}
    for attempt in attempts:
        for failure in set(attempt.failures):
            counts[failure] = counts.get(failure, 0) + 1
    return dict(sorted(counts.items()))


def comparison(attempts, kinds=None):
    """The paired comparison over some kinds (all by default): mean differences and their intervals."""
    out = {}
    for name, metric in (("pass_rate", passed_metric), ("score", score_metric)):
        found = differences(attempts, metric, kinds)
        out[name] = {"tasks": len(found), "mean": mean(found.values()),
                     "interval": paired_bootstrap(list(found.values())), "per_task": found}
    return out


def paired_ratio(attempts, metric, kinds=None):
    """The team's metric over the single agent's, paired: per task the mean attempt on each side, over the tasks both
    sides have (optionally only some kinds), so a side with fewer counted attempts doesn't move it."""
    table = per_task([attempt for attempt in attempts if kinds is None or attempt.kind in kinds], metric)
    pairs = [(sides["team"], sides["single"]) for sides in table.values() if "team" in sides and "single" in sides]
    single = sum(single for _, single in pairs)
    return sum(team for team, _ in pairs) / single if single else None


def wall_ratio(attempts, kinds=None):
    """The team's wall time over the single agent's, paired per task."""
    return paired_ratio(attempts, lambda attempt: attempt.wall_secs, kinds)


def token_ratio(attempts, kinds=None):
    """The team's tokens (lead and helpers) over the single agent's, paired per task."""
    return paired_ratio(attempts, lambda attempt: attempt.tokens, kinds)


def group(attempts, kinds):
    """One group of kinds on its own: each side's summary, the paired comparison and the paired cost ratios."""
    return {"kinds": list(kinds), "sides": {side: side_summary(attempts, side, kinds) for side in SIDES},
            "comparison": comparison(attempts, kinds), "wall_ratio": wall_ratio(attempts, kinds),
            "token_ratio": token_ratio(attempts, kinds)}


def bars(attempts):
    """The proposed bars (for the lead's decision): recommend for split work when the team's judged score is higher
    (the paired mean difference over the tasks, as the interval is), its interval on research and report tasks
    excludes no gain, it costs at most 1.5x the wall time (paired per task), and no team-only failure class shows in
    more than 10 % of its attempts; otherwise keep experimental."""
    team = side_summary(attempts, "team")
    split = comparison(attempts, SPLIT)["score"]
    gain = comparison(attempts)["score"]["mean"]
    low = split["interval"][0]
    ratio = wall_ratio(attempts)
    team_attempts = team["attempts"] or 1
    team_only = {name: count / team_attempts for name, count in team["failures"].items() if name in TEAM_ONLY}
    reasons = []
    if gain is None:
        reasons.append("not judged yet")
    elif gain <= 0:
        reasons.append("the team's mean judged score is not higher")
    if low is None or low <= 0:
        reasons.append("the interval on research and report tasks does not exclude no gain")
    if ratio is None or ratio > 1.5:
        reasons.append("the team costs more than 1.5x the wall time")
    worst = [name for name, share in team_only.items() if share > 0.10]
    if worst:
        reasons.append("team-only failures above 10 %: " + ", ".join(sorted(worst)))
    return {"verdict": "recommend for split work" if not reasons else "keep experimental", "reasons": reasons,
            "wall_ratio": ratio, "score_gain": gain, "team_only_shares": team_only, "split_interval": split["interval"],
            "split_tasks": split["tasks"]}


def analyse(attempts):
    return {
        "sides": {side: side_summary(attempts, side) for side in SIDES},
        "overall": comparison(attempts),
        "by_kind": {kind: comparison(attempts, (kind,)) for kind in KINDS},
        "split": comparison(attempts, SPLIT),
        "by_group": {name: group(attempts, kinds) for name, kinds in GROUPS},
        "wall_ratio": wall_ratio(attempts),
        "token_ratio": token_ratio(attempts),
        "lead_alone_share": lead_alone_share(attempts),
        "bars": bars(attempts),
    }


def lead_alone_share(attempts):
    """The share of team attempts whose lead started no helper: still team attempts, reported apart."""
    team = [attempt for attempt in attempts if attempt.side == "team"]
    return sum(1 for attempt in team if attempt.helpers == 0) / len(team) if team else None

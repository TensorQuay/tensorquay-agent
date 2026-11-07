"""E2E-DECISION-008: the model deciding alone against a decision engine, from one comparison run's `attempts.jsonl`
(ADR-0013 point 9). Reads only; it never starts a run.

The run pairs the arms per task and round, the order alternating per round (ABBA: control first in odd rounds, the
engine first in even ones), and a `harness.provider` failure is rerun with its arm and place. This script:
- keeps the last attempt of each rerun chain, and pairs the two arms by task and round (`round`, or for a file written
  before the field, the order of each task's attempts in its arm);
- drops a pair with a harness failure on either side (counted in all and per arm), or with one arm only;
- completion: each arm's pass rate with a 95 % Wilson interval, and the discordant pairs with an exact McNemar test;
- tokens (prompt plus completion), steps and wall time: the paired difference, engine minus control, with a seeded
  bootstrap 95 % interval of its mean;
- the order effect: the mean paired difference when the engine ran first and when the control did, and their gap (the
  carryover a warm cache or a shared workspace would cause), plus each metric's mean at place 1 and place 2;
- the engine: its calls, its latency's share of the engine arm's wall time, decisions by kind and fallbacks by reason;
- the routes: how often the engine chose each route before a step, and the tokens each route spent, read from the
  run's `events/` (each attempt's event log, by its conversation);
- the verdict, ADR-0013's rule: a saving is claimed only where its interval lies wholly below zero, and a completion
  drop is a regression, whatever the saving, where the paired completion difference's interval lies wholly below zero.

With fewer than 10 pairs the report says it makes no claim (TEST-PLAN: no claim without this data).

Usage: decision_compare.py RUN_DIR [--json] [--out FILE]
Run the tests: python3 -m unittest tests/e2e/test_decision_compare.py
"""

import argparse
import json
import math
import random
import sys
from collections import Counter, defaultdict
from dataclasses import dataclass
from pathlib import Path

METRICS = ("tokens", "steps", "wall_ms")
MIN_PAIRS = 10
RESAMPLES = 10_000
SEED = 8
Z = 1.959963984540054


@dataclass
class Pair:
    task: str
    round: int
    model: dict
    engine: dict

    @property
    def engine_first(self):
        return self.engine["pair_position"] == 1


def load(run):
    """Every attempt of a run directory, in the order written."""
    path = Path(run) / "attempts.jsonl"
    return [json.loads(line) for line in path.read_text().splitlines() if line.strip()]


def value(record, metric):
    if metric == "tokens":
        return record["prompt_tokens"] + record["completion_tokens"]
    return record[metric]


def finals(records):
    """The last attempt of each rerun chain, each with the attempt number its chain started from."""
    by_number = {record["attempt"]: record for record in records}
    rerun = {record["rerun_of"] for record in records if record.get("rerun_of") is not None}
    kept = []
    for record in records:
        if record["attempt"] in rerun:
            continue
        root = record
        while root.get("rerun_of") is not None and root["rerun_of"] in by_number:
            root = by_number[root["rerun_of"]]
        kept.append((root["attempt"], record))
    return kept


def pairs(records):
    """The complete pairs, and the dropped ones counted by reason."""
    slots = defaultdict(dict)
    order = defaultdict(list)
    for root, record in finals(records):
        if "round" in record:
            slots[(record["task_id"], record["round"])][record["arm"]] = record
        else:
            order[(record["task_id"], record["arm"])].append((root, record))
    # A file from before `round`: the schedule starts a task's rounds in order, so its k-th attempt in an arm (by the
    # number its chain started from) is round k.
    for (task, arm), chain in order.items():
        for round_, (_, record) in enumerate(sorted(chain, key=lambda item: item[0]), start=1):
            slots[(task, round_)][arm] = record
    found, excluded = [], Counter()
    for (task, round_), arms in sorted(slots.items()):
        if set(arms) != {"model", "engine"}:
            excluded["one arm only"] += 1
        elif failed := [arm for arm, record in sorted(arms.items()) if record["class"].startswith("harness.")]:
            excluded["harness failure"] += 1
            # Per arm too: an arm that fails the harness more often loses more pairs, which biases what remains.
            for arm in failed:
                excluded[f"harness failure ({arm})"] += 1
        else:
            found.append(Pair(task, round_, arms["model"], arms["engine"]))
    return found, dict(excluded)


def mcnemar_exact(model_only, engine_only):
    """Two-sided exact McNemar p-value: the smaller discordant count against Binomial(n, 1/2)."""
    n = model_only + engine_only
    if n == 0:
        return 1.0
    tail = sum(math.comb(n, i) for i in range(min(model_only, engine_only) + 1)) / 2 ** n
    return min(1.0, 2 * tail)


def wilson(passed, total):
    if total == 0:
        return (None, None)
    p = passed / total
    denominator = 1 + Z * Z / total
    centre = (p + Z * Z / (2 * total)) / denominator
    half = Z * math.sqrt(p * (1 - p) / total + Z * Z / (4 * total * total)) / denominator
    return (max(0.0, centre - half), min(1.0, centre + half))


def mean(values):
    return sum(values) / len(values) if values else None


def bootstrap_mean(values, resamples=RESAMPLES, seed=SEED):
    """A percentile bootstrap 95 % interval of the mean, seeded so a report is reproducible."""
    if not values:
        return (None, None)
    if len(values) == 1:
        return (values[0], values[0])
    rng = random.Random(seed)
    n = len(values)
    means = sorted(sum(rng.choice(values) for _ in range(n)) / n for _ in range(resamples))
    return (means[int(0.025 * resamples)], means[int(0.975 * resamples) - 1])


def routes(run, records):
    """The engine arm's routes, from each attempt's event log: the route chosen before each step (`call_model` at
    `before_step`), and the tokens of each step's usage on the route it called. An attempt without its log is counted
    as missing, never guessed."""
    chosen, tokens, missing = Counter(), Counter(), 0
    for record in records:
        path = Path(run) / "events" / f"{record.get('conversation_id')}.jsonl"
        if not record.get("conversation_id") or not path.is_file():
            missing += 1
            continue
        route = None
        for line in path.read_text().splitlines():
            event = json.loads(line).get("event") if line.strip() else None
            if not isinstance(event, dict):
                continue
            choice = event.get("choice") or {}
            if event.get("type") == "decision" and event.get("point") == "before_step" \
                    and choice.get("kind") == "call_model":
                route = choice["route"]
                chosen[route] += 1
            elif event.get("type") == "usage":
                tokens[route or "unknown"] += event.get("prompt_tokens", 0) + event.get("completion_tokens", 0)
    return {"chosen": dict(chosen), "tokens": dict(tokens), "missing": missing}


def verdict(report):
    """ADR-0013's rule over a report: no claim below MIN_PAIRS; a regression where the completion difference's interval
    lies wholly below zero, whatever the saving; a saving for each metric whose interval lies wholly below zero, a cost
    for each whose interval lies wholly above it; otherwise no measurable effect."""
    if report["pairs"] < MIN_PAIRS:
        return {"claim": "none", "savings": [], "costs": [], "regression": False}
    low, high = report["completion"]["difference"]["interval"]
    regression = high is not None and high < 0
    savings = [metric for metric in METRICS if (report["metrics"][metric]["interval"][1] or 0) < 0]
    costs = [metric for metric in METRICS if (report["metrics"][metric]["interval"][0] or 0) > 0]
    claim = "regression" if regression else "saving" if savings else "no_effect"
    return {"claim": claim, "savings": savings, "costs": costs, "regression": regression}


def analyse(records, run=None):
    if not any(record.get("arm") == "engine" for record in records):
        raise SystemExit("No engine arm in this run: it was not a comparison run (tqa-e2e --engine-profile)")
    found, excluded = pairs(records)
    passed = {arm: sum(getattr(pair, arm)["class"] == "pass" for pair in found) for arm in ("model", "engine")}
    model_only = sum(pair.model["class"] == "pass" and pair.engine["class"] != "pass" for pair in found)
    engine_only = sum(pair.engine["class"] == "pass" and pair.model["class"] != "pass" for pair in found)
    report = {
        "pairs": len(found),
        "excluded": excluded,
        "completion": {
            **{arm: {"passed": passed[arm], "rate": passed[arm] / len(found) if found else None,
                     "interval": list(wilson(passed[arm], len(found)))} for arm in ("model", "engine")},
            "model_only": model_only,
            "engine_only": engine_only,
            "p_value": mcnemar_exact(model_only, engine_only),
        },
        "metrics": {},
        "order": {"position": {}},
    }
    completed = [int(pair.engine["class"] == "pass") - int(pair.model["class"] == "pass") for pair in found]
    report["completion"]["difference"] = {"mean": mean(completed), "interval": list(bootstrap_mean(completed))}
    for metric in METRICS:
        differences = [value(pair.engine, metric) - value(pair.model, metric) for pair in found]
        report["metrics"][metric] = {
            "model_mean": mean([value(pair.model, metric) for pair in found]),
            "engine_mean": mean([value(pair.engine, metric) for pair in found]),
            "mean_difference": mean(differences),
            "interval": list(bootstrap_mean(differences)),
        }
        first = [value(pair.engine, metric) - value(pair.model, metric) for pair in found if pair.engine_first]
        second = [value(pair.engine, metric) - value(pair.model, metric) for pair in found if not pair.engine_first]
        engine_first, model_first = mean(first), mean(second)
        report["order"][metric] = {
            "engine_first": engine_first,
            "model_first": model_first,
            "carryover": None if engine_first is None or model_first is None else engine_first - model_first,
        }
        attempts = [record for pair in found for record in (pair.model, pair.engine)]
        report["order"]["position"][metric] = {
            "first": mean([value(record, metric) for record in attempts if record["pair_position"] == 1]),
            "second": mean([value(record, metric) for record in attempts if record["pair_position"] == 2]),
        }
    engine = [pair.engine for pair in found]
    kinds, fallbacks = Counter(), Counter()
    for record in engine:
        kinds.update(record["decisions"]["by_kind"])
        fallbacks.update(record["decisions"]["fallbacks"])
    wall = sum(record["wall_ms"] for record in engine)
    report["engine"] = {
        "engine_calls": sum(record["decisions"]["engine_calls"] for record in engine),
        "latency_share": sum(record["decisions"]["engine_latency_ms"] for record in engine) / wall if wall else None,
        "by_kind": dict(kinds),
        "fallbacks": dict(fallbacks),
    }
    if run is not None:
        report["routes"] = routes(run, engine)
    report["verdict"] = verdict(report)
    return report


def number(x, digits=1):
    return "–" if x is None else f"{x:.{digits}f}"


def markdown(report):
    lines = ["# E2E-DECISION-008: the model deciding alone against the decision engine", ""]
    excluded = ", ".join(f"{count} {reason}" for reason, count in sorted(report["excluded"].items())) or "none"
    lines.append(f"Pairs (task and round, both arms, no harness failure): {report['pairs']}; dropped: {excluded}.")
    if report["pairs"] < MIN_PAIRS:
        lines += ["", f"**Too few pairs ({report['pairs']} of at least {MIN_PAIRS}): no claim either way.** "
                      "The numbers below describe this run only."]
    found = report["verdict"]
    if found["claim"] != "none":
        lines += ["", "## Verdict (ADR-0013's rule)", "", {
            "regression": "**A regression:** completion drops with the engine beyond its interval, whatever it saves.",
            "saving": "**A saving:** " + ", ".join(found["savings"]) + ", each interval wholly below zero.",
            "no_effect": "**No measurable effect:** no interval excludes zero.",
        }[found["claim"]]]
        if found["costs"]:
            lines.append("Costs more with the engine: " + ", ".join(found["costs"]) + " (interval wholly above zero).")
    completion = report["completion"]
    lines += ["", "## Completion", "", "| Arm | Passed | Rate | 95 % Wilson |", "|---|---|---|---|"]
    for arm, label in (("model", "Model decides"), ("engine", "Engine")):
        row = completion[arm]
        low, high = row["interval"]
        rate = None if row["rate"] is None else 100 * row["rate"]
        interval = "–" if low is None else f"{100 * low:.1f}–{100 * high:.1f} %"
        lines.append(f"| {label} | {row['passed']} | {number(rate)} % | {interval} |")
    lines += ["", f"Discordant pairs: {completion['model_only']} passed with the model only, "
                  f"{completion['engine_only']} with the engine only; exact McNemar p = {completion['p_value']:.4f}."]
    low, high = completion["difference"]["interval"]
    lines.append(f"Paired completion difference (engine minus model): {number(completion['difference']['mean'], 3)}, "
                 f"95 % bootstrap {number(low, 3)} to {number(high, 3)}.")
    lines += ["", "## Tokens, steps and wall time (paired, engine minus model)", "",
              "| Metric | Model mean | Engine mean | Mean difference | 95 % bootstrap |", "|---|---|---|---|---|"]
    for metric in METRICS:
        row = report["metrics"][metric]
        low, high = row["interval"]
        lines.append(f"| {metric} | {number(row['model_mean'])} | {number(row['engine_mean'])} | "
                     f"{number(row['mean_difference'])} | {number(low)} to {number(high)} |")
    lines += ["", "## Order effect (ABBA)", "",
              "Mean paired difference (engine minus model) by which arm ran first, and their gap. A gap far from 0 "
              "means the second attempt of a pair gains or loses from the first (a warm cache, another workload on "
              "the same machine).", "",
              "| Metric | Engine first | Model first | Gap | Place 1 mean | Place 2 mean |",
              "|---|---|---|---|---|---|"]
    for metric in METRICS:
        row, place = report["order"][metric], report["order"]["position"][metric]
        lines.append(f"| {metric} | {number(row['engine_first'])} | {number(row['model_first'])} | "
                     f"{number(row['carryover'])} | {number(place['first'])} | {number(place['second'])} |")
    engine = report["engine"]
    share = None if engine["latency_share"] is None else 100 * engine["latency_share"]
    lines += ["", "## The engine", "",
              f"Engine calls: {engine['engine_calls']}; its latency is {number(share)} % of the engine arm's "
              "wall time.",
              "Decisions: " + (", ".join(f"{k} {v}" for k, v in sorted(engine["by_kind"].items())) or "none") + ".",
              "Fallbacks: " + (", ".join(f"{k} {v}" for k, v in sorted(engine["fallbacks"].items())) or "none") + "."]
    if "routes" in report:
        found = report["routes"]
        lines += ["", "## Routes (the engine arm)", "", "| Route | Steps it was chosen for | Tokens |", "|---|---|---|"]
        for route in sorted(set(found["chosen"]) | set(found["tokens"])):
            lines.append(f"| {route} | {found['chosen'].get(route, 0)} | {found['tokens'].get(route, 0)} |")
        if found["missing"]:
            lines.append(f"\n{found['missing']} engine attempt(s) had no event log: not counted.")
    return "\n".join(lines) + "\n"


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("run", help="the comparison run's --out directory")
    parser.add_argument("--json", action="store_true", help="print the report as JSON instead of Markdown")
    parser.add_argument("--out", help="write the report to this file instead of standard output")
    args = parser.parse_args(argv)
    report = analyse(load(args.run), args.run)
    text = json.dumps(report, indent=2) + "\n" if args.json else markdown(report)
    if args.out:
        Path(args.out).write_text(text)
    else:
        sys.stdout.write(text)


if __name__ == "__main__":
    main()

"""The agent-teams evaluation's blind judging (docs/evals/agent-teams.md section 2): packets that don't name their
side, the judges' score sheets, and the scores they settle to.

- **Packets:** each attempt's task statement, its key facts, its final answer and its changed files go under a random
  id. Words that name a side become "a colleague" (or "colleagues") in the answer, with their article: the team's
  helper names and roles, and "helper", "agent team", "team member" and "sub-agent"; in file paths a helper's name
  becomes "part". The words read naturally, but they still appear only in team answers: a residual tell the report
  names, and measures through the judges' guesses. The side map (packet id to task, side and attempt) is written
  apart, for whoever joins the scores back, never for the judges.
- **Sheets:** one CSV per judge, a row per packet in the packets' shuffled order: correct, complete, grounded,
  coherent and usable, each 0, 1 or 2, and `side_guess`, the judge's guess at the side (single, team, or blank when
  unsure), so the report measures the blindness instead of assuming it.
- **Settling:** two judges who agree settle a packet at the mean of their totals (each total the sum of the five
  criteria, 0 to 10). A packet goes to the third judge when any criterion is scored 0 against 2, or when the two
  totals differ by more than two points; the third judge's whole score of it then settles it.
"""

import csv
import json
import random
import re
import shutil
from pathlib import Path

CRITERIA = ("correct", "complete", "grounded", "coherent", "usable")
SCORES = (0, 1, 2)
COLLEAGUE, COLLEAGUES = "a colleague", "colleagues"
GUESSES = ("single", "team")
# An article or possessive before a redacted word goes with it, so "the analyst" and "my helpers" read naturally.
ARTICLE = r"(?:\b(?:my|our|the|a|an|its|their|his|her|this|that|these|those|each|one)\s+)?"
# Words that would tell a judge the side (the team's tools never reach a packet: only the final answer and files do).
SIDE_SINGULAR = re.compile(ARTICLE + r"\b(?:helper|team member|sub[- ]?agent)\b", re.IGNORECASE)
SIDE_PLURAL = re.compile(ARTICLE + r"\b(?:agent[- ]teams?|team members|helpers|sub[- ]?agents|team)\b",
                         re.IGNORECASE)


def _as(words):
    """A replacement keeping the capital of what it replaces, as at a sentence's start."""
    return lambda match: words[0].upper() + words[1:] if match.group(0)[0].isupper() else words


def redact(text, names=()):
    """`text` with the words that name a side as "a colleague": the given helper names and roles, then the side
    words (plural ones as "colleagues")."""
    for name in sorted({name for name in names if name}, key=len, reverse=True):
        text = re.sub(ARTICLE + rf"\b{re.escape(name)}\b", _as(COLLEAGUE), text, flags=re.IGNORECASE)
    text = SIDE_SINGULAR.sub(_as(COLLEAGUE), text)
    return SIDE_PLURAL.sub(_as(COLLEAGUES), text)


def redact_path(path, names=()):
    """A changed file's path with any helper name replaced, so a taken file (uploads/<helper>-<name>) names no side."""
    out = path
    for name in sorted({name for name in names if name}, key=len, reverse=True):
        out = re.sub(rf"(?<![A-Za-z0-9]){re.escape(name)}(?![A-Za-z0-9])", "part", out, flags=re.IGNORECASE)
    return out


def packet_path(path, names, taken):
    """Where a changed file goes in its packet. A file the lead took from a helper (uploads/<helper>-<name>, a layout a
    single agent rarely makes) moves to <name> at the top, or part-<name> when the attempt has a file there already;
    any other path keeps its place with helper names replaced."""
    for name in sorted({name for name in names if name}, key=len, reverse=True):
        match = re.fullmatch(rf"uploads/{re.escape(name)}-(.+)", path, flags=re.IGNORECASE)
        if match:
            rest = redact_path(match.group(1), names)
            return f"part-{rest}" if rest in taken else rest
    return redact_path(path, names)


def make_packets(attempts, out, seed):
    """Writes one packet per attempt under `out`/packets/<id>/ and the side map to `out`/side-map.json; answers the
    packet ids in their shuffled order.

    `attempts` is a list of dicts: task, side, attempt, statement (task.md), key_facts (list of strings), answer (the
    final answer's text), files ({path: text} of changed files), helpers (the team's helper names and roles, each
    redacted; empty for a single agent)."""
    out = Path(out)
    rng = random.Random(seed)
    ids = set()
    side_map = {}
    for record in attempts:
        packet = f"{rng.getrandbits(40):010x}"
        while packet in ids:
            packet = f"{rng.getrandbits(40):010x}"
        ids.add(packet)
        names = record.get("helpers") or ()
        directory = out / "packets" / packet
        if directory.exists():
            shutil.rmtree(directory)
        (directory / "files").mkdir(parents=True)
        (directory / "task.md").write_text(record["statement"])
        (directory / "key-facts.md").write_text("".join(f"- {fact}\n" for fact in record["key_facts"]))
        (directory / "answer.md").write_text(redact(record["answer"], names))
        for path, text in sorted(record["files"].items()):
            if Path(path).is_absolute() or ".." in Path(path).parts:
                raise ValueError(f"a changed file's path leaves the packet: {path}")
            placed = packet_path(path, names, set(record["files"]))
            target = directory / "files" / placed
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_text(redact(text, names))
        side_map[packet] = {"task": record["task"], "side": record["side"], "attempt": record["attempt"]}
        if record.get("batch") is not None:
            side_map[packet]["batch"] = record["batch"]
    order = sorted(ids)
    rng.shuffle(order)
    (out / "side-map.json").write_text(json.dumps(side_map, indent=2, sort_keys=True) + "\n")
    (out / "order.json").write_text(json.dumps(order) + "\n")
    return order


def write_sheet(path, order):
    """An empty score sheet for one judge, a row per packet in the shuffled order: the criteria, then side_guess
    ("which side do you think this was?": single, team, or blank when unsure), then notes."""
    with open(path, "w", newline="") as handle:
        writer = csv.writer(handle)
        writer.writerow(("packet",) + CRITERIA + ("side_guess", "notes"))
        for packet in order:
            writer.writerow((packet,) + ("",) * len(CRITERIA) + ("", ""))


def read_guesses(path):
    """{packet: "single" or "team"} from a judge's side_guess column; a blank is unsure and left out, anything else is
    refused naming its row."""
    found = {}
    with open(path, newline="") as handle:
        for number, row in enumerate(csv.DictReader(handle), start=2):
            guess = (row.get("side_guess") or "").strip().lower()
            if not guess:
                continue
            if guess not in GUESSES:
                raise ValueError(f"{path}: row {number}: side_guess {guess!r} is not single, team or blank")
            found[row["packet"].strip()] = guess
    return found


def guess_accuracy(guesses, side_map):
    """How well one judge guessed the sides: guesses made, right, the share right with its Wilson interval, and the
    packets left unsure. Chance is one half; an interval above it means the packets told the judge the side."""
    from stats import wilson
    made = {packet: guess for packet, guess in guesses.items() if packet in side_map}
    right = sum(1 for packet, guess in made.items() if side_map[packet]["side"] == guess)
    return {"guessed": len(made), "right": right, "share": right / len(made) if made else None,
            "interval": wilson(right, len(made)), "unsure": len(side_map) - len(made)}


def read_sheet(path, order=None):
    """{packet: {criterion: score}} from a judge's sheet; a score that isn't 0, 1 or 2 is refused, naming its cell, and
    so are a packet's second row and, given the packets' order, a packet that isn't one of them."""
    found = {}
    known = set(order) if order is not None else None
    with open(path, newline="") as handle:
        for number, row in enumerate(csv.DictReader(handle), start=2):
            packet = row["packet"].strip()
            if packet in found:
                raise ValueError(f"{path}: row {number}: packet {packet} is scored twice")
            if known is not None and packet not in known:
                raise ValueError(f"{path}: row {number}: {packet} is not one of the packets")
            scores = {}
            for criterion in CRITERIA:
                cell = (row.get(criterion) or "").strip()
                if cell == "":
                    continue
                if cell not in {str(score) for score in SCORES}:
                    raise ValueError(f"{path}: packet {packet}, {criterion}: {cell!r} is not 0, 1 or 2")
                scores[criterion] = int(cell)
            found[packet] = scores
    return found


def split(a, b):
    """Whether two judges' full scores of a packet need the third: a criterion scored 0 against 2, or totals more than
    two points apart (small differences that add up), the rule in docs/evals/agent-teams.md."""
    if any(abs(a[criterion] - b[criterion]) > 1 for criterion in CRITERIA):
        return True
    return abs(sum(a.values()) - sum(b.values())) > 2


def settle(first, second, third=None, order=()):
    """The settled scores from two judges' sheets (and the third's, for splits): ({packet: total}, [packets waiting for
    the third], [packets the two judges haven't both fully scored]). Two judges who agree settle a packet at the mean
    of their totals; on a split the third judge's whole score of the packet settles it. `order` adds the packets no
    sheet has a row for, so none goes missing from the report."""
    third = third or {}
    totals = {}
    waiting = []
    incomplete = []
    for packet in sorted(set(first) | set(second) | set(order)):
        a, b = first.get(packet, {}), second.get(packet, {})
        if any(criterion not in a or criterion not in b for criterion in CRITERIA):
            incomplete.append(packet)
            continue
        if not split(a, b):
            totals[packet] = (sum(a.values()) + sum(b.values())) / 2
        elif all(criterion in third.get(packet, {}) for criterion in CRITERIA):
            totals[packet] = float(sum(third[packet][criterion] for criterion in CRITERIA))
        else:
            waiting.append(packet)
    return totals, waiting, incomplete


def join(totals, side_map, batch=None):
    """{(batch, task, side, attempt): score} for the packets settled so far. A side map made before batches (one run,
    no `batch` in its entries) takes `batch`: the one run the report reads."""
    return {(entry.get("batch", batch), entry["task"], entry["side"], entry["attempt"]): totals[packet]
            for packet, entry in side_map.items() if packet in totals}

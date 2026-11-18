"""The rounds run's task (ADR-0034's rounds amendment; E2E-AGENTS-041): a first ask, then a person's follow-up that
opens the team's next round. Tagged teams-rounds, never teams-eval: the evaluation's 20 tasks are unchanged. Run with
`runner.py --tag teams-rounds --side team` (and `stack_dry_run.py --tag teams-rounds` first)."""
import csv
import io
import random
from decimal import Decimal

from common import Task

REGIONS = ("north", "south", "east", "west", "central")


def x1():
    rng = random.Random(901)
    buffer = io.StringIO()
    writer = csv.writer(buffer, lineterminator="\n")
    writer.writerow(("region", "amount"))
    totals = {region: Decimal(0) for region in REGIONS}
    for _ in range(60):
        region = rng.choice(REGIONS)
        amount = Decimal(rng.randint(500, 50_000)) / 100
        totals[region] += amount
        writer.writerow((region, f"{amount:.2f}"))
    whole = sum(totals.values())
    shares = {region: round(totals[region] * 100 / whole) for region in REGIONS}

    def table(with_shares):
        head = "| Region | Total | Share |\n|---|---|---|\n" if with_shares else "| Region | Total |\n|---|---|\n"
        rows = "".join(
            f"| {region.capitalize()} | {totals[region]:.2f} |" + (f" {shares[region]} % |" if with_shares else "")
            + "\n" for region in REGIONS)
        return {"regions.md": "# Sales by region\n\n" + head + rows}

    check = '''
from checklib import expected, fail, numbers, read, run, table

def check(workspace):
    truth = expected()
    rows = table(read(workspace, "regions.md"))
    if len(rows) < 2:
        fail("regions.md has no markdown table")
    wrong = []
    for region, total in truth["totals"].items():
        row = next((row for row in rows[1:] if row and region in row[0].lower()), None)
        if row is None or len(row) < 3:
            fail(f"the table has no row for {region} with a total and a share")
        values = numbers(row[1])
        if not values or abs(values[0] - total) > 0.005:
            wrong.append(f"{region}: total {row[1]!r}, the data gives {total:.2f}")
        shares = numbers(row[2])
        if not shares or abs(shares[0] - truth["shares"][region]) > 1:
            wrong.append(f"{region}: share {row[2]!r}, the data gives {truth['shares'][region]} %")
    if wrong:
        fail(f"{len(wrong)} value(s) wrong", "\\n".join(wrong))
    return "every region's total is exact and its share within a point"

run(check)
'''
    return Task(
        id="x1-region-shares", title="Region totals, then each region's share on a follow-up", kind="report",
        statement="""
The file `sales.csv` lists sales by region (columns `region` and `amount`, in euros). Write `regions.md` with a markdown
table: one row per region, with its total amount to the cent.
""",
        follow_up=("Thank you. Now add a third column to the table in regions.md with each region's share of the "
                   "overall total, as a whole percent. If a helper worked out the totals, bring that same helper back "
                   "for this."),
        seed={"sales.csv": buffer.getvalue()},
        check=check,
        expected={"totals": {region: float(totals[region]) for region in REGIONS}, "shares": shares},
        key_facts=["Each region's total is the sum of its amounts, to the cent.",
                   "Each region's share is its total over the overall total, as a whole percent."],
        solution=table(True),
        wrong=table(False),
        budgets=(80, 1800, 1_000_000),
    )


TASKS = [x1]

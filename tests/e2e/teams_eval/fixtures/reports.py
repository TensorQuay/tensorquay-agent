"""The report tasks P1-P5: a written report from workspace sources, checked on its facts, with key facts for the
judges."""

import csv
import io
import random
from decimal import Decimal

from common import Task

P1_REGIONS = ("north", "south", "east", "west")
P1_PRODUCTS = {"router": Decimal("129.00"), "switch": Decimal("89.50"), "cable": Decimal("4.25"),
               "antenna": Decimal("37.80")}
P1_TARGETS = {"north": Decimal("52000"), "south": Decimal("47000"), "east": Decimal("61000"),
              "west": Decimal("39000")}


def p1():
    rng = random.Random(201)
    seed, q3, q3_and_oct = {}, {}, {}
    for region in P1_REGIONS:
        buffer = io.StringIO()
        writer = csv.writer(buffer, lineterminator="\n")
        writer.writerow(("date", "product", "units", "unit_price"))
        rows = []
        for month in range(7, 13):
            for _ in range(rng.randint(10, 14)):
                product = rng.choice(sorted(P1_PRODUCTS))
                units = rng.randint(1, 60) if rng.random() > 0.08 else -rng.randint(1, 5)  # a refund
                rows.append((f"2025-{month:02d}-{rng.randint(1, 28):02d}", product, units))
        rows.sort()
        for date, product, units in rows:
            writer.writerow((date, product, units, P1_PRODUCTS[product]))
            amount = P1_PRODUCTS[product] * units
            if date < "2025-10":
                q3[region] = q3.get(region, Decimal(0)) + amount
            if date < "2025-11":
                q3_and_oct[region] = q3_and_oct.get(region, Decimal(0)) + amount
        seed[f"sales/{region}.csv"] = buffer.getvalue()
    seed["targets.md"] = "# Q3 2025 targets (net sales, EUR)\n\n" + "".join(
        f"- {region.capitalize()}: {target:,} EUR\n" for region, target in P1_TARGETS.items())

    svg = ('<svg xmlns="http://www.w3.org/2000/svg" width="400" height="200">'
           + "".join(f'<rect x="{20 + 90 * index}" y="20" width="60" height="150" fill="#4a7"/>'
                     for index in range(4)) + "</svg>\n")

    def answer(totals):
        rows = ["| Region | Q3 sales (EUR) | Target (EUR) | Difference |", "|---|---|---|---|"]
        for region in P1_REGIONS:
            total = totals[region].quantize(Decimal("0.01"))
            rows.append(f"| {region.capitalize()} | {total:,} | {P1_TARGETS[region]:,} | "
                        f"{total - P1_TARGETS[region]:+,} |")
        return {"report.md": "# Q3 2025 sales by region\n\n" + "\n".join(rows) + "\n\n![Q3 sales](chart.svg)\n",
                "chart.svg": svg}

    check = '''
import re
from checklib import expected, fail, numbers, read, run, table

def check(workspace):
    truth = expected()
    text = read(workspace, "report.md")
    rows = table(text)
    if len(rows) < 2:
        fail("report.md has no markdown table")
    wrong = []
    for region, total in truth["totals"].items():
        row = next((row for row in rows[1:] if row and region in row[0].lower()), None)
        if row is None or len(row) < 2:
            fail(f"the table has no row for {region}")
        values = numbers(row[1])
        if not values or abs(values[0] - total) > 0.005:
            wrong.append(f"{region}: {row[1]!r}, the data gives {total:.2f}")
    if wrong:
        fail(f"{len(wrong)} regional total(s) wrong", "\\n".join(wrong))
    images = re.findall(r"!\\[[^\\]]*\\]\\(([^)\\s]+)\\)", text)
    charts = [name for name in images if (workspace / name).is_file() and (workspace / name).stat().st_size > 0]
    if not charts and "```mermaid" not in text:
        fail("report.md has no chart: no image file it links to, and no mermaid block")
    return "every region's Q3 total is exact, with a chart"

run(check)
'''
    return Task(
        id="p1-regional-sales", title="A one-page Q3 sales report by region with a chart", kind="report",
        statement="""
The folder `sales/` holds a CSV file per region with every sale from July to December 2025 (negative units are
refunds). Write `report.md`, a one-page report of Q3 2025 (July to September) net sales by region against the
targets in `targets.md`: a markdown table with the columns Region, Q3 sales (EUR), Target (EUR) and Difference,
a short commentary, and a chart of sales against target, as an image file in the workspace that `report.md` links
to or as a mermaid block.
""",
        seed=seed, check=check, budgets=(60, 1500, 800_000),
        expected={"totals": {region: float(total.quantize(Decimal("0.01"))) for region, total in q3.items()}},
        key_facts=[f"{region.capitalize()}: {q3[region].quantize(Decimal('0.01')):,} EUR against "
                   f"{P1_TARGETS[region]:,} ({'above' if q3[region] >= P1_TARGETS[region] else 'below'} target)"
                   for region in P1_REGIONS] + ["Refunds (negative units) reduce the totals; October to December "
                                                "is not Q3"],
        solution=answer(q3), wrong=answer(q3_and_oct))


# (the check's word, owner, date, how the notes put it)
P2_DECISIONS = (
    ("launch", "Dana", "2026-01-08", "Decided: the launch moves to 14 April."),
    ("mysql", "Ken", "2026-02-12", "We reversed the Postgres decision of 15 January: we stay on MySQL this year."),
    ("pricing page", "Lena", "2026-01-15", "We agreed that Lena redesigns the pricing page."),
    ("on-call", "Omar", "2026-01-22", "Decision: on-call rotates weekly, starting with Omar."),
    ("beta", "Dana", "2026-01-29", "Outcome: the beta is limited to 50 customers; Dana picks them."),
    ("payment provider", "Ken", "2026-02-05", "Final call: Brisa becomes our payment provider; Ken owns the "
     "contract."),
    ("hiring", "Priya", "2026-02-05", "Decided: we hire two engineers this quarter; Priya leads the hiring."),
    ("logo", "Lena", "2026-02-12", "We agreed to keep the current logo; Lena tells the agency."),
    ("sso", "Omar", "2026-02-19", "Decision: single sign-on (SSO) ships in Q2; Omar owns it."),
    ("travel", "Priya", "2026-02-26", "Outcome: the travel budget is cut by 20 %; Priya informs the teams."),
    ("docs site", "Lena", "2026-03-05", "Decided: the docs site moves to the new generator; Lena owns the move."),
    ("freeze", "Omar", "2026-03-05", "We agreed on a code freeze one week before launch; Omar enforces it."),
    ("support hours", "Dana", "2026-03-12", "Final call: support hours extend to 8 pm; Dana staffs the shifts."),
    ("analytics", "Ken", "2026-03-19", "Decision: we drop the old analytics tool by April; Ken removes it."),
    ("offsite", "Priya", "2026-03-26", "Decided: the team offsite is in May; Priya books it."),
)
P2_QUESTIONS = (("retention", "How long do we keep customer data (the retention period)?"),
                ("mobile", "Who owns the mobile app after the reorganisation?"),
                ("browser", "Which old browsers do we still support?"),
                ("conference", "Is there budget for the autumn conference?"),
                ("translation", "Which translation vendor do we use for the German site?"))
P2_CHATTER = ("We discussed SSO again but did not decide anything.", "Ken suggested Postgres; to be confirmed.",
              "Lena showed early sketches.", "Everyone agreed the dashboard is slow, no action yet.",
              "Omar asked about the rota; we come back to it next week.", "Dana raised the beta size; no decision.")


def p2():
    rng = random.Random(202)
    dates = sorted({date for _, _, date, _ in P2_DECISIONS} | {"2026-01-15", "2026-03-19"})
    notes = {date: [] for date in dates}
    for word, owner, date, text in P2_DECISIONS:
        notes[date].append(text)
    notes["2026-01-15"].append("Decided: we migrate the database to Postgres; Ken plans it.")
    for index, (_, question) in enumerate(P2_QUESTIONS):
        notes[dates[2 + 2 * index]].append(f"Open question: {question}")
    seed = {}
    for date in dates[:12]:
        lines = notes[date] + rng.sample(P2_CHATTER, 2)
        rng.shuffle(lines)
        seed[f"meetings/{date}.md"] = (f"# Team meeting {date}\n\nPresent: Dana, Ken, Lena, Omar, Priya\n\n"
                                       + "".join(f"- {line}\n" for line in lines))
    assert len(seed) == 12 and all(any(date in name for name in seed) for _, _, date, _ in P2_DECISIONS)

    def answer(decisions):
        rows = ["| Decision | Owner | Date |", "|---|---|---|"]
        rows += [f"| {text} | {owner} | {date} |" for _, owner, date, text in decisions]
        questions = "".join(f"- {question}\n" for _, question in P2_QUESTIONS)
        return "# Decisions, January to March 2026\n\n" + "\n".join(rows) + "\n\n## Open questions\n\n" + questions

    wrong = [entry for entry in P2_DECISIONS if entry[0] != "mysql"][:11] + [
        ("mysql", "Ken", "2026-01-15", "We migrate the database to Postgres.")]
    check = '''
from checklib import expected, fail, read, run, sections, table

def check(workspace):
    truth = expected()
    text = read(workspace, "decisions.md")
    rows = [" ".join(row).lower() for row in table(text)[1:]]
    if not rows:
        fail("decisions.md has no markdown table")
    found = [d["word"] for d in truth["decisions"]
             if any(d["word"] in row and d["owner"].lower() in row for row in rows)]
    stale = any("postgres" in row and "mysql" not in row for row in rows)
    open_text = " ".join(body.lower() for heading, body in sections(text) if "open" in heading.lower())
    questions = [word for word in truth["questions"] if word in open_text]
    message = f"{len(found)} of 15 decisions with their owners, {len(questions)} of 5 open questions"
    if len(found) < truth["pass_at"] or len(questions) < 4 or stale:
        detail = "the reversed Postgres decision is listed as standing" if stale else ""
        fail(message, detail)
    return message

run(check)
'''
    return Task(
        id="p2-meeting-decisions", title="Decisions, owners and open questions from twelve meetings", kind="report",
        statement="""
The folder `meetings/` holds the notes of 12 team meetings from January to March 2026. Write `decisions.md`: a
markdown table of every decision still standing, with the columns Decision, Owner and Date, and then a section
`## Open questions` listing the questions still open. A decision later reversed is listed as it finally stands.
""",
        seed=seed, check=check, budgets=(60, 1500, 800_000),
        expected={"decisions": [{"word": word, "owner": owner} for word, owner, _, _ in P2_DECISIONS],
                  "questions": [word for word, _ in P2_QUESTIONS], "pass_at": 13},
        key_facts=[f"{date}: {text} (owner {owner})" for _, owner, date, text in P2_DECISIONS]
        + ["Postgres (15 Jan) was reversed on 12 Feb"] + [question for _, question in P2_QUESTIONS],
        solution={"decisions.md": answer(P2_DECISIONS)}, wrong={"decisions.md": answer(wrong)})


# (the check's words, owner, source, what the source says)
P3_RISKS = (
    (("vendor", "shipping", "deliver"), "Ken", "mails/2026-02-03-ken.txt",
     "The hardware vendor told me shipping slips by six weeks; I am chasing them."),
    (("key person", "leave", "lead engineer", "attrition"), "Priya", "mails/2026-02-10-priya.txt",
     "Our lead engineer has an offer elsewhere; I am working on a retention plan."),
    (("budget", "overrun", "hardware cost"), "Omar", "budget.csv", None),
    (("regulat",), "Dana", "mails/2026-02-14-dana.txt",
     "The regulator wants to review the data flows before go-live; I am preparing the file."),
    (("migration", "data quality", "legacy data"), "Ken", "mails/2026-02-20-ken.txt",
     "A third of the legacy records fail validation; the migration may need a cleanup sprint, which I will run."),
    (("audit", "security"), "Omar", "plan.md", None),
    (("scope", "sales request"), "Dana", "mails/2026-02-24-dana.txt",
     "Sales keeps adding requests to the first release; I will hold the scope line."),
    (("integration", "payment provider", "api change"), "Lena", "mails/2026-03-02-lena.txt",
     "The payment provider changes its API in April, in the middle of our integration; I own the adapter."),
    (("training",), "Priya", "mails/2026-03-05-priya.txt",
     "Store staff will not be trained before go-live unless we book the trainers this month; I will book them."),
    (("capacity", "power", "datacenter", "data centre"), "Lena", "mails/2026-03-09-lena.txt",
     "The data centre says our rack's power is near its cap; adding servers needs a new circuit. I am on it."),
)


def p3():
    seed = {"project/plan.md": (
        "# Store rollout plan\n\nGo-live: 1 June 2026.\n\n## Milestones\n- March: hardware installed\n"
        "- April: data migrated\n- May: staff trained\n\n## Dependencies\nThe external security audit must pass "
        "before go-live; findings could delay us. Owner: Omar.\n"),
        "project/budget.csv": ("line,budget_eur,spent_eur,owner\nhardware,120000,110400,Omar\n"
                               "software,40000,12000,Ken\ntraining,15000,0,Priya\ncontractors,60000,21000,Dana\n"),
        "project/mails/2026-01-20-omar.txt": "From: Omar\nSubject: hardware spend\n\nWe have spent 92 % of the "
                                             "hardware budget and half the stores are still to come.\n"}
    for _, owner, source, text in P3_RISKS:
        if text:
            seed[f"project/{source}"] = f"From: {owner}\nSubject: status\n\n{text}\n"

    def answer(risks):
        rows = ["| Risk | Likelihood | Impact | Owner |", "|---|---|---|---|"]
        rows += [f"| {words[0].capitalize()} risk | Medium | High | {owner} |" for words, owner, _, _ in risks]
        return "# Risk register\n\n" + "\n".join(rows) + "\n"

    wrong = [(words, "Dana" if owner != "Dana" else "Ken", source, text)
             for words, owner, source, text in P3_RISKS]
    check = '''
from checklib import expected, fail, read, run, table

def check(workspace):
    truth = expected()
    rows = table(read(workspace, "risks.md"))
    if len(rows) < 2:
        fail("risks.md has no markdown table")
    body = [[cell.lower() for cell in row] for row in rows[1:]]
    found, owned = 0, 0
    for risk in truth["risks"]:
        matching = [row for row in body if row and any(word in row[0] for word in risk["words"])]
        if matching:
            found += 1
            if any(risk["owner"].lower() in " ".join(row[1:]) for row in matching):
                owned += 1
    empty = [row[0] for row in body if len(row) < 4 or not row[1] or not row[2]]
    message = f"{found} of 10 risks found, {owned} with the right owner"
    if found < 9 or owned < 8 or empty:
        fail(message, ("no likelihood or impact: " + ", ".join(empty)) if empty else "")
    return message

run(check)
'''
    return Task(
        id="p3-risk-register", title="A risk register from a project's plan, budget and mails", kind="report",
        statement="""
The folder `project/` holds a store rollout's plan, its budget sheet and status mails. Write `risks.md`: a risk
register as a markdown table with the columns Risk, Likelihood, Impact and Owner, one row per risk the sources show,
with the owner the sources name.
""",
        seed=seed, check=check, budgets=(60, 1500, 800_000),
        expected={"risks": [{"words": list(words), "owner": owner} for words, owner, _, _ in P3_RISKS]},
        key_facts=[f"{words[0]} (owner {owner}, from {source})" for words, owner, source, _ in P3_RISKS]
        + ["Hardware is 92 % spent with half the stores to go (budget overrun)", "Go-live is 1 June 2026"],
        solution={"risks.md": answer(P3_RISKS)}, wrong={"risks.md": answer(wrong)})


P4_QUESTION = "Does a four-day work week change productivity?"
# (file, title, two claims as (sentence, the words the check looks for))
P4_PAPERS = (
    ("haug-2023.txt", "Output in a four-day week: a trial in 61 firms",
     (("Output per employee stayed the same in 52 of 61 firms.", ("61 firms",)),
      ("Sick days fell by 65 %.", ("65",)))),
    ("ortiz-2022.txt", "Compressed hours in a call centre",
     (("Calls handled per hour rose by 8 %.", ("8 %", "8%")),
      ("Customer satisfaction did not change.", ("satisfaction",)))),
    ("lund-2024.txt", "The four-day week in public services",
     (("Waiting times rose in two of five agencies.", ("waiting time",)),
      ("The trial was ended early in one agency for staffing gaps.", ("staffing",)))),
    ("mehta-2023.txt", "Burnout and the shorter week",
     (("Self-reported burnout fell by a third.", ("burnout",)),
      ("Effects faded after twelve months in a follow-up.", ("twelve months", "12 months")))),
    ("chen-2021.txt", "Software teams on 32 hours",
     (("Story points delivered did not fall.", ("story points",)),
      ("Meetings were cut by 40 %.", ("meeting",)))),
    ("adeyemi-2024.txt", "Manufacturing shifts and the four-day week",
     (("Output fell by 4 % on lines that cannot speed up.", ("4 %", "4%")),
      ("Overtime costs rose.", ("overtime",)))),
    ("novak-2022.txt", "Selection effects in voluntary trials",
     (("Firms that volunteer for trials were already more productive.", ("volunteer",)),
      ("Most trials have no control group.", ("control group",)))),
    ("sato-2023.txt", "Hours, rest and cognitive performance",
     (("Error rates in cognitive tests fell with more rest days.", ("error rate",)),
      ("The study was a lab experiment with 120 participants.", ("120",)))),
)


def p4():
    seed = {}
    for name, title, claims in P4_PAPERS:
        author = name.split("-")[0].capitalize()
        year = name.split("-")[1].split(".")[0]
        seed[f"papers/{name}"] = (f"{title}\n{author} et al., {year}\n\nAbstract\n"
                                  + " ".join(sentence for sentence, _ in claims)
                                  + "\n\nMethod\nWe describe the setting, the data and the limits of the study.\n\n"
                                  "Results\n" + "\n".join(sentence for sentence, _ in claims) + "\n")

    def answer(papers):
        out = [f"# {P4_QUESTION}\n", "## Findings\n"]
        for name, title, claims in papers:
            out.append(f"- *{title}* (`{name}`): " + " ".join(sentence for sentence, _ in claims))
        out += ["", "## Agreements\n", "Most trials found output unchanged and wellbeing better.\n", "## Gaps\n",
                "Few trials have a control group, and long-term effects are unclear.\n"]
        return "\n".join(out)

    check = '''
from checklib import expected, fail, has, read, run, sections

def check(workspace):
    truth = expected()
    text = read(workspace, "summary.md")
    headings = [heading.lower() for heading, _ in sections(text)]
    for needed in ("agree", "gap"):
        if not any(needed in heading for heading in headings):
            fail(f"summary.md has no section on {needed}s")
    uncited = [paper["file"] for paper in truth["papers"]
               if not has(text, paper["file"], paper["file"].rsplit(".", 1)[0], paper["title"])]
    if uncited:
        fail(f"{len(uncited)} paper(s) not cited", ", ".join(uncited))
    claims = [words for paper in truth["papers"] for words in paper["claims"]]
    found = sum(1 for words in claims if has(text, *words))
    message = f"all 8 papers cited, {found} of {len(claims)} key claims"
    if found < truth["pass_at"]:
        fail(message)
    return message

run(check)
'''
    return Task(
        id="p4-literature-summary", title="A literature-style summary of eight papers", kind="report",
        statement=f"""
The folder `papers/` holds eight studies (text files) on one question: {P4_QUESTION} Write `summary.md`, a
literature-style summary: what each paper finds, citing each by title or file name, then a section `## Agreements`
on where they agree and a section `## Gaps` on what they leave open.
""",
        seed=seed, check=check, budgets=(60, 1500, 800_000),
        expected={"papers": [{"file": name, "title": title, "claims": [list(words) for _, words in claims]}
                             for name, title, claims in P4_PAPERS], "pass_at": 14},
        key_facts=[f"{name}: {sentence}" for name, _, claims in P4_PAPERS for sentence, _ in claims]
        + ["Agreement: output mostly unchanged, wellbeing better", "Gaps: selection effects, no control groups, "
                                                                  "long-term effects"],
        solution={"summary.md": answer(P4_PAPERS)}, wrong={"summary.md": answer(P4_PAPERS[:6])})


P5_REPO = {
    "repo/README.md": "# ledgerlite\n\nA tiny command-line ledger.\n",
    "repo/ledgerlite/__init__.py": '"""ledgerlite: a tiny ledger."""\n',
    "repo/ledgerlite/__main__.py": (
        "import argparse\nimport sys\n\nfrom ledgerlite.book import Book\n\n\ndef main(argv=None):\n"
        "    parser = argparse.ArgumentParser(prog=\"ledgerlite\", description=\"A tiny ledger.\")\n"
        "    parser.add_argument(\"--file\", default=\"ledger.json\")\n"
        "    sub = parser.add_subparsers(dest=\"command\", required=True)\n"
        "    add = sub.add_parser(\"add\")\n    add.add_argument(\"amount\", type=float)\n"
        "    add.add_argument(\"note\")\n    sub.add_parser(\"balance\")\n    args = parser.parse_args(argv)\n"
        "    book = Book.load(args.file)\n    if args.command == \"add\":\n        book.add(args.amount, args.note)\n"
        "        book.save(args.file)\n    print(f\"{book.balance():.2f}\")\n    return 0\n\n\n"
        "if __name__ == \"__main__\":\n    sys.exit(main())\n"),
    "repo/ledgerlite/book.py": (
        '"""The ledger book."""\nimport json\nfrom pathlib import Path\n\n\nclass Book:\n'
        "    def __init__(self, entries=None):\n        self.entries = list(entries or [])\n\n"
        "    @classmethod\n    def load(cls, path):\n        path = Path(path)\n"
        "        return cls(json.loads(path.read_text())) if path.exists() else cls()\n\n"
        "    def save(self, path):\n        Path(path).write_text(json.dumps(self.entries))\n\n"
        "    def add(self, amount, note):\n        self.entries.append({\"amount\": amount, \"note\": note})\n\n"
        "    def balance(self):\n        return sum(entry[\"amount\"] for entry in self.entries)\n"),
    "repo/tests/__init__.py": "",
    "repo/tests/test_book.py": (
        "import unittest\n\nfrom ledgerlite.book import Book\n\n\nclass BookTest(unittest.TestCase):\n"
        "    def test_balance_sums_entries(self):\n        book = Book()\n        book.add(5, \"in\")\n"
        "        book.add(-2, \"out\")\n        self.assertEqual(book.balance(), 3)\n"),
    "repo/docs/old-setup.md": ("# Setup (2023)\n\nRun `make test`. (The Makefile was removed in 2024; this page is "
                               "out of date.)\n"),
    "repo/CONTRIBUTING.md": "Branch from main, add a test with every change, and keep functions small.\n",
}


def p5():
    def answer(test_command):
        return {"repo/ONBOARDING.md": (
            "# Onboarding\n\n## Setup\n\nYou need Python 3.10 or later; there are no dependencies.\n\n"
            "## Project layout\n\n- `ledgerlite/book.py`: the ledger\n- `ledgerlite/__main__.py`: the command line\n"
            "- `tests/`: unit tests\n\n## Running the tests\n\n```sh\n" + test_command + "\n```\n\n"
            "## Using the tool\n\n```sh\npython3 -m ledgerlite --help\npython3 -m ledgerlite --file /tmp/l.json "
            "add 5 coffee\n```\n\n## Making a change\n\nBranch from main and add a test with every change "
            "(`CONTRIBUTING.md`).\n")}

    check = '''
import os
import re
import shlex
import subprocess
import sys
from checklib import fail, read, run, sections

def check(workspace):
    text = read(workspace, "repo/ONBOARDING.md")
    headings = " ".join(heading.lower() for heading, _ in sections(text))
    for needed, words in (("setup", ("setup", "set up", "install")), ("layout", ("layout", "structure")),
                          ("tests", ("test",)), ("making a change", ("change", "contribut"))):
        if not any(word in headings for word in words):
            fail(f"ONBOARDING.md has no section on {needed}")
    blocks = re.findall(r"```(?:sh|bash|shell|console)?\\n(.*?)```", text, re.S)
    commands = [line.strip().lstrip("$ ").strip() for block in blocks for line in block.splitlines()]
    commands = [command for command in commands if command.startswith("python3 ")]
    if len(commands) < 2 or not any("unittest" in command for command in commands):
        fail("ONBOARDING.md gives fewer than two python3 commands, or none that runs the tests")
    repo = workspace / "repo"
    environment = dict(os.environ, PYTHONPATH=str(repo), PYTHONDONTWRITEBYTECODE="1")
    for command in commands:
        argv = [sys.executable] + shlex.split(command)[1:]
        result = subprocess.run(argv, cwd=repo, capture_output=True, text=True, timeout=60, env=environment)
        if result.returncode != 0:
            fail(f"the guide's command fails: {command}", result.stdout[-800:] + result.stderr[-800:])
    return f"the sections are there and its {len(commands)} commands run"

run(check)
'''
    return Task(
        id="p5-onboarding-guide", title="An onboarding guide for a repository, with commands that run",
        kind="report",
        statement="""
Write an onboarding guide for the repository in `repo/` as `repo/ONBOARDING.md`, from its code, tests and existing
docs: sections on setup, the project layout, running the tests, using the tool and making a change. Put commands in
fenced `sh` blocks, one per line, run from `repo/`; they must work as written.
""",
        seed=P5_REPO, check=check, budgets=(50, 1200, 600_000),
        key_facts=["No dependencies; Python 3.10+", "Tests: python3 -m unittest discover -s tests -t . (from repo/)",
                   "The tool: python3 -m ledgerlite [--file F] add AMOUNT NOTE | balance",
                   "docs/old-setup.md is out of date (make test; the Makefile is gone)",
                   "CONTRIBUTING.md: branch from main, a test with every change"],
        solution=answer("python3 -m unittest discover -s tests -t ."),
        wrong=answer("python3 -m unittest discover -s test -t ."))


TASKS = (p1, p2, p3, p4, p5)

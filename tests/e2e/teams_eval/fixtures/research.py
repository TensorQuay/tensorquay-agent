"""The research tasks R1-R6: sources in the workspace, an answer file the check reads, and key facts for the judges."""

import csv
import io
import json
import random

from common import Task

R1_TEAMS = ("payments", "search", "auth", "storage")
# (key, heading words the check accepts, count, phrasings of the cause as write-ups put it)
R1_CAUSES = (
    ("disk-full", ("disk", "space", "volume"), 9, (
        "the log volume on {host} reached 100 % and every write after that failed",
        "/var on {host} filled up with logs that were never rotated",
        "the database host {host} reported 'no space left on device' and stopped accepting writes")),
    ("expired-certificate", ("certificat", "tls", "ssl"), 8, (
        "the TLS certificate for {service} expired at midnight and clients refused the handshake",
        "an expired certificate on the {service} load balancer broke every new connection",
        "the certificate renewal job for {service} had been failing silently for weeks until the old one lapsed")),
    ("config-push", ("config",), 7, (
        "a configuration change pushed to every region at once set the {service} pool size to 0",
        "a config push without review switched {service} to a test backend",
        "a configuration rollout changed the {service} timeout from seconds to milliseconds")),
    ("dns", ("dns", "resolver"), 6, (
        "a DNS record for {service} was changed to the address of a decommissioned host",
        "the resolver settings on {host} pointed at a DNS server that had been shut down",
        "a DNS zone update removed the record {service} depended on")),
    ("memory-leak", ("memory", "leak", "oom", "heap"), 5, (
        "a memory leak in the {service} cache client grew until the kernel's OOM killer stopped the process",
        "heap usage in {service} grew steadily after the last release until the process was killed",
        "{service} leaked connections' buffers and ran out of memory every few hours")),
    ("dependency-timeout", ("dependency", "upstream", "timeout"), 3, (
        "an upstream dependency of {service} slowed past our client's timeout and requests piled up",
        "a third-party API {service} calls answered in 40 s instead of 200 ms")),
    ("clock-skew", ("clock", "ntp"), 1, ("NTP stopped on {host} and its clock drifted four minutes",)),
    ("manual-error", ("manual", "operator"), 1, ("an operator ran the cleanup script against production instead "
                                                 "of staging",)),
)
R1_FACTORS = ("disk alerts had been noisy for weeks, so the page was missed at first",
              "the runbook still described the old certificate process",
              "the on-call engineer was new to the service",
              "dashboards for the service had no alert on error rate",
              "a deploy freeze was in place, which slowed the fix")


def r1():
    rng = random.Random(101)
    causes = [key for key, _, count, _ in R1_CAUSES for _ in range(count)]
    rng.shuffle(causes)
    seed, truth = {}, {}
    hosts = [f"{name}-{n:02d}" for name in ("db", "app", "edge", "batch") for n in range(1, 9)]
    services = ("checkout", "search-api", "login", "blob-store", "ledger", "indexer", "session", "media")
    by_key = {key: (words, phrasings) for key, words, _, phrasings in R1_CAUSES}
    for number, cause in enumerate(causes, start=1):
        team = R1_TEAMS[(number - 1) % 4]
        path = f"incidents/{team}/INC-{number:03d}.md"
        host, service = rng.choice(hosts), rng.choice(services)
        sentence = rng.choice(by_key[cause][1]).format(host=host, service=service)
        minutes = rng.randint(12, 240)
        factor = rng.choice(R1_FACTORS)
        seed[path] = (f"# INC-{number:03d}: {service} degraded\n\n"
                      f"**Team:** {team}  \n**Duration:** {minutes} minutes  \n**Severity:** "
                      f"{rng.choice(('SEV-2', 'SEV-3'))}\n\n"
                      f"## Summary\n{service} returned errors for {minutes} minutes; customers saw failed requests.\n\n"
                      f"## Timeline\n- 00:00 alert fired\n- 00:{rng.randint(5, 30):02d} on-call engaged\n"
                      f"- {minutes // 60:02d}:{minutes % 60:02d} resolved\n\n"
                      f"## What happened\nInvestigation showed that {sentence}.\n\n"
                      f"## Contributing factors\n- {factor}\n\n"
                      f"## Follow-ups\n- add an alert for this condition\n- update the runbook\n")
        truth[path] = cause
    top = [key for key, _, _, _ in R1_CAUSES[:5]]
    counts = {key: count for key, _, count, _ in R1_CAUSES}
    names = {"disk-full": "Disk full", "expired-certificate": "Expired TLS certificate",
             "config-push": "Bad configuration push", "dns": "DNS misconfiguration", "memory-leak": "Memory leak"}

    def answer(order, cite):
        out = ["# The five most common root causes\n"]
        for key in order:
            cited = cite(key)
            out.append(f"## {names[key]} ({counts[key]} incidents)\n")
            out += [f"- `{path}`" for path in cited]
            out.append("")
        return "\n".join(out) + "\n"

    def right(key):
        return sorted(path for path, cause in truth.items() if cause == key)[:3]

    def wrong_cite(key):
        return sorted(path for path, cause in truth.items() if cause != key)[:3]

    swapped = [top[0], top[2], top[1], top[3], top[4]]
    check = '''
import re
from checklib import fail, read, run, sections, expected

def starts_a_word(words, heading):
    return any(re.search(r"\\b" + re.escape(word), heading) for word in words)

def names(key, heading, words):
    """Whether a heading names the cause: one of its words starts a word in it, or sits inside one ("misconfigured")
    while no other cause's word starts one: "DNS misconfiguration" is DNS, not a config push (night 1, 11 Oct)."""
    heading = heading.lower()
    if starts_a_word(words[key], heading):
        return True
    return (any(word in heading for word in words[key])
            and not any(starts_a_word(other, heading) for name, other in words.items() if name != key))

def check(workspace):
    truth = expected()
    found = sections(read(workspace, "causes.md"))
    if len(found) < 5:
        fail(f"causes.md has {len(found)} '## ' cause headings, not five")
    for position, (key, (heading, body)) in enumerate(zip(truth["order"], found[:5]), start=1):
        if not names(key, heading, truth["words"]):
            fail(f"cause {position} should be {truth['names'][key]}, found {heading!r}")
        cited = set(re.findall(r"incidents/[a-z]+/INC-\\d{3}\\.md", body))
        real = [path for path in cited if (workspace / path).is_file() and truth["causes"].get(path) == key]
        if len(real) < 3:
            fail(f"{heading!r} cites {len(real)} real incident(s) with that cause, not three")
    return "the five causes in order, each with three real incidents of that cause"

run(check)
'''
    return Task(
        id="r1-incident-causes", title="The five most common root causes in forty incident write-ups",
        kind="research",
        statement="""
The folder `incidents/` holds 40 incident write-ups from four teams. Find the five most common root causes across all
of them. Write `causes.md` with one `## ` heading per cause, most common first, naming the cause and how many
incidents it caused. Under each heading, cite three incidents with that root cause by their paths (for example
`incidents/payments/INC-001.md`). Root causes are described in different words in different write-ups; contributing
factors are not root causes.
""",
        seed=seed, check=check,
        expected={"order": top, "words": {key: list(words) for key, words, _, _ in R1_CAUSES},
                  "names": names, "causes": truth},
        key_facts=[f"{names[key]}: {counts[key]} incidents (rank {rank})" for rank, key in enumerate(top, 1)]
        + ["Dependency timeouts (3), clock skew (1) and a manual error (1) are not in the top five",
           "Contributing factors (noisy disk alerts, an old runbook) are not root causes"],
        solution={"causes.md": answer(top, right)}, wrong={"causes.md": answer(swapped, wrong_cite)})


R2_VENDORS = {
    "Arcwell": {"auth": "OAuth 2.0 client credentials", "rate": "600 requests per minute",
                "pagination": "cursor-based (`next_cursor`)", "pricing": "$0.002 per request"},
    "Brisa": {"auth": "an API key in the `X-Brisa-Key` header", "rate": "50 requests per second",
              "pagination": "offset and limit", "pricing": "$49 per month flat, unlimited requests"},
    "Corvane": {"auth": "HMAC-SHA256 signed requests", "rate": "10,000 requests per day",
                "pagination": "page numbers (`page` and `per_page`)",
                "pricing": "free up to 1,000 requests a month, then $0.01 per request"},
}


def r2():
    seed = {}
    old = {"Arcwell": "Version 1 of the API allowed 300 requests per minute; version 2 raised the limit.",
           "Brisa": "Until January 2025 the plan cost $29 per month.",
           "Corvane": "API keys were retired in 2024 in favour of signed requests."}
    for vendor, facts in R2_VENDORS.items():
        slug = vendor.lower()
        seed[f"vendors/{slug}/overview.md"] = (
            f"# {vendor} API\n\n{vendor} offers a REST API for inventory and orders. This documentation describes "
            "version 2. See the other pages for authentication, limits, pagination and pricing.\n")
        seed[f"vendors/{slug}/authentication.md"] = (
            f"# Authentication\n\nEvery request to the {vendor} API is authenticated with {facts['auth']}. "
            "Requests without valid credentials receive 401.\n\n## Rotating credentials\nRotate credentials at "
            "least once a year from the dashboard.\n")
        seed[f"vendors/{slug}/limits.md"] = (
            f"# Limits\n\nThe {vendor} API allows {facts['rate']} per account. Requests over the limit receive 429 "
            "with a `Retry-After` header.\n\nPayloads are limited to 1 MB.\n")
        seed[f"vendors/{slug}/pagination.md"] = (
            f"# Pagination\n\nList endpoints use {facts['pagination']}. The default page holds 50 items and the "
            "largest 200.\n")
        seed[f"vendors/{slug}/pricing.md"] = f"# Pricing\n\nUsage is billed at {facts['pricing']}.\n"
        seed[f"vendors/{slug}/changelog.md"] = f"# Changelog\n\n- {old[vendor]}\n- Typos fixed in the docs.\n"

    def table(values):
        rows = ["| Vendor | Auth | Rate limits | Pagination | Pricing |", "|---|---|---|---|---|"]
        rows += [f"| {vendor} | {v['auth']} | {v['rate']} | {v['pagination']} | {v['pricing']} |"
                 for vendor, v in values.items()]
        return "# Vendor comparison\n\n" + "\n".join(rows) + "\n"

    wrong = json.loads(json.dumps(R2_VENDORS))
    wrong["Arcwell"]["rate"] = "300 requests per minute"
    wrong["Brisa"]["pricing"] = "$29 per month"
    check = '''
from checklib import fail, has, numbers, read, run, table

CELLS = {
    "arcwell": {"auth": lambda c: has(c, "oauth"), "rate": lambda c: 600 in numbers(c) and has(c, "minute", "/min"),
                "pagination": lambda c: has(c, "cursor"), "pricing": lambda c: 0.002 in numbers(c)},
    "brisa": {"auth": lambda c: has(c, "api key", "x-brisa-key"),
              "rate": lambda c: 50 in numbers(c) and has(c, "second", "/s"),
              "pagination": lambda c: has(c, "offset"), "pricing": lambda c: 49 in numbers(c) and has(c, "month")},
    "corvane": {"auth": lambda c: has(c, "hmac", "signed"), "rate": lambda c: 10000 in numbers(c) and has(c, "day"),
                "pagination": lambda c: has(c, "page") and not has(c, "cursor", "offset"),
                "pricing": lambda c: 1000 in numbers(c) and 0.01 in numbers(c)},
}
COLUMNS = {"auth": ("auth",), "rate": ("rate", "limit"), "pagination": ("pagin",), "pricing": ("pric", "cost")}

def check(workspace):
    rows = table(read(workspace, "comparison.md"))
    if len(rows) < 2:
        fail("comparison.md has no markdown table")
    header = [cell.lower() for cell in rows[0]]
    where = {}
    for column, words in COLUMNS.items():
        matches = [index for index, cell in enumerate(header) if any(word in cell for word in words)]
        if not matches:
            fail(f"the table has no {column} column")
        where[column] = matches[0]
    wrong = []
    for vendor, cells in CELLS.items():
        row = next((row for row in rows[1:] if row and vendor in row[0].lower()), None)
        if row is None:
            fail(f"the table has no row for {vendor}")
        for column, good in cells.items():
            cell = row[where[column]] if where[column] < len(row) else ""
            if not good(cell):
                wrong.append(f"{vendor} {column}: {cell!r}")
    if wrong:
        fail(f"{12 - len(wrong)} of 12 cells match the documentation", "\\n".join(wrong))
    return "all 12 cells match the vendors' current documentation"

run(check)
'''
    facts = [f"{vendor} {name}: {value}" for vendor, v in R2_VENDORS.items() for name, value in v.items()]
    return Task(
        id="r2-vendor-comparison", title="Compare three vendors' APIs from their documentation", kind="research",
        statement="""
The folder `vendors/` holds the API documentation of three vendors. Write `comparison.md` with a markdown table: one
row per vendor and the columns Auth, Rate limits, Pagination and Pricing, each cell stating what the vendor's
current documentation says. Use only the documentation in the workspace.
""",
        seed=seed, check=check, key_facts=facts + ["Changelog values are old: Arcwell's 300/min, Brisa's $29, "
                                                   "Corvane's API keys"],
        solution={"comparison.md": table(R2_VENDORS)}, wrong={"comparison.md": table(wrong)})


# (version, the token that identifies it, the entry as written, the migration)
R3_BREAKING = (
    ("4.1.0", "legacy-auth", "BREAKING: the `--legacy-auth` flag is removed.", "use `--auth token`"),
    ("4.1.0", "fetch_all", "Removed `Client.fetch_all()`.", "iterate with `Client.iter_pages()`"),
    ("4.2.0", "server.port", "The config key `server.port` is renamed to `listen.port`; the old name is no longer "
     "read.", "rename the key to `listen.port`"),
    ("4.3.0", "3.8", "Python 3.8 is no longer supported.", "run on Python 3.10 or later"),
    ("4.3.0", "export", "The default format of `export` changed from CSV to JSON (breaking).",
     "pass `--format csv` to keep CSV"),
    ("4.4.0", "tq_home", "`TQ_HOME` is no longer read.", "set `TQ_CONFIG_DIR` instead"),
    ("4.5.0", "/v1/items", "BREAKING: the `/v1/items` endpoint is gone.", "call `/v2/items`"),
    ("4.5.0", "timeout", "The `timeout` argument is now in seconds, not milliseconds (breaking).",
     "divide existing values by 1000"),
    ("4.6.0", "--force", "`sync --force` was removed.", "use `sync --overwrite`"),
    ("4.7.0", "register", "Plugins: `register(name, fn)` no longer works; `register()` takes a `Plugin` object.",
     "wrap the function in a `Plugin`"),
    ("4.8.0", "record.id", "`Record.id` is now a string instead of an integer (breaking).",
     "compare ids as strings"),
    ("4.9.0", "quiet", "`--quiet` is renamed to `--silent`; `--quiet` is no longer accepted.", "use `--silent`"),
    ("5.0.0", "init", "BREAKING: `tq init` no longer creates a sample project.", "run `tq init --sample`"),
    ("5.0.0", "master", "New projects use `main` as the default branch instead of `master` (breaking).",
     "pass `--branch master` to keep the old name"),
    ("5.1.0", "cache.dir", "Removed the `cache.dir` config key.", "use `cache.path`"),
    ("5.2.0", "retries", "`Client(retries=...)` is removed.", "pass a `RetryPolicy`"),
    ("5.3.0", "--html", "BREAKING: `report --html` is removed.", "use `report --format html`"),
)
R3_DEPRECATED = (
    ("4.2.0", "verbose", "`--verbose` is deprecated in favour of `--log-level debug`; it still works."),
    ("4.4.0", "config.yaml", "`config.yaml` is deprecated in favour of `config.toml`; both are read until 6.0."),
    ("4.6.0", "get_raw", "`Client.get_raw()` is deprecated and will be removed in 6.0."),
    ("4.8.0", "tq_debug", "`TQ_DEBUG` is deprecated; it still enables debug logging."),
    ("5.1.0", "doctor", "`tq doctor --fix` is deprecated; it keeps working until 6.0."),
    ("5.2.0", "q=", "The `q=` parameter of `/v2/search` is deprecated in favour of `query=`; both work."),
)
R3_MONTHS = {"4.0.0": "2025-01", "4.1.0": "2025-02", "4.2.0": "2025-03", "4.3.0": "2025-04", "4.4.0": "2025-05",
             "4.5.0": "2025-06", "4.6.0": "2025-07", "4.7.0": "2025-08", "4.8.0": "2025-09", "4.9.0": "2025-10",
             "5.0.0": "2025-11", "5.1.0": "2025-12", "5.2.0": "2025-12", "5.3.0": "2025-12"}
R3_OTHER = ("Faster startup on large projects.", "Fixed a crash when the config file is empty.",
            "Added `tq status --json`.", "Improved error messages for network failures.",
            "Documentation: new tutorial on plugins.", "Fixed a race in the sync lock.",
            "Added support for Python 3.13.", "Reduced memory use of `export` by 30 %.")


def r3():
    rng = random.Random(103)
    entries = {}
    for version, _, text, migration in R3_BREAKING:
        entries.setdefault(version, []).append(f"{text} Migration: {migration}.")
    for version, _, text in R3_DEPRECATED:
        entries.setdefault(version, []).append(text)
    for version in R3_MONTHS:
        entries.setdefault(version, [])
        entries[version] += rng.sample(R3_OTHER, 2)
        rng.shuffle(entries[version])
    seed = {}
    for version, month in R3_MONTHS.items():
        body = f"## {version}\n\n" + "".join(f"- {entry}\n" for entry in entries[version]) + "\n"
        seed[f"changelog/{month}.md"] = seed.get(f"changelog/{month}.md", f"# Releases, {month}\n\n") + body

    def answer(items):
        rows = ["| Version | Change | Migration |", "|---|---|---|"]
        rows += [f"| {version} | {text} | {migration} |" for version, _, text, migration in items]
        return "# Breaking changes in 2025\n\n" + "\n".join(rows) + "\n"

    wrong = list(R3_BREAKING[:12]) + [(v, t, x, "none") for v, t, x in R3_DEPRECATED[:4]]
    check = '''
from checklib import expected, fail, read, run, table

def check(workspace):
    truth = expected()
    rows = table(read(workspace, "breaking.md"))
    if len(rows) < 2:
        fail("breaking.md has no markdown table")
    texts = [(row[0] if row else "", " ".join(row).lower()) for row in rows[1:]]
    found = [token for version, token in truth["breaking"]
             if any(version in first and token in text for first, text in texts)]
    false = [token for version, token in truth["deprecated"]
             if any(version in first and token in text for first, text in texts)]
    message = f"{len(found)} of {len(truth['breaking'])} breaking changes found, {len(false)} deprecation(s) listed"
    if len(found) < truth["pass_at"] or len(false) > truth["false_at_most"]:
        missing = [token for _, token in truth["breaking"] if token not in found]
        fail(message, "missing: " + ", ".join(missing))
    return message

run(check)
'''
    return Task(
        id="r3-breaking-changes", title="Every breaking change in a year of release notes", kind="research",
        statement="""
The folder `changelog/` holds a year of release notes, one file per month. Write `breaking.md` with a markdown table
listing every breaking change: the columns Version, Change and Migration (what a user must do). A deprecation that
still works is not a breaking change.
""",
        seed=seed, check=check,
        expected={"breaking": [[v, t] for v, t, _, _ in R3_BREAKING],
                  "deprecated": [[v, t] for v, t, _ in R3_DEPRECATED],
                  "pass_at": 15, "false_at_most": 2},
        key_facts=[f"{version}: {text} ({migration})" for version, _, text, migration in R3_BREAKING]
        + ["Six deprecations still work and are not breaking (e.g. --verbose, config.yaml, get_raw)"],
        solution={"breaking.md": answer(R3_BREAKING)}, wrong={"breaking.md": answer(wrong)})


R4_THEMES = (
    ("slow sync", ("sync",), 18, (
        "Sync takes forever, sometimes ten minutes before my files show up on the laptop.",
        "Files I save on my phone appear on the desktop only after a long delay.",
        "Syncing a folder of photos stalls halfway every time.",
        "The sync is so slow that we email files to each other instead.",
        "Changes from my colleagues show up hours late; sync is unreliable.",
        "Large folders never finish syncing overnight.")),
    ("pricing", ("pric", "cost", "expensive"), 14, (
        "The price went up again and we are paying for seats we do not use.",
        "Too expensive for a team of three compared with the alternatives.",
        "The new pricing tiers make no sense for a small school.",
        "We would upgrade if the yearly plan cost less.",
        "Paying per seat is hard to justify for occasional users.")),
    ("mobile app crashes", ("mobile", "crash", "app"), 12, (
        "The Android app crashes whenever I open a PDF.",
        "The iPhone app closes itself when I rotate the screen.",
        "Mobile app crashed three times during a client meeting.",
        "The app on my tablet freezes and then crashes on upload.")),
    ("missing export", ("export",), 9, (
        "There is no way to export a whole project as a zip.",
        "Please add an export to CSV for the activity log.",
        "I cannot export my notes to PDF, which our auditors need.")),
    ("helpful support", ("support",), 7, (
        "Support answered within an hour and fixed my account.",
        "The support team was patient and walked me through the setup.",
        "Great support, they called me back the same day.")),
)


def r4():
    rng = random.Random(104)
    rows = []
    for theme, _, count, texts in R4_THEMES:
        for index in range(count):
            rows.append((theme, texts[index % len(texts)]))
    rng.shuffle(rows)
    buffer = io.StringIO()
    writer = csv.writer(buffer, lineterminator="\n")
    writer.writerow(("id", "segment", "date", "comment"))
    by_theme = {}
    for number, (theme, text) in enumerate(rows, start=1):
        segment = rng.choice(("enterprise", "small business", "education", "individual"))
        writer.writerow((number, segment, f"2026-0{rng.randint(1, 9)}-{rng.randint(10, 28)}", text))
        by_theme.setdefault(theme, []).append(text)

    def answer(counts):
        out = ["| Theme | Count | Quote | Quote |", "|---|---|---|---|"]
        for theme, _, count, texts in R4_THEMES:
            out.append(f"| {theme} | {counts.get(theme, count)} | \"{texts[0]}\" | \"{texts[1]}\" |")
        return "# Feedback themes\n\n" + "\n".join(out) + "\n"

    check = '''
from checklib import expected, fail, norm, numbers, read, run, table

def check(workspace):
    truth = expected()
    rows = table(read(workspace, "themes.md"))
    if len(rows) < 2:
        fail("themes.md has no markdown table")
    problems = []
    for theme in truth["themes"]:
        row = next((row for row in rows[1:] if row and any(word in row[0].lower() for word in theme["words"])), None)
        if row is None or len(row) < 4:
            problems.append(f"no row for {theme['name']}")
            continue
        counted = numbers(row[1])
        if not counted or abs(counted[0] - theme["count"]) > 2:
            problems.append(f"{theme['name']}: count {row[1]!r}, the comments give {theme['count']}")
        quotes = [norm(cell).strip('"\\u201c\\u201d\\' ') for cell in row[2:4]]
        comments = [norm(text) for text in theme["comments"]]
        if not all(quote and any(quote in comment for comment in comments) for quote in quotes):
            problems.append(f"{theme['name']}: a quote is not a comment of that theme")
    if problems:
        fail(f"{len(problems)} problem(s) in the themes", "\\n".join(problems))
    return "five themes with counts within two of the comments' and real quotes"

run(check)
'''
    return Task(
        id="r4-feedback-themes", title="The main themes in sixty customer comments", kind="research",
        statement="""
`feedback.csv` holds 60 customer comments from four segments. Find the main themes. Write `themes.md` with a
markdown table: one row per theme, with the columns Theme, Count (how many comments raise it) and two columns with a
quote each, copied word for word from comments on that theme.
""",
        seed={"feedback.csv": buffer.getvalue()}, check=check,
        expected={"themes": [{"name": theme, "words": list(words), "count": count, "comments": by_theme[theme]}
                             for theme, words, count, _ in R4_THEMES]},
        key_facts=[f"{theme}: {count} comments" for theme, _, count, _ in R4_THEMES]
        + ["Helpful support is the one positive theme"],
        solution={"themes.md": answer({})}, wrong={"themes.md": answer({"pricing": 8, "missing export": 3})})


R5_DOCUMENTS = {
    "travel.md": ("# Travel policy\n\n"
                  "1. Book economy class for every flight under 6 hours.\n"
                  "2. Hotels: up to 150 EUR per night in most cities.\n"
                  "3. The daily meal allowance is 60 EUR.\n"
                  "4. Take your company laptop on every business trip, including trips outside the EU.\n"
                  "5. Connect only through the company VPN on hotel networks.\n"),
    "expenses.md": ("# Expenses policy\n\n"
                    "- Business class may be booked for flights over 4 hours.\n"
                    "- Hotels are reimbursed up to 150 EUR per night.\n"
                    "- Meals are reimbursed up to 45 EUR per day.\n"
                    "- Purchases above 1,000 EUR need a director's approval.\n"
                    "- Home internet is not reimbursable.\n"),
    "remote-work.md": ("# Remote work policy\n\n"
                       "Employees may work from home up to four days a week.\n\n"
                       "Equipment purchases above 500 EUR need a director's approval.\n\n"
                       "The company reimburses home internet up to 30 EUR a month.\n\n"
                       "Do not rotate passwords on a schedule; change one only when you suspect it was exposed.\n\n"
                       "Use the company VPN for all work traffic.\n"),
    "security.md": ("# Security policy\n\n"
                    "- Passwords must be changed every 90 days.\n"
                    "- Company laptops must not be taken outside the EU.\n"
                    "- Use the company VPN on any network you do not control.\n"
                    "- Report a lost device within 24 hours.\n"),
}
# (topic words, the two documents, the statements)
R5_CONTRADICTIONS = (
    (("flight", "business class", "economy"), ("travel", "expenses"), "economy under 6 h vs business over 4 h"),
    (("meal",), ("travel", "expenses"), "60 EUR vs 45 EUR a day for meals"),
    (("approval", "purchase"), ("expenses", "remote-work"), "director approval above 1,000 EUR vs above 500 EUR"),
    (("laptop",), ("travel", "security"), "laptop on trips outside the EU vs never outside the EU"),
    (("password",), ("remote-work", "security"), "no scheduled rotation vs every 90 days"),
    (("internet",), ("expenses", "remote-work"), "home internet not reimbursable vs up to 30 EUR a month"),
)


def r5():
    def answer(items):
        out = ["# Contradictions\n"]
        for words, (a, b), summary in items:
            out.append(f"## {words[0].capitalize()}\n\n`{a}.md` and `{b}.md` disagree: {summary}.\n")
        return "\n".join(out)

    check = '''
from checklib import expected, fail, read, run, sections

def check(workspace):
    truth = expected()
    answer = read(workspace, "contradictions.md")
    found_sections = [(heading + " " + body).lower() for heading, body in sections(answer)]
    found = [item["topic"] for item in truth["contradictions"]
             if any(any(word in text for word in item["words"]) and all(doc in text for doc in item["documents"])
                    for text in found_sections)]
    message = f"{len(found)} of {len(truth['contradictions'])} contradictions found"
    if len(found) < truth["pass_at"]:
        missing = [item["topic"] for item in truth["contradictions"] if item["topic"] not in found]
        fail(message, "missing: " + ", ".join(missing))
    return message

run(check)
'''
    return Task(
        id="r5-policy-contradictions", title="Where four policy documents contradict each other", kind="research",
        statement="""
The folder `policy/` holds four company policies. Find every place where two of them contradict each other. Write
`contradictions.md` with one `## ` heading per contradiction; under it, name both documents by file name and quote
or state what each says. Rules that agree are not contradictions.
""",
        seed={f"policy/{name}": text for name, text in R5_DOCUMENTS.items()}, check=check,
        expected={"contradictions": [{"topic": words[0], "words": list(words), "documents": list(docs)}
                                     for words, docs, _ in R5_CONTRADICTIONS], "pass_at": 5},
        key_facts=[f"{a}.md vs {b}.md: {summary}" for _, (a, b), summary in R5_CONTRADICTIONS]
        + ["Hotels (150 EUR) and the VPN rule agree across documents"],
        solution={"contradictions.md": answer(R5_CONTRADICTIONS)},
        wrong={"contradictions.md": answer(R5_CONTRADICTIONS[:3])})


R6_COMPONENTS = {
    "api": "HTTP handlers: validates orders and publishes them to the queue",
    "auth": "checks API tokens for the api handlers",
    "queue": "an in-process job queue between the api and the worker",
    "worker": "takes jobs off the queue and runs them",
    "billing": "charges an order's total; called by the worker",
    "storage": "saves orders and their states; used by the worker and the api",
    "notifier": "sends the customer an email when an order is charged",
    "config": "loads settings from environment variables for every component",
}


def r6():
    files = {
        "shopflow/__init__.py": '"""shopflow: a small order service."""\n',
        "shopflow/config/__init__.py": "",
        "shopflow/config/settings.py": (
            '"""Settings from environment variables, read once."""\nimport os\n\n\n'
            "def load():\n    return {\"queue_size\": int(os.environ.get(\"SHOPFLOW_QUEUE\", \"100\")),\n"
            "            \"smtp_host\": os.environ.get(\"SHOPFLOW_SMTP\", \"localhost\"),\n"
            "            \"db_path\": os.environ.get(\"SHOPFLOW_DB\", \"orders.json\")}\n"),
        "shopflow/auth/__init__.py": "",
        "shopflow/auth/tokens.py": (
            '"""API tokens."""\n\nKNOWN = {"demo-token": "demo"}\n\n\n'
            "def account_for(token):\n    \"\"\"The account a token belongs to, or None.\"\"\"\n"
            "    return KNOWN.get(token)\n"),
        "shopflow/api/__init__.py": "",
        "shopflow/api/handlers.py": (
            '"""HTTP handlers."""\nfrom shopflow.auth.tokens import account_for\nfrom shopflow.queue.jobs import '
            "publish\nfrom shopflow.storage.orders import get_order, save_order\n\n\n"
            "def create_order(token, body):\n    account = account_for(token)\n    if account is None:\n"
            "        return 401, {\"error\": \"unknown token\"}\n    order = {\"account\": account, \"items\": "
            "body[\"items\"], \"state\": \"new\"}\n    order_id = save_order(order)\n"
            "    publish(\"charge\", order_id)\n    return 202, {\"id\": order_id}\n\n\n"
            "def show_order(token, order_id):\n    if account_for(token) is None:\n        return 401, {}\n"
            "    return 200, get_order(order_id)\n"),
        "shopflow/api/routes.py": (
            '"""The routing table."""\nfrom shopflow.api import handlers\n\nROUTES = {("POST", "/orders"): '
            'handlers.create_order, ("GET", "/orders/<id>"): handlers.show_order}\n'),
        "shopflow/queue/__init__.py": "",
        "shopflow/queue/jobs.py": (
            '"""An in-process job queue."""\nimport collections\n\nJOBS = collections.deque()\n\n\n'
            "def publish(kind, order_id):\n    JOBS.append((kind, order_id))\n\n\n"
            "def take():\n    return JOBS.popleft() if JOBS else None\n"),
        "shopflow/worker/__init__.py": "",
        "shopflow/worker/loop.py": (
            '"""Runs queued jobs."""\nfrom shopflow.billing.charge import charge\nfrom shopflow.notifier.email '
            "import send_receipt\nfrom shopflow.queue.jobs import take\nfrom shopflow.storage.orders import "
            "get_order, set_state\n\n\ndef run_once():\n    job = take()\n    if job is None:\n        return False\n"
            "    kind, order_id = job\n    if kind == \"charge\":\n        order = get_order(order_id)\n"
            "        charge(order)\n        set_state(order_id, \"charged\")\n        send_receipt(order)\n"
            "    return True\n"),
        "shopflow/billing/__init__.py": "",
        "shopflow/billing/charge.py": (
            '"""Charges orders."""\nPRICES = {"book": 12.5, "pen": 1.2}\n\n\n'
            "def total(order):\n    return round(sum(PRICES[item] for item in order[\"items\"]), 2)\n\n\n"
            "def charge(order):\n    return {\"amount\": total(order), \"status\": \"ok\"}\n"),
        "shopflow/storage/__init__.py": "",
        "shopflow/storage/orders.py": (
            '"""Order storage."""\nORDERS = {}\n\n\ndef save_order(order):\n    order_id = str(len(ORDERS) + 1)\n'
            "    ORDERS[order_id] = dict(order)\n    return order_id\n\n\ndef get_order(order_id):\n"
            "    return ORDERS[order_id]\n\n\ndef set_state(order_id, state):\n"
            "    ORDERS[order_id][\"state\"] = state\n"),
        "shopflow/notifier/__init__.py": "",
        "shopflow/notifier/email.py": (
            '"""Customer email."""\nfrom shopflow.config.settings import load\n\nSENT = []\n\n\n'
            "def send_receipt(order):\n    SENT.append((load()[\"smtp_host\"], order[\"account\"]))\n"),
        "tests/__init__.py": "",
        "tests/test_flow.py": (
            "import unittest\n\nfrom shopflow.api.handlers import create_order\nfrom shopflow.worker.loop import "
            "run_once\nfrom shopflow.storage.orders import get_order\n\n\nclass Flow(unittest.TestCase):\n"
            "    def test_an_order_is_charged(self):\n        status, body = create_order(\"demo-token\", "
            "{\"items\": [\"book\"]})\n        self.assertEqual(status, 202)\n        self.assertTrue(run_once())\n"
            "        self.assertEqual(get_order(body[\"id\"])[\"state\"], \"charged\")\n"),
        "README.md": "# shopflow\n\nRun the tests with `python3 -m unittest discover -s tests -t .`.\n",
    }

    def answer(components):
        out = ["# Architecture\n", "## Components\n"]
        out += [f"- `shopflow/{name}`: {text}" for name, text in components.items()]
        out += ["", "## Data flow\n", "A request reaches `api` (checked by `auth`), the order is saved by `storage` "
                "and a job is published to `queue`; `worker` takes it, `billing` charges it, `storage` records the "
                "state and `notifier` emails the customer.\n", "## Adding a feature\n",
                "Add a handler in `shopflow/api/handlers.py` and its route in `shopflow/api/routes.py`; background "
                "work goes through `shopflow/queue/jobs.py` and a branch in `shopflow/worker/loop.py`.\n"]
        return "\n".join(out)

    check = '''
from checklib import expected, fail, read, run, sections

def check(workspace):
    truth = expected()
    found = {heading.lower(): body.lower() for heading, body in sections(read(workspace, "ARCHITECTURE.md"))}
    def section(*words):
        for heading, body in found.items():
            if any(word in heading for word in words):
                return body
        fail(f"ARCHITECTURE.md has no section on {words[0]}")
    components = section("component")
    named = [name for name in truth["components"] if name in components]
    if len(named) < truth["pass_at"]:
        fail(f"{len(named)} of {len(truth['components'])} components named")
    flow = section("data flow", "flow")
    if not all(word in flow for word in ("api", "queue", "worker")):
        fail("the data flow section does not follow a request through api, queue and worker")
    feature = section("feature", "extend")
    if "shopflow/" not in feature:
        fail("the feature section names no file to change")
    return f"{len(named)} of {len(truth['components'])} components, the flow and where to add a feature"

run(check)
'''
    return Task(
        id="r6-architecture-overview", title="An architecture overview of a repository", kind="research",
        statement="""
Write `ARCHITECTURE.md` for the repository in this workspace with three `## ` sections: Components (each component
and what it does), Data flow (how a request moves through them) and Adding a feature (where to add a new endpoint
and its background work, naming the files). Base it on the code.
""",
        seed=files, check=check, budgets=(60, 1500, 800_000),
        expected={"components": list(R6_COMPONENTS), "pass_at": 7},
        key_facts=[f"{name}: {text}" for name, text in R6_COMPONENTS.items()]
        + ["Flow: api (auth) -> storage + queue -> worker -> billing -> storage state -> notifier",
           "A new endpoint goes in api/handlers.py and api/routes.py; background work via queue and worker/loop.py"],
        solution={"ARCHITECTURE.md": answer(R6_COMPONENTS)},
        wrong={"ARCHITECTURE.md": answer({name: text for name, text in list(R6_COMPONENTS.items())[:4]})})


TASKS = (r1, r2, r3, r4, r5, r6)

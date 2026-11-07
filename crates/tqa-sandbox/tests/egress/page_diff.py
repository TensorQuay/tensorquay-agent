"""WEB-E2E-010's live page scan: what may differ between two loads of the same page. With the CSP nonce every page renders
per request, and Next adds a fresh random id to each render's payload (the head element's key). So besides the nonce
(already replaced by a marker), a page pair may differ only by a few server-made random tokens, and none of them may be
request data. Agreed with the scan's owner; each rule has a unit test (test_page_diff.py)."""
import base64
import re
import urllib.parse
from collections import Counter

TOKEN = re.compile(r"[A-Za-z0-9_-]+")
# A server-made random id: long, from the id alphabet. A shorter differing token (a count, a time, a word) is a real
# difference in content.
RANDOM_LENGTH = 16
# More than a few render ids: the page changed. Tokens count in both directions (a changed id is one token only in
# the first load and one only in the second), so 8 tokens is about 4 changed ids per pair.
MAX_DIFFERING = 8
# Request values shorter than this say nothing when found inside a token ("1" is in most ids).
MIN_VALUE_LENGTH = 8


def differing(first, second):
    """The tokens, with their counts, in one body and not the other (a moved or repeated token counts too)."""
    one, two = Counter(TOKEN.findall(first)), Counter(TOKEN.findall(second))
    return (one - two) + (two - one)


def decoded(value):
    """A cookie value as it is, percent-decoded, and base64url-decoded where that gives text."""
    forms = {value, urllib.parse.unquote(value)}
    try:
        text = base64.urlsafe_b64decode(value + "=" * (-len(value) % 4)).decode("utf-8")
        forms.add(text)
    except (ValueError, UnicodeDecodeError):
        pass
    return forms


def request_values(headers, cookies, others):
    """Everything request-side a token must not match: header values (the Cookie header whole), each cookie value in
    its forms, and `others` (the credential forms, the task, conversation and run ids)."""
    values = set(headers)
    for cookie in cookies:
        values |= decoded(cookie)
    values |= set(others)
    return {value.lower() for value in values if value}


def problems(first, second, values):
    """Why a page pair differs more than it may, as short reasons: none when it passes. Never prints a request value:
    a token that matches one is counted, not shown."""
    changed = differing(first, second)
    reasons = []
    total = sum(changed.values())
    if total > MAX_DIFFERING:
        reasons.append("{} tokens differ (at most {})".format(total, MAX_DIFFERING))
    short = [token for token in changed if len(token) < RANDOM_LENGTH]
    if short:
        reasons.append("short tokens differ: " + ", ".join(token[:12] for token in short[:3]))
    matching = 0
    for token in changed:
        low = token.lower()
        if any(low in value or (len(value) >= MIN_VALUE_LENGTH and value in low) for value in values):
            matching += 1
    if matching:
        reasons.append("{} differing tokens match request data".format(matching))
    return reasons

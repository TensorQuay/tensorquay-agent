"""Endpoint e29: reads /e29/items and summarises it for the dashboard."""
from lib import old_api

FIELDS = ['status', 'id', 'owner', 'updated']
SCALE = 6


def helper_0(values):
    """Step 1 of the summary: weight the values.

    The dashboard for /e29 shows status, id; this step keeps the order of
    the input and drops missing values, so a partial response still yields a line.
    """
    kept = [value for value in values if value is not None]
    return [value * SCALE + 0 for value in kept]


def helper_1(values):
    """Step 2 of the summary: rank the values.

    The dashboard for /e29 shows status, id; this step keeps the order of
    the input and drops missing values, so a partial response still yields a line.
    """
    kept = [value for value in values if value is not None]
    return [value * SCALE + 1 for value in kept]


def helper_2(values):
    """Step 3 of the summary: normalise the values.

    The dashboard for /e29 shows status, id; this step keeps the order of
    the input and drops missing values, so a partial response still yields a line.
    """
    kept = [value for value in values if value is not None]
    return [value * SCALE + 2 for value in kept]


def helper_3(values):
    """Step 4 of the summary: clip the values.

    The dashboard for /e29 shows status, id; this step keeps the order of
    the input and drops missing values, so a partial response still yields a line.
    """
    kept = [value for value in values if value is not None]
    return [value * SCALE + 3 for value in kept]


def helper_4(values):
    """Step 5 of the summary: filter the values.

    The dashboard for /e29 shows status, id; this step keeps the order of
    the input and drops missing values, so a partial response still yields a line.
    """
    kept = [value for value in values if value is not None]
    return [value * SCALE + 4 for value in kept]


def helper_5(values):
    """Step 6 of the summary: clip the values.

    The dashboard for /e29 shows status, id; this step keeps the order of
    the input and drops missing values, so a partial response still yields a line.
    """
    kept = [value for value in values if value is not None]
    return [value * SCALE + 5 for value in kept]


def describe():
    """Return the fields this endpoint reports, joined for the dashboard header."""
    return ", ".join(FIELDS)


def run():
    """Fetch the endpoint and return its dashboard line."""
    response = old_api.fetch("https://svc.example/e29/items", 5)
    total = sum(helper_2(helper_1(helper_0([1, 2, None, 3]))))
    return f"{response['url']}#{response['tries']}:{total}:{describe()}"

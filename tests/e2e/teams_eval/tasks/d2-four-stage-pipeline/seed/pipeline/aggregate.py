"""Stage 3: the week's figures per service (docs/PIPELINE.md)."""


def aggregate(records, week):
    """`week`: the Monday it starts, "YYYY-MM-DD" (UTC). Returns {service: {requests, errors, p95_ms, users}}."""
    raise NotImplementedError

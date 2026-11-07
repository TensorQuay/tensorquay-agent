"""Stage 3: the week's figures per service (docs/PIPELINE.md)."""
import math
from datetime import datetime, timedelta, timezone


def aggregate(records, week):
    """`week`: the Monday it starts, "YYYY-MM-DD" (UTC). Returns {service: {requests, errors, p95_ms, users}}."""
    start = datetime.fromisoformat(week).replace(tzinfo=timezone.utc)
    end = start + timedelta(days=7)
    by_service = {}
    for record in records:
        if start <= record["time"] < end:
            by_service.setdefault(record["service"], []).append(record)
    figures = {}
    for service, items in sorted(by_service.items()):
        durations = sorted(item["ms"] for item in items)
        figures[service] = {"requests": len(items), "errors": sum(item["failed"] for item in items),
                            "p95_ms": durations[math.ceil(0.95 * len(durations)) - 1],
                            "users": len({item["user"] for item in items})}
    return figures

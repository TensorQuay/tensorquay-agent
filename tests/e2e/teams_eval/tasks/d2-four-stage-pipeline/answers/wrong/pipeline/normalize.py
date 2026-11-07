"""Stage 2: raw records from any source into one record shape (docs/PIPELINE.md):
time (an aware UTC datetime),
service, failed (bool), ms (int) and user."""
from datetime import datetime, timezone


def normalize(raw, source):
    """`raw`: stage 1's records from one source ("access", "app" or "worker")."""
    records = []
    for item in raw:
        if source == "access":
            time = datetime.fromisoformat(item["time"].replace("Z", "+00:00"))
            failed, ms = int(item["status"]) >= 500, int(item["ms"])
        elif source == "app":
            time = datetime.fromisoformat(item["time"].replace("Z", "+00:00"))
            failed, ms = int(item["status"]) >= 500, int(item["duration_ms"])
        else:
            time = datetime.strptime(f"2026 {item['month']} {item['day']} {item['clock']}", "%Y %b %d %H:%M:%S")
            time = time.replace(tzinfo=timezone.utc)
            failed, ms = item["status"] == "failed", round(float(item["seconds"]))
        records.append({"time": time, "service": item["service"], "failed": failed, "ms": ms, "user": item["user"]})
    return records

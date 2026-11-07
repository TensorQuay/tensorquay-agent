from client import v1


def push(records):
    status = v1.send("sync", {"records": records})
    return f"synced {len(records)}" if status == 200 else "sync failed"

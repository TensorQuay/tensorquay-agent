from client.v2 import Client


def push(records):
    status = Client().post("/sync", json={"records": records}).status
    return f"synced {len(records)}" if status == 200 else "sync failed"

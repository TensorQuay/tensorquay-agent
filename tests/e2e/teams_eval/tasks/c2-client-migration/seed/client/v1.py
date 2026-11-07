"""Version 1 of the store client. Deprecated: use client.v2 (docs/MIGRATION.md)."""
from client.server import SERVER


def fetch(path, query=None):
    """GET `path` (no leading slash): the decoded body, or None when it does not exist."""
    status, body = SERVER.handle("GET", "/" + path, query or {})
    return None if status == 404 else body


def send(path, data):
    """POST `data` to `path`: the status code."""
    status, _ = SERVER.handle("POST", "/" + path, {}, data)
    return status

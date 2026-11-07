"""Version 2 of the store client."""
from client.server import SERVER


class NotFound(Exception):
    """The path names nothing."""


class Response:
    def __init__(self, status, body):
        self.status = status
        self._body = body

    def json(self):
        return self._body


class Client:
    def __init__(self, server=None):
        self.server = server or SERVER

    def get(self, path, params=None):
        if not path.startswith("/"):
            raise ValueError("paths start with /")
        status, body = self.server.handle("GET", path, params or {})
        if status == 404:
            raise NotFound(path)
        return Response(status, body)

    def post(self, path, json=None):
        if not path.startswith("/"):
            raise ValueError("paths start with /")
        status, body = self.server.handle("POST", path, {}, json)
        return Response(status, body)

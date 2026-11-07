"""An in-memory store the clients talk to, standing in for the network."""


class Server:
    ORDERS = {"1": {"items": ["pen", "book"], "total": 13.7}, "2": {"items": ["lamp"], "total": 24.0}}
    USERS = {"7": {"name": "Ada Lovelace"}, "8": {"name": "Alan Turing"}}
    DAILY = {"2026-10-01": 99.5, "2026-10-02": 12.25}

    def handle(self, method, path, query, body=None):
        """(status, decoded body)."""
        parts = path.strip("/").split("/")
        if method == "GET" and parts[0] == "orders" and len(parts) == 2:
            order = self.ORDERS.get(parts[1])
            return (200, order) if order else (404, None)
        if method == "POST" and parts == ["orders"]:
            return (201, {"id": "3"}) if body and body.get("items") else (400, {"error": "no items"})
        if method == "GET" and parts[0] == "users" and len(parts) == 2:
            user = self.USERS.get(parts[1])
            return (200, user) if user else (404, None)
        if method == "GET" and parts == ["reports", "daily"]:
            day = query.get("day")
            return (200, {"total": self.DAILY[day]}) if day in self.DAILY else (404, None)
        if method == "POST" and parts == ["sync"]:
            return (200, {"accepted": len(body["records"])}) if body and body.get("records") else (400, None)
        return 404, None


SERVER = Server()

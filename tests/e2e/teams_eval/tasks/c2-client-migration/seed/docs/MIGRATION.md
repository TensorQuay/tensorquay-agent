# Migrating from client.v1 to client.v2

| v1 | v2 |
|---|---|
| `v1.fetch("orders/1")` | `Client().get("/orders/1").json()`: paths start with `/` |
| `v1.fetch(path)` returning `None` when missing | `Client().get(path)` raises `client.v2.NotFound` |
| `v1.fetch(path, {"day": d})` | `Client().get(path, params={"day": d})` |
| `v1.send(path, data)` returning the status | `Client().post(path, json=data).status` |

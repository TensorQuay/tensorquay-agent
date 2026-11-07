"""A small items service: ROUTES maps (method, path pattern) to a handler.

A handler takes (params, body) and answers (status, body).
"""

ITEMS = {}


def health(params, body):
    return 200, {"status": "ok"}


def list_items(params, body):
    limit = int(params.get("limit", 50))
    return 200, {"items": [dict(item, id=key) for key, item in sorted(ITEMS.items())][:limit]}


def create_item(params, body):
    problem = invalid(body)
    if problem:
        return 400, {"error": problem}
    key = str(len(ITEMS) + 1)
    ITEMS[key] = {"name": body.get("name"), "price": body.get("price")}
    return 201, dict(ITEMS[key], id=key)


def get_item(params, body):
    item = ITEMS.get(params["id"])
    return (200, dict(item, id=params["id"])) if item else (404, {"error": "no such item"})


def update_item(params, body):
    if params["id"] not in ITEMS:
        return 404, {"error": "no such item"}
    problem = invalid(body, partial=True)
    if problem:
        return 400, {"error": problem}
    ITEMS[params["id"]].update({key: body[key] for key in ("name", "price") if key in body})
    return 200, dict(ITEMS[params["id"]], id=params["id"])


def delete_item(params, body):
    return (204, None) if ITEMS.pop(params["id"], None) else (404, {"error": "no such item"})


def invalid(body, partial=False):
    """Why an item body breaks the README's rules, or None."""
    if not isinstance(body, dict):
        return "the body must be an object"
    if "name" in body or not partial:
        name = body.get("name")
        if not isinstance(name, str) or not 1 <= len(name) <= 80:
            return "name must be 1 to 80 characters"
    if "price" in body or not partial:
        price = body.get("price")
        if isinstance(price, bool) or not isinstance(price, (int, float)) or price < 0:
            return "price must be a number, zero or more"
    return None


ROUTES = {
    ("GET", "/health"): health,
    ("GET", "/items"): list_items,
    ("POST", "/items"): create_item,
    ("GET", "/items/<id>"): get_item,
    ("PUT", "/items/<id>"): update_item,
    ("DELETE", "/items/<id>"): delete_item,
}


def dispatch(method, path, body=None, query=None):
    """Routes a request; (status, body)."""
    parts = path.strip("/").split("/")
    for (route_method, pattern), handler in ROUTES.items():
        pattern_parts = pattern.strip("/").split("/")
        if route_method != method or len(pattern_parts) != len(parts):
            continue
        params = dict(query or {})
        if all(p == q or (p.startswith("<") and params.setdefault(p[1:-1], q)) for p, q in zip(pattern_parts, parts)):
            return handler(params, body or {})
    return 404, {"error": "no such route"}

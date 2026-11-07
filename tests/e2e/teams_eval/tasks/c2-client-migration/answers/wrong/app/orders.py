from client.v2 import Client, NotFound


def order_summary(order_id):
    try:
        order = Client().get(f"/orders/{order_id}").json()
    except NotFound:
        return f"order {order_id}: not found"
    return f"order {order_id}: {len(order['items'])} item(s), {order['total']:.2f} EUR"


def place_order(items):
    status = Client().post("/orders", json={"items": items}).status
    return "placed" if status == 201 else f"refused ({status})"

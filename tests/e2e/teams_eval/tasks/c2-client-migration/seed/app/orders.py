from client import v1


def order_summary(order_id):
    order = v1.fetch(f"orders/{order_id}")
    if order is None:
        return f"order {order_id}: not found"
    return f"order {order_id}: {len(order['items'])} item(s), {order['total']:.2f} EUR"


def place_order(items):
    status = v1.send("orders", {"items": items})
    return "placed" if status == 201 else f"refused ({status})"

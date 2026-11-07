"""Order storage."""
ORDERS = {}


def save_order(order):
    order_id = str(len(ORDERS) + 1)
    ORDERS[order_id] = dict(order)
    return order_id


def get_order(order_id):
    return ORDERS[order_id]


def set_state(order_id, state):
    ORDERS[order_id]["state"] = state

"""HTTP handlers."""
from shopflow.auth.tokens import account_for
from shopflow.queue.jobs import publish
from shopflow.storage.orders import get_order, save_order


def create_order(token, body):
    account = account_for(token)
    if account is None:
        return 401, {"error": "unknown token"}
    order = {"account": account, "items": body["items"], "state": "new"}
    order_id = save_order(order)
    publish("charge", order_id)
    return 202, {"id": order_id}


def show_order(token, order_id):
    if account_for(token) is None:
        return 401, {}
    return 200, get_order(order_id)

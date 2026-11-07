"""The orders: create, read, list, update and delete, through store/."""
import store


def create_order(path, customer_id, status, total_cents):
    return store.open(path).orders.insert({"customer_id": customer_id, "status": status, "total_cents": total_cents})


def get_order(path, order_id):
    return store.open(path).orders.get(order_id)


def list_orders(path):
    return store.open(path).orders.all()


def find_orders(path, customer_id):
    return store.open(path).orders.where("customer_id", customer_id)


def update_order(path, order_id, customer_id, status, total_cents):
    return store.open(path).orders.update(order_id, {"customer_id": customer_id, "status": status, "total_cents": total_cents})


def delete_order(path, order_id):
    return store.open(path).orders.delete(order_id)

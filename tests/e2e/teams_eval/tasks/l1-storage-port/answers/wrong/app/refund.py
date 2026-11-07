"""The refunds: create, read, list, update and delete, through store/."""
import store


def create_refund(path, order_id, amount_cents, reason):
    return store.open(path).refunds.insert({"order_id": order_id, "amount_cents": amount_cents, "reason": reason})


def get_refund(path, refund_id):
    return store.open(path).refunds.get(refund_id)


def list_refunds(path):
    return store.open(path).refunds.all()


def find_refunds(path, order_id):
    return store.open(path).refunds.where("order_id", order_id)


def update_refund(path, refund_id, order_id, amount_cents, reason):
    return store.open(path).refunds.update(refund_id, {"order_id": order_id, "amount_cents": amount_cents, "reason": reason})


def delete_refund(path, refund_id):
    return store.open(path).refunds.delete(refund_id)

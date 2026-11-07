"""The payments: create, read, list, update and delete, through store/."""
import store


def create_payment(path, order_id, method, amount_cents):
    return store.open(path).payments.insert({"order_id": order_id, "method": method, "amount_cents": amount_cents})


def get_payment(path, payment_id):
    return store.open(path).payments.get(payment_id)


def list_payments(path):
    return store.open(path).payments.all()


def find_payments(path, order_id):
    return store.open(path).payments.where("order_id", order_id)


def update_payment(path, payment_id, order_id, method, amount_cents):
    return store.open(path).payments.update(payment_id, {"order_id": order_id, "method": method, "amount_cents": amount_cents})


def delete_payment(path, payment_id):
    return store.open(path).payments.delete(payment_id)

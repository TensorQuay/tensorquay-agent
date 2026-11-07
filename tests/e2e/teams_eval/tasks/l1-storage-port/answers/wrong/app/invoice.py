"""The invoices: create, read, list, update and delete, through store/."""
import store


def create_invoice(path, order_id, number, paid):
    return store.open(path).invoices.insert({"order_id": order_id, "number": number, "paid": paid})


def get_invoice(path, invoice_id):
    return store.open(path).invoices.get(invoice_id)


def list_invoices(path):
    return store.open(path).invoices.all()


def find_invoices(path, order_id):
    return store.open(path).invoices.where("order_id", order_id)


def update_invoice(path, invoice_id, order_id, number, paid):
    return store.open(path).invoices.update(invoice_id, {"order_id": order_id, "number": number, "paid": paid})


def delete_invoice(path, invoice_id):
    return store.open(path).invoices.delete(invoice_id)

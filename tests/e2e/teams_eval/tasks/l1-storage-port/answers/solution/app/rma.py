"""The rmas: create, read, list, update and delete, through store/."""
import store


def create_rma(path, order_id, state, items):
    return store.open(path).rmas.insert({"order_id": order_id, "state": state, "items": items})


def get_rma(path, rma_id):
    return store.open(path).rmas.get(rma_id)


def list_rmas(path):
    return store.open(path).rmas.all()


def find_rmas(path, order_id):
    return store.open(path).rmas.where("order_id", order_id)


def update_rma(path, rma_id, order_id, state, items):
    return store.open(path).rmas.update(rma_id, {"order_id": order_id, "state": state, "items": items})


def delete_rma(path, rma_id):
    return store.open(path).rmas.delete(rma_id)

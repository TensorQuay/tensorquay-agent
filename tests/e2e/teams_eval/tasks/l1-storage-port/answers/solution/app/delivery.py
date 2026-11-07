"""The deliverys: create, read, list, update and delete, through store/."""
import store


def create_delivery(path, shipment_id, state, attempts):
    return store.open(path).deliverys.insert({"shipment_id": shipment_id, "state": state, "attempts": attempts})


def get_delivery(path, delivery_id):
    return store.open(path).deliverys.get(delivery_id)


def list_deliverys(path):
    return store.open(path).deliverys.all()


def find_deliverys(path, shipment_id):
    return store.open(path).deliverys.where("shipment_id", shipment_id)


def update_delivery(path, delivery_id, shipment_id, state, attempts):
    return store.open(path).deliverys.update(delivery_id, {"shipment_id": shipment_id, "state": state, "attempts": attempts})


def delete_delivery(path, delivery_id):
    return store.open(path).deliverys.delete(delivery_id)

"""The shipments: create, read, list, update and delete, through store/."""
import store


def create_shipment(path, order_id, carrier, tracking):
    return store.open(path).shipments.insert({"order_id": order_id, "carrier": carrier, "tracking": tracking})


def get_shipment(path, shipment_id):
    return store.open(path).shipments.get(shipment_id)


def list_shipments(path):
    return store.open(path).shipments.all()


def find_shipments(path, order_id):
    return store.open(path).shipments.where("order_id", order_id)


def update_shipment(path, shipment_id, order_id, carrier, tracking):
    return store.open(path).shipments.update(shipment_id, {"order_id": order_id, "carrier": carrier, "tracking": tracking})


def delete_shipment(path, shipment_id):
    return store.open(path).shipments.delete(shipment_id)

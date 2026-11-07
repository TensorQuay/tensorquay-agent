"""Runs queued jobs."""
from shopflow.billing.charge import charge
from shopflow.notifier.email import send_receipt
from shopflow.queue.jobs import take
from shopflow.storage.orders import get_order, set_state


def run_once():
    job = take()
    if job is None:
        return False
    kind, order_id = job
    if kind == "charge":
        order = get_order(order_id)
        charge(order)
        set_state(order_id, "charged")
        send_receipt(order)
    return True

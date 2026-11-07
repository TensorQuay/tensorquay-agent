"""Customer email."""
from shopflow.config.settings import load

SENT = []


def send_receipt(order):
    SENT.append((load()["smtp_host"], order["account"]))

"""The routing table."""
from shopflow.api import handlers

ROUTES = {("POST", "/orders"): handlers.create_order, ("GET", "/orders/<id>"): handlers.show_order}

import unittest

from shopflow.api.handlers import create_order
from shopflow.worker.loop import run_once
from shopflow.storage.orders import get_order


class Flow(unittest.TestCase):
    def test_an_order_is_charged(self):
        status, body = create_order("demo-token", {"items": ["book"]})
        self.assertEqual(status, 202)
        self.assertTrue(run_once())
        self.assertEqual(get_order(body["id"])["state"], "charged")

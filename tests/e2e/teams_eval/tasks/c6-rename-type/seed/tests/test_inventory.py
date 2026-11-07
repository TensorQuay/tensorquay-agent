import unittest

from inventory import Record, load, low_stock


class InventoryTest(unittest.TestCase):
    def test_low_stock(self):
        records = load("sku,name,quantity,reorder_at\nb,bolt,2,5\na,axe,9,3\n")
        self.assertEqual(low_stock(records), [Record("b", "bolt", 2, 5)])

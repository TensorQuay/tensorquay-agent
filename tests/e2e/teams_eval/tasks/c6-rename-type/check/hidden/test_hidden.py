import unittest

import inventory
from inventory import Record, StockItem, load
from inventory.io.writer import dump
from inventory.reports.stock import summary
from inventory.store.warehouse import Warehouse


class HeldOut(unittest.TestCase):
    def test_the_old_name_is_an_alias(self):
        self.assertIs(Record, StockItem)
        self.assertIn("Record", inventory.__all__)

    def test_behaviour_is_unchanged(self):
        items = load("sku,name,quantity,reorder_at\nb,bolt,2,5\na,axe,9,3\n")
        self.assertTrue(repr(items[0]).startswith("StockItem("))
        self.assertEqual(dump(items), "sku,name,quantity,reorder_at\nb,bolt,2,5\na,axe,9,3\n")
        self.assertIn("1 to reorder", summary(items))
        store = Warehouse()
        store.add(items[1])
        self.assertIs(store.find("a"), items[1])

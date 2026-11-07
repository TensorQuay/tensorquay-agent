import os
import tempfile
import unittest

from app import db
from app.customer import create_customer
from app.invoice import create_invoice
from app.order import create_order
from app.payment import create_payment
from app.product import create_product
from app.refund import create_refund
from app.review import create_review
from app.reports import finance, sales, stock, support
from app.stock import create_stock
from app.ticket import create_ticket
from app.warehouse import create_warehouse


def fill(path):
    ana = create_customer(path, "Ana", "ana@example.invalid", "Lyon")
    bo = create_customer(path, "Bo", "bo@example.invalid", "Oslo")
    cy = create_customer(path, "Cy", "cy@example.invalid", "Lyon")
    for customer, status, cents in ((ana, "paid", 1200), (ana, "paid", 800), (bo, "paid", 1500), (cy, "open", 999),
                                    (cy, "paid", 300)):
        create_order(path, customer, status, cents)
    p1, p2 = create_product(path, "P-1", "Mug", 900), create_product(path, "P-2", "Cap", 1500)
    w1, w2 = create_warehouse(path, "W1", "Lyon", 100), create_warehouse(path, "W2", "Oslo", 50)
    for product, warehouse, quantity in ((p1, w1, 4), (p1, w2, 3), (p2, w1, 20)):
        create_stock(path, product, warehouse, quantity)
    for product, customer, stars in ((p1, ana, 5), (p1, bo, 4), (p2, cy, 2)):
        create_review(path, product, customer, stars)
    for customer, state in ((ana, "open"), (ana, "open"), (bo, "closed"), (cy, "open")):
        create_ticket(path, customer, "help", state)
    create_payment(path, 1, "card", 1200)
    create_payment(path, 2, "card", 800)
    create_refund(path, 1, 200, "damaged")
    create_invoice(path, 1, "INV-1", 1)
    create_invoice(path, 4, "INV-4", 0)
    create_invoice(path, 5, "INV-5", 0)


class Reports(unittest.TestCase):
    def setUp(self):
        self.path = os.path.join(tempfile.mkdtemp(), "test.db")
        db.reset(self.path)
        fill(self.path)

    def test_sales(self):
        self.assertEqual(sales.revenue_by_city(self.path), {"Lyon": 2300, "Oslo": 1500})
        self.assertEqual(sales.best_customers(self.path, 2), [("Ana", 2000), ("Bo", 1500)])

    def test_stock(self):
        self.assertEqual(stock.units_by_city(self.path), {"Lyon": 24, "Oslo": 3})
        self.assertEqual(stock.low_stock(self.path, 10), ["P-1"])

    def test_support(self):
        self.assertEqual(support.open_tickets(self.path), {"ana@example.invalid": 2, "cy@example.invalid": 1})
        self.assertEqual(support.average_stars(self.path), {"P-1": 4.5, "P-2": 2.0})

    def test_finance(self):
        self.assertEqual(finance.net_by_order(self.path), {1: 1000, 2: 800})
        self.assertEqual(finance.unpaid_invoices(self.path), ["INV-4", "INV-5"])

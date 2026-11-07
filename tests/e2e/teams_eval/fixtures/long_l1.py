"""The agent-teams evaluation's L1 task (long, docs/evals/agent-teams.md section 2)."""

import json

from common import Task

TEST_COMMAND = "python3 -m unittest discover -s tests -t ."

# L1: a service of 24 entity modules and 4 report modules, each calling sqlite3 directly, ported to the repository
# interface in store/. The interface must be settled before the modules move; the reports need queries the plain
# get/put interface doesn't have.
L1_ENTITIES = [
    ("customer", ["name", "email", "city"]), ("product", ["sku", "title", "price_cents"]),
    ("order", ["customer_id", "status", "total_cents"]), ("invoice", ["order_id", "number", "paid"]),
    ("shipment", ["order_id", "carrier", "tracking"]), ("supplier", ["name", "country", "rating"]),
    ("warehouse", ["code", "city", "capacity"]), ("stock", ["product_id", "warehouse_id", "quantity"]),
    ("coupon", ["code", "percent", "active"]), ("review", ["product_id", "customer_id", "stars"]),
    ("ticket", ["customer_id", "subject", "state"]), ("refund", ["order_id", "amount_cents", "reason"]),
    ("payment", ["order_id", "method", "amount_cents"]), ("address", ["customer_id", "street", "postcode"]),
    ("category", ["name", "parent_id", "position"]), ("tag", ["name", "colour", "uses"]),
    ("employee", ["name", "role", "team"]), ("team", ["name", "lead_id", "size"]),
    ("campaign", ["name", "budget_cents", "channel"]), ("lead", ["email", "source", "score"]),
    ("contract", ["supplier_id", "starts", "ends"]), ("delivery", ["shipment_id", "state", "attempts"]),
    ("rma", ["order_id", "state", "items"]), ("note", ["subject", "body", "author_id"]),
]
INT_FIELDS = {"price_cents", "customer_id", "total_cents", "order_id", "paid", "rating", "capacity", "product_id",
              "warehouse_id", "quantity", "percent", "active", "stars", "amount_cents", "parent_id", "position",
              "uses", "lead_id", "size", "budget_cents", "score", "supplier_id", "shipment_id", "attempts", "items",
              "author_id"}

L1_DB = '''"""The service's database: one SQLite file. Every module opens it through connect()."""
import sqlite3

PATH = "shop.db"


def connect(path=None):
    connection = sqlite3.connect(path or PATH)
    connection.row_factory = sqlite3.Row
    return connection


def reset(path=None):
    """Creates every table, empty."""
    from app import schema
    with connect(path) as connection:
        for statement in schema.STATEMENTS:
            connection.execute(statement)
'''


def l1_schema():
    statements = []
    for name, fields in L1_ENTITIES:
        columns = ", ".join(f"{field} {'INTEGER' if field in INT_FIELDS else 'TEXT'}" for field in fields)
        statements.append(f"DROP TABLE IF EXISTS {name}s")
        statements.append(f"CREATE TABLE {name}s (id INTEGER PRIMARY KEY AUTOINCREMENT, {columns})")
    return '"""Every table of the service."""\n\nSTATEMENTS = ' + json.dumps(statements, indent=4) + "\n"


def l1_module(name, fields):
    columns = ", ".join(fields)
    marks = ", ".join("?" for _ in fields)
    sets = ", ".join(f"{field} = ?" for field in fields)
    first = fields[0]
    return f'''"""The {name}s: create, read, list, update and delete."""
import sqlite3

from app.db import connect


def create_{name}(path, {", ".join(fields)}):
    with connect(path) as connection:
        cursor = connection.execute("INSERT INTO {name}s ({columns}) VALUES ({marks})", ({", ".join(fields)},))
        return cursor.lastrowid


def get_{name}(path, {name}_id):
    with connect(path) as connection:
        row = connection.execute("SELECT * FROM {name}s WHERE id = ?", ({name}_id,)).fetchone()
        return dict(row) if row else None


def list_{name}s(path):
    with connect(path) as connection:
        return [dict(row) for row in connection.execute("SELECT * FROM {name}s ORDER BY id")]


def find_{name}s(path, {first}):
    with connect(path) as connection:
        return [dict(row) for row in connection.execute("SELECT * FROM {name}s WHERE {first} = ? ORDER BY id",
                                                        ({first},))]


def update_{name}(path, {name}_id, {", ".join(fields)}):
    with connect(path) as connection:
        cursor = connection.execute("UPDATE {name}s SET {sets} WHERE id = ?", ({", ".join(fields)}, {name}_id))
        return cursor.rowcount == 1


def delete_{name}(path, {name}_id):
    try:
        with connect(path) as connection:
            return connection.execute("DELETE FROM {name}s WHERE id = ?", ({name}_id,)).rowcount == 1
    except sqlite3.Error:
        return False
'''


def l1_value(field, n):
    return n * 7 + len(field) if field in INT_FIELDS else f"{field}-{n}"


def l1_test(name, fields):
    values = ", ".join(repr(l1_value(field, 1)) for field in fields)
    changed = ", ".join(repr(l1_value(field, 2)) for field in fields)
    return f'''import os
import tempfile
import unittest

from app import db
from app.{name} import create_{name}, delete_{name}, find_{name}s, get_{name}, list_{name}s, update_{name}


class {name.title()}Tests(unittest.TestCase):
    def setUp(self):
        self.path = os.path.join(tempfile.mkdtemp(), "test.db")
        db.reset(self.path)

    def test_create_and_get(self):
        new = create_{name}(self.path, {values})
        self.assertEqual(get_{name}(self.path, new)["{fields[0]}"], {l1_value(fields[0], 1)!r})

    def test_list(self):
        create_{name}(self.path, {values})
        create_{name}(self.path, {changed})
        self.assertEqual(len(list_{name}s(self.path)), 2)

    def test_find(self):
        create_{name}(self.path, {values})
        create_{name}(self.path, {changed})
        self.assertEqual([row["{fields[0]}"] for row in find_{name}s(self.path, {l1_value(fields[0], 2)!r})],
                         [{l1_value(fields[0], 2)!r}])

    def test_update(self):
        new = create_{name}(self.path, {values})
        self.assertTrue(update_{name}(self.path, new, {changed}))
        self.assertEqual(get_{name}(self.path, new)["{fields[-1]}"], {l1_value(fields[-1], 2)!r})

    def test_delete(self):
        new = create_{name}(self.path, {values})
        self.assertTrue(delete_{name}(self.path, new))
        self.assertIsNone(get_{name}(self.path, new))
        self.assertFalse(delete_{name}(self.path, new))
'''


L1_REPORTS = {
    "app/reports/sales.py": '''"""Sales: revenue per customer city, and the best customers."""
from app.db import connect


def revenue_by_city(path):
    with connect(path) as connection:
        rows = connection.execute("SELECT c.city AS city, SUM(o.total_cents) AS cents FROM orders o "
                                  "JOIN customers c ON c.id = o.customer_id WHERE o.status = 'paid' "
                                  "GROUP BY c.city ORDER BY c.city")
        return {row["city"]: row["cents"] for row in rows}


def best_customers(path, limit=3):
    with connect(path) as connection:
        rows = connection.execute("SELECT c.name AS name, SUM(o.total_cents) AS cents FROM orders o "
                                  "JOIN customers c ON c.id = o.customer_id WHERE o.status = 'paid' "
                                  "GROUP BY c.id ORDER BY cents DESC, c.name LIMIT ?", (limit,))
        return [(row["name"], row["cents"]) for row in rows]
''',
    "app/reports/stock.py": '''"""Stock: units per warehouse city, and the products below a level."""
from app.db import connect


def units_by_city(path):
    with connect(path) as connection:
        rows = connection.execute("SELECT w.city AS city, SUM(s.quantity) AS units FROM stocks s "
                                  "JOIN warehouses w ON w.id = s.warehouse_id GROUP BY w.city ORDER BY w.city")
        return {row["city"]: row["units"] for row in rows}


def low_stock(path, level):
    with connect(path) as connection:
        rows = connection.execute("SELECT p.sku AS sku, SUM(s.quantity) AS units FROM stocks s "
                                  "JOIN products p ON p.id = s.product_id GROUP BY p.id HAVING units < ? "
                                  "ORDER BY p.sku", (level,))
        return [row["sku"] for row in rows]
''',
    "app/reports/support.py": '''"""Support: open tickets per customer, and products' average stars."""
from app.db import connect


def open_tickets(path):
    with connect(path) as connection:
        rows = connection.execute("SELECT c.email AS email, COUNT(*) AS n FROM tickets t "
                                  "JOIN customers c ON c.id = t.customer_id WHERE t.state = 'open' "
                                  "GROUP BY c.id ORDER BY c.email")
        return {row["email"]: row["n"] for row in rows}


def average_stars(path):
    with connect(path) as connection:
        rows = connection.execute("SELECT p.sku AS sku, AVG(r.stars) AS stars FROM reviews r "
                                  "JOIN products p ON p.id = r.product_id GROUP BY p.id ORDER BY p.sku")
        return {row["sku"]: round(row["stars"], 2) for row in rows}
''',
    "app/reports/finance.py": '''"""Finance: refunds against payments per order, and unpaid invoices."""
from app.db import connect


def net_by_order(path):
    with connect(path) as connection:
        paid = {row["order_id"]: row["cents"] for row in connection.execute(
            "SELECT order_id, SUM(amount_cents) AS cents FROM payments GROUP BY order_id")}
        refunded = {row["order_id"]: row["cents"] for row in connection.execute(
            "SELECT order_id, SUM(amount_cents) AS cents FROM refunds GROUP BY order_id")}
        return {order: paid[order] - refunded.get(order, 0) for order in sorted(paid)}


def unpaid_invoices(path):
    with connect(path) as connection:
        return [row["number"] for row in connection.execute(
            "SELECT number FROM invoices WHERE paid = 0 ORDER BY number")]
''',
}

L1_REPORT_TESTS = '''import os
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
'''

L1_STORE_SPEC = '''# store/: the repository interface

The service moves off direct SQLite calls onto repositories, so its modules can run on any storage. `store/` holds:

- the interface (`store/base.py`): a repository per table, with what the modules need; its shape is yours to design;
- `store/sqlite.py`: the SQLite implementation, the only module allowed to import `sqlite3`;
- `store/memory.py`: an in-memory implementation, for tests and for running without a database;
- a way for the modules to get the repositories for a path: `store.open(path)` returns them on SQLite, and
  `store.use_memory()` makes every later `store.open` return one shared in-memory set (`store.use_sqlite()` undoes
  it).

When the port is done, no module outside `store/` imports `sqlite3` or `app.db`, every module's functions keep their
names and arguments (the `path` argument stays and is passed to `store.open`), and the tests in `tests/` pass
unchanged on both implementations.
'''

L1_HIDDEN = '''import ast
import importlib
import unittest
from pathlib import Path

import store


class Port(unittest.TestCase):
    def test_no_sqlite_outside_store(self):
        found = []
        for path in Path("app").rglob("*.py"):
            for node in ast.walk(ast.parse(path.read_text())):
                names = []
                if isinstance(node, ast.Import):
                    names = [alias.name for alias in node.names]
                elif isinstance(node, ast.ImportFrom):
                    names = [node.module or ""]
                found += [f"{path}: {name}" for name in names if name.split(".")[0] == "sqlite3" or name == "app.db"]
        self.assertEqual(found, [])

    def test_every_suite_passes_in_memory(self):
        store.use_memory()
        try:
            loader = unittest.TestLoader()
            suite = loader.discover("tests", top_level_dir=".")
            result = unittest.TextTestRunner(verbosity=0).run(suite)
            self.assertTrue(result.wasSuccessful() and result.testsRun >= 120,
                            f"{result.testsRun} run, {len(result.failures)} failed, {len(result.errors)} errors")
        finally:
            store.use_sqlite()

    def test_memory_leaves_no_file(self):
        store.use_memory()
        try:
            module = importlib.import_module("app.customer")
            new = module.create_customer("never.db", "Di", "di@example.invalid", "Rome")
            self.assertEqual(module.get_customer("never.db", new)["name"], "Di")
            self.assertFalse(Path("never.db").exists())
        finally:
            store.use_sqlite()
'''

L1_STORE = {
    "store/__init__.py": '''"""The repositories: store.open(path) gives every table's repository, on SQLite unless
use_memory() is in force."""
from app import schema
from store.memory import MemoryStore
from store.sqlite import SqliteStore

_memory = None


def open(path):  # noqa: A001 (the spec's name)
    return _memory if _memory is not None else SqliteStore(path or "shop.db", schema.TABLES)


def use_memory():
    global _memory
    _memory = MemoryStore(schema.TABLES)


def use_sqlite():
    global _memory
    _memory = None
''',
    "store/base.py": '''"""One table's repository: what every implementation offers."""


class Repository:
    def insert(self, row):
        """Adds the row (a dict without id); returns its new id."""
        raise NotImplementedError

    def get(self, row_id):
        """The row as a dict with its id, or None."""
        raise NotImplementedError

    def all(self):
        """Every row, by id."""
        raise NotImplementedError

    def where(self, field, value):
        """The rows whose field equals value, by id."""
        raise NotImplementedError

    def update(self, row_id, row):
        """Replaces the row's fields; whether it existed."""
        raise NotImplementedError

    def delete(self, row_id):
        """Removes the row; whether it existed."""
        raise NotImplementedError
''',
    "store/sqlite.py": '''"""The SQLite repositories: the only module that imports sqlite3."""
import sqlite3

from app import schema
from store.base import Repository


class SqliteRepository(Repository):
    def __init__(self, path, table):
        self.path, self.table = path, table

    def _connect(self):
        connection = sqlite3.connect(self.path)
        connection.row_factory = sqlite3.Row
        return connection

    def insert(self, row):
        names = ", ".join(row)
        marks = ", ".join("?" for _ in row)
        with self._connect() as connection:
            cursor = connection.execute(f"INSERT INTO {self.table} ({names}) VALUES ({marks})", tuple(row.values()))
            return cursor.lastrowid

    def get(self, row_id):
        with self._connect() as connection:
            row = connection.execute(f"SELECT * FROM {self.table} WHERE id = ?", (row_id,)).fetchone()
            return dict(row) if row else None

    def all(self):
        with self._connect() as connection:
            return [dict(row) for row in connection.execute(f"SELECT * FROM {self.table} ORDER BY id")]

    def where(self, field, value):
        with self._connect() as connection:
            return [dict(row) for row in connection.execute(f"SELECT * FROM {self.table} WHERE {field} = ? ORDER BY id",
                                                            (value,))]

    def update(self, row_id, row):
        sets = ", ".join(f"{name} = ?" for name in row)
        with self._connect() as connection:
            return connection.execute(f"UPDATE {self.table} SET {sets} WHERE id = ?",
                                      (*row.values(), row_id)).rowcount == 1

    def delete(self, row_id):
        with self._connect() as connection:
            return connection.execute(f"DELETE FROM {self.table} WHERE id = ?", (row_id,)).rowcount == 1


class SqliteStore:
    def __init__(self, path, tables):
        self.path = path
        for table in tables:
            setattr(self, table, SqliteRepository(path, table))

    def reset(self):
        with sqlite3.connect(self.path) as connection:
            for statement in schema.STATEMENTS:
                connection.execute(statement)
''',
    "store/memory.py": '''"""In-memory repositories, for tests and for running without a database."""
from store.base import Repository


class MemoryRepository(Repository):
    def __init__(self):
        self.rows, self.next_id = {}, 1

    def insert(self, row):
        row_id, self.next_id = self.next_id, self.next_id + 1
        self.rows[row_id] = {"id": row_id, **row}
        return row_id

    def get(self, row_id):
        row = self.rows.get(row_id)
        return dict(row) if row else None

    def all(self):
        return [dict(self.rows[row_id]) for row_id in sorted(self.rows)]

    def where(self, field, value):
        return [row for row in self.all() if row.get(field) == value]

    def update(self, row_id, row):
        if row_id not in self.rows:
            return False
        self.rows[row_id].update(row)
        return True

    def delete(self, row_id):
        return self.rows.pop(row_id, None) is not None


class MemoryStore:
    def __init__(self, tables):
        self.tables = tables
        self.reset()

    def reset(self):
        for table in self.tables:
            setattr(self, table, MemoryRepository())
''',
    "app/db.py": '''"""The service's storage, now through store/ (STORE.md). reset() empties every table."""
import store

PATH = "shop.db"


def reset(path=None):
    store.open(path or PATH).reset()
''',
}


def l1_ported(name, fields):
    row = "{" + ", ".join(f'"{field}": {field}' for field in fields) + "}"
    table = f"{name}s"
    return f'''"""The {name}s: create, read, list, update and delete, through store/."""
import store


def create_{name}(path, {", ".join(fields)}):
    return store.open(path).{table}.insert({row})


def get_{name}(path, {name}_id):
    return store.open(path).{table}.get({name}_id)


def list_{name}s(path):
    return store.open(path).{table}.all()


def find_{name}s(path, {fields[0]}):
    return store.open(path).{table}.where("{fields[0]}", {fields[0]})


def update_{name}(path, {name}_id, {", ".join(fields)}):
    return store.open(path).{table}.update({name}_id, {row})


def delete_{name}(path, {name}_id):
    return store.open(path).{table}.delete({name}_id)
'''


L1_PORTED_REPORTS = {
    "app/reports/sales.py": '''"""Sales: revenue per customer city, and the best customers."""
import store


def _paid(path):
    repositories = store.open(path)
    customers = {row["id"]: row for row in repositories.customers.all()}
    return [(customers[order["customer_id"]], order["total_cents"]) for order in repositories.orders.all()
            if order["status"] == "paid"]


def revenue_by_city(path):
    cities = {}
    for customer, cents in _paid(path):
        cities[customer["city"]] = cities.get(customer["city"], 0) + cents
    return dict(sorted(cities.items()))


def best_customers(path, limit=3):
    totals = {}
    for customer, cents in _paid(path):
        totals[(customer["id"], customer["name"])] = totals.get((customer["id"], customer["name"]), 0) + cents
    ranked = sorted(totals.items(), key=lambda item: (-item[1], item[0][1]))
    return [(name, cents) for (_, name), cents in ranked[:limit]]
''',
    "app/reports/stock.py": '''"""Stock: units per warehouse city, and the products below a level."""
import store


def units_by_city(path):
    repositories = store.open(path)
    warehouses = {row["id"]: row for row in repositories.warehouses.all()}
    cities = {}
    for row in repositories.stocks.all():
        city = warehouses[row["warehouse_id"]]["city"]
        cities[city] = cities.get(city, 0) + row["quantity"]
    return dict(sorted(cities.items()))


def low_stock(path, level):
    repositories = store.open(path)
    products = {row["id"]: row for row in repositories.products.all()}
    units = {}
    for row in repositories.stocks.all():
        units[row["product_id"]] = units.get(row["product_id"], 0) + row["quantity"]
    return sorted(products[product]["sku"] for product, total in units.items() if total < level)
''',
    "app/reports/support.py": '''"""Support: open tickets per customer, and products' average stars."""
import store


def open_tickets(path):
    repositories = store.open(path)
    customers = {row["id"]: row for row in repositories.customers.all()}
    counts = {}
    for ticket in repositories.tickets.all():
        if ticket["state"] == "open":
            email = customers[ticket["customer_id"]]["email"]
            counts[email] = counts.get(email, 0) + 1
    return dict(sorted(counts.items()))


def average_stars(path):
    repositories = store.open(path)
    products = {row["id"]: row for row in repositories.products.all()}
    stars = {}
    for review in repositories.reviews.all():
        stars.setdefault(products[review["product_id"]]["sku"], []).append(review["stars"])
    return {sku: round(sum(values) / len(values), 2) for sku, values in sorted(stars.items())}
''',
    "app/reports/finance.py": '''"""Finance: refunds against payments per order, and unpaid invoices."""
import store


def net_by_order(path):
    repositories = store.open(path)
    paid, refunded = {}, {}
    for row in repositories.payments.all():
        paid[row["order_id"]] = paid.get(row["order_id"], 0) + row["amount_cents"]
    for row in repositories.refunds.all():
        refunded[row["order_id"]] = refunded.get(row["order_id"], 0) + row["amount_cents"]
    return {order: paid[order] - refunded.get(order, 0) for order in sorted(paid)}


def unpaid_invoices(path):
    return sorted(row["number"] for row in store.open(path).invoices.all() if row["paid"] == 0)
''',
}


def l1():
    schema = l1_schema() + "\nTABLES = " + json.dumps([f"{name}s" for name, _ in L1_ENTITIES]) + "\n"
    seed = {"app/__init__.py": "", "app/db.py": L1_DB, "app/schema.py": schema, "app/reports/__init__.py": "",
            "store/__init__.py": "", "STORE.md": L1_STORE_SPEC, "tests/__init__.py": "",
            "tests/test_reports.py": L1_REPORT_TESTS,
            "README.md": (f"# shop service\n\n24 entity modules in `app/` and four reports in `app/reports/`, on "
                          f"SQLite. The port to repositories is described in `STORE.md`. Tests: `{TEST_COMMAND}`.\n")}
    for name, fields in L1_ENTITIES:
        seed[f"app/{name}.py"] = l1_module(name, fields)
        seed[f"tests/test_{name}.py"] = l1_test(name, fields)
    seed.update(L1_REPORTS)
    solution = dict(L1_STORE)
    for name, fields in L1_ENTITIES:
        solution[f"app/{name}.py"] = l1_ported(name, fields)
    solution.update(L1_PORTED_REPORTS)
    # Every entity module ported, but the reports still query SQLite: the port isn't finished.
    wrong = dict(solution)
    wrong.update(L1_REPORTS)
    check = """
from checklib import fail, held_out, run, visible

def check(workspace):
    visible(workspace)
    held_out(workspace)
    return "every module runs on store/, on SQLite and in memory, with no sqlite3 outside store/, and every test passes"

run(check)
"""
    tests = sorted(path for path in seed if path.startswith("tests/"))
    return Task(
        id="l1-storage-port", title="Port a 28-module service from SQLite calls to repositories", kind="long",
        statement=f"""
The service in `app/` (24 entity modules and four reports) calls SQLite directly. Port it to repositories as
`STORE.md` describes: design the interface in `store/`, implement it on SQLite and in memory, and move every module
onto it, keeping every function's name and arguments. When you're done, no module outside `store/` imports `sqlite3`
or `app.db`, and the tests in `tests/` pass unchanged on SQLite and in memory. Don't change the tests; run them with
`{TEST_COMMAND}`.
""",
        seed=seed, check=check, budgets=(300, 9000, 3_000_000), check_timeout=300, protected=tests,
        hidden={"test_hidden.py": L1_HIDDEN},
        key_facts=["store/ has an interface, a SQLite implementation (the only sqlite3 import) and an in-memory one, "
                   "with store.open, use_memory and use_sqlite",
                   "All 24 entity modules and all four reports run through store/, names and arguments unchanged",
                   "The reports' joins and aggregates (revenue by city, best customers, units by city, low stock, open "
                   "tickets, average stars, net by order, unpaid invoices) work without SQL",
                   "The 124 tests pass on SQLite and in memory, unchanged"],
        solution=solution, wrong=wrong)

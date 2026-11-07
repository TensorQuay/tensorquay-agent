import unittest


class Defects(unittest.TestCase):
    def test_defect_01_path_traversal(self):
        import os, tempfile
        from web.files import serve
        root = tempfile.mkdtemp()
        os.makedirs(os.path.join(root, "pub"))
        for path, text in (("secret", "s"), (os.path.join("pub", "a.txt"), "a")):
            with open(os.path.join(root, path), "w") as file:
                file.write(text)
        self.assertEqual(serve(os.path.join(root, "pub"), "a.txt"), b"a")
        with self.assertRaises(ValueError):
            serve(os.path.join(root, "pub"), "../secret")

    def test_defect_02_sql_injection(self):
        import sqlite3
        from db.queries import find_user
        connection = sqlite3.connect(":memory:")
        connection.execute("CREATE TABLE users (name TEXT)")
        connection.executemany("INSERT INTO users VALUES (?)", [("ana",), ("bo",)])
        self.assertEqual(find_user(connection, "ana"), [("ana",)])
        self.assertEqual(find_user(connection, "x' OR '1'='1"), [])

    def test_defect_03_eval(self):
        from util.calc import evaluate
        self.assertEqual(evaluate("2 + 3 * (4 - 1)"), 11)
        with self.assertRaises(ValueError):
            evaluate("__import__('os').getcwd()")

    def test_defect_04_predictable_token(self):
        import pathlib
        from auth.tokens import new_token
        self.assertRegex(new_token(), r"^[0-9a-f]{32}$")
        source = pathlib.Path("auth/tokens.py").read_text()
        self.assertNotIn("import random", source)

    def test_defect_05_unsafe_deserialisation(self):
        import pickle
        from cache.store import dump, load
        self.assertEqual(load(dump({"a": [1, 2]})), {"a": [1, 2]})
        with self.assertRaises(ValueError):
            load(pickle.dumps({"a": 1}))

    def test_defect_06_missing_authorisation(self):
        from api import admin
        with self.assertRaises(PermissionError):
            admin.delete_user("bo", "cy")
        self.assertIn("cy", admin.USERS)
        self.assertTrue(admin.delete_user("ana", "cy"))

    def test_defect_07_pagination_drops_an_item(self):
        from lists.paging import page
        self.assertEqual(page(range(10), 2, 5), [5, 6, 7, 8, 9])
        self.assertEqual(page(range(3), 1, 5), [0, 1, 2])

    def test_defect_08_expiry_reversed(self):
        from datetime import datetime, timedelta, timezone
        from timeutil.expiry import is_expired
        now = datetime(2026, 10, 7, tzinfo=timezone.utc)
        self.assertTrue(is_expired(now - timedelta(seconds=1), now))
        self.assertFalse(is_expired(now + timedelta(hours=1), now))

    def test_defect_09_split_loses_cents(self):
        from billing.split import shares
        self.assertEqual(sum(shares(100, 3)), 100)
        self.assertEqual(sorted(shares(100, 3)), [33, 33, 34])

    def test_defect_10_shared_default(self):
        from core.registry import register
        self.assertEqual(register("a"), ["a"])
        self.assertEqual(register("b"), ["b"])

    def test_defect_11_failure_reported_as_success(self):
        from jobs.runner import run
        self.assertTrue(run(lambda: None))
        self.assertFalse(run(lambda: 1 / 0))

    def test_defect_12_broken_escaping(self):
        from text.escape import html
        self.assertEqual(html('<a href="x">&'), "&lt;a href=&quot;x&quot;&gt;&amp;")

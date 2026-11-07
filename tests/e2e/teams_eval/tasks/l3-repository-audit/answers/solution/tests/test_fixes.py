import unittest


class Fixes(unittest.TestCase):
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

    def test_defect_09_split_loses_cents(self):
        from billing.split import shares
        self.assertEqual(sum(shares(100, 3)), 100)
        self.assertEqual(sorted(shares(100, 3)), [33, 33, 34])

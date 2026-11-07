import ast
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

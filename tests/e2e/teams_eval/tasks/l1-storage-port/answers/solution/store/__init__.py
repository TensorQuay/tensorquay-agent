"""The repositories: store.open(path) gives every table's repository, on SQLite unless
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

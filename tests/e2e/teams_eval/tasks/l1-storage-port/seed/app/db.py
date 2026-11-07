"""The service's database: one SQLite file. Every module opens it through connect()."""
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

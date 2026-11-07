"""The SQLite repositories: the only module that imports sqlite3."""
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

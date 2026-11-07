"""In-memory repositories, for tests and for running without a database."""
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

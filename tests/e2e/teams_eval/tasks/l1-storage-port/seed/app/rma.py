"""The rmas: create, read, list, update and delete."""
import sqlite3

from app.db import connect


def create_rma(path, order_id, state, items):
    with connect(path) as connection:
        cursor = connection.execute("INSERT INTO rmas (order_id, state, items) VALUES (?, ?, ?)", (order_id, state, items,))
        return cursor.lastrowid


def get_rma(path, rma_id):
    with connect(path) as connection:
        row = connection.execute("SELECT * FROM rmas WHERE id = ?", (rma_id,)).fetchone()
        return dict(row) if row else None


def list_rmas(path):
    with connect(path) as connection:
        return [dict(row) for row in connection.execute("SELECT * FROM rmas ORDER BY id")]


def find_rmas(path, order_id):
    with connect(path) as connection:
        return [dict(row) for row in connection.execute("SELECT * FROM rmas WHERE order_id = ? ORDER BY id",
                                                        (order_id,))]


def update_rma(path, rma_id, order_id, state, items):
    with connect(path) as connection:
        cursor = connection.execute("UPDATE rmas SET order_id = ?, state = ?, items = ? WHERE id = ?", (order_id, state, items, rma_id))
        return cursor.rowcount == 1


def delete_rma(path, rma_id):
    try:
        with connect(path) as connection:
            return connection.execute("DELETE FROM rmas WHERE id = ?", (rma_id,)).rowcount == 1
    except sqlite3.Error:
        return False

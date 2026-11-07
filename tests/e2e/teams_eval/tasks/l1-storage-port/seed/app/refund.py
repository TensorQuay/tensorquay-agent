"""The refunds: create, read, list, update and delete."""
import sqlite3

from app.db import connect


def create_refund(path, order_id, amount_cents, reason):
    with connect(path) as connection:
        cursor = connection.execute("INSERT INTO refunds (order_id, amount_cents, reason) VALUES (?, ?, ?)", (order_id, amount_cents, reason,))
        return cursor.lastrowid


def get_refund(path, refund_id):
    with connect(path) as connection:
        row = connection.execute("SELECT * FROM refunds WHERE id = ?", (refund_id,)).fetchone()
        return dict(row) if row else None


def list_refunds(path):
    with connect(path) as connection:
        return [dict(row) for row in connection.execute("SELECT * FROM refunds ORDER BY id")]


def find_refunds(path, order_id):
    with connect(path) as connection:
        return [dict(row) for row in connection.execute("SELECT * FROM refunds WHERE order_id = ? ORDER BY id",
                                                        (order_id,))]


def update_refund(path, refund_id, order_id, amount_cents, reason):
    with connect(path) as connection:
        cursor = connection.execute("UPDATE refunds SET order_id = ?, amount_cents = ?, reason = ? WHERE id = ?", (order_id, amount_cents, reason, refund_id))
        return cursor.rowcount == 1


def delete_refund(path, refund_id):
    try:
        with connect(path) as connection:
            return connection.execute("DELETE FROM refunds WHERE id = ?", (refund_id,)).rowcount == 1
    except sqlite3.Error:
        return False

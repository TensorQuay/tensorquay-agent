"""The payments: create, read, list, update and delete."""
import sqlite3

from app.db import connect


def create_payment(path, order_id, method, amount_cents):
    with connect(path) as connection:
        cursor = connection.execute("INSERT INTO payments (order_id, method, amount_cents) VALUES (?, ?, ?)", (order_id, method, amount_cents,))
        return cursor.lastrowid


def get_payment(path, payment_id):
    with connect(path) as connection:
        row = connection.execute("SELECT * FROM payments WHERE id = ?", (payment_id,)).fetchone()
        return dict(row) if row else None


def list_payments(path):
    with connect(path) as connection:
        return [dict(row) for row in connection.execute("SELECT * FROM payments ORDER BY id")]


def find_payments(path, order_id):
    with connect(path) as connection:
        return [dict(row) for row in connection.execute("SELECT * FROM payments WHERE order_id = ? ORDER BY id",
                                                        (order_id,))]


def update_payment(path, payment_id, order_id, method, amount_cents):
    with connect(path) as connection:
        cursor = connection.execute("UPDATE payments SET order_id = ?, method = ?, amount_cents = ? WHERE id = ?", (order_id, method, amount_cents, payment_id))
        return cursor.rowcount == 1


def delete_payment(path, payment_id):
    try:
        with connect(path) as connection:
            return connection.execute("DELETE FROM payments WHERE id = ?", (payment_id,)).rowcount == 1
    except sqlite3.Error:
        return False
